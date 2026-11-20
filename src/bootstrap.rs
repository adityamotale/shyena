//! Application wiring: global hotkey registration and async message pumps.
//!
//! Both pumps live for the lifetime of the process. Each received event is
//! marshalled onto GPUI's UI thread through `cx.update`; the shared context is
//! cloned per event so a slow UI update cannot block the next message.

use std::{cell::RefCell, rc::Rc, thread};

use async_channel::{Receiver, Sender};
use global_hotkey::{
    GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState,
    hotkey::{Code, HotKey, Modifiers},
};
use gpui::App;

use crate::config::ConfigStore;
use crate::dispatch::PsOutcome;
use crate::effects::Effects;
use crate::ui::{self, ActiveWindow};

/// Handles shared by both pumps and the composition root.
pub(crate) struct LauncherContext {
    pub(crate) active_window: ActiveWindow,
    pub(crate) config_store: Rc<RefCell<ConfigStore>>,
    pub(crate) ps_outcome_tx: Sender<PsOutcome>,
    pub(crate) effects: Effects,
}

/// Registers the launcher toggle hotkey and forwards presses to the UI channel from
/// a dedicated thread. Release builds register Alt+Backquote; debug builds register
/// Alt+Shift+Backquote so development builds do not clash with installed releases.
pub(crate) fn setup_global_hotkey(tx: Sender<()>) -> GlobalHotKeyManager {
    let manager = GlobalHotKeyManager::new().expect("Failed to initialize GlobalHotKeyManager");

    #[cfg(debug_assertions)]
    let (modifiers, label) = (Modifiers::ALT | Modifiers::SHIFT, "Alt+Shift+Backquote");
    #[cfg(not(debug_assertions))]
    let (modifiers, label) = (Modifiers::ALT, "Alt+Backquote");

    let hotkey = HotKey::new(Some(modifiers), Code::Backquote);
    let hotkey_id = hotkey.id();
    manager
        .register(hotkey)
        .unwrap_or_else(|_| panic!("Failed to register {label} global hotkey"));

    let event_receiver = GlobalHotKeyEvent::receiver().clone();
    thread::spawn(move || {
        while let Ok(event) = event_receiver.recv() {
            if event.id == hotkey_id && event.state == HotKeyState::Pressed {
                let _ = tx.send_blocking(());
            }
        }
    });

    manager
}

/// Surfaces asynchronous PowerShell outcomes (output or error) in the launcher itself.
/// If the bar was already dismissed, open a fresh window.
pub(crate) fn spawn_outcome_pump(cx: &mut App, rx: Receiver<PsOutcome>, ctx: &LauncherContext) {
    let active = ctx.active_window.clone();
    let ps_outcome_tx = ctx.ps_outcome_tx.clone();
    let store = ctx.config_store.clone();
    let effects = ctx.effects.clone();

    cx.spawn(async move |cx| {
        while let Ok(outcome) = rx.recv().await {
            let active = active.clone();
            let ps_outcome_tx = ps_outcome_tx.clone();
            let store = store.clone();
            let effects = effects.clone();
            let _ = cx.update(move |cx| {
                let handle = active.borrow().as_ref().copied();
                let handle = if let Some(handle) = handle {
                    handle
                } else {
                    let handle =
                        ui::open_search_window(cx, &active, ps_outcome_tx.clone(), &store, &effects);
                    *active.borrow_mut() = Some(handle);
                    handle
                };

                let update_result = handle.update(cx, |bar, window, cx| match &outcome {
                    PsOutcome::Error(message) => {
                        bar.show_error(message.clone(), window, cx);
                    }
                    PsOutcome::Output { name, text } => {
                        bar.show_ps_output(name.clone(), text.clone(), window, cx);
                    }
                });

                if update_result.is_err() {
                    let handle =
                        ui::open_search_window(cx, &active, ps_outcome_tx, &store, &effects);
                    *active.borrow_mut() = Some(handle);
                    let _ = handle.update(cx, |bar, window, cx| match outcome {
                        PsOutcome::Error(message) => {
                            bar.show_error(message, window, cx);
                        }
                        PsOutcome::Output { name, text } => {
                            bar.show_ps_output(name, text, window, cx);
                        }
                    });
                }
            });
        }
    })
    .detach();
}

/// Turns hotkey events into launcher toggles on the UI thread.
///
/// `keep_alive` holds the real `GlobalHotKeyManager` for the process lifetime;
/// tests pass `()` so no OS-level hotkey is ever registered.
pub(crate) fn spawn_hotkey_pump<K: 'static>(
    cx: &mut App,
    rx: Receiver<()>,
    ctx: &LauncherContext,
    keep_alive: K,
) {
    let active = ctx.active_window.clone();
    let ps_outcome_tx = ctx.ps_outcome_tx.clone();
    let store = ctx.config_store.clone();
    let effects = ctx.effects.clone();

    cx.spawn(async move |cx| {
        let _keep_alive = keep_alive;
        while let Ok(()) = rx.recv().await {
            let active = active.clone();
            let ps_outcome_tx = ps_outcome_tx.clone();
            let store = store.clone();
            let effects = effects.clone();
            let _ = cx.update(move |cx| {
                ui::toggle_search_window(cx, &active, &ps_outcome_tx, &store, &effects);
            });
        }
    })
    .detach();
}

#[cfg(test)]
mod tests {
    use std::{sync::Arc, time::Duration};

    use gpui::TestAppContext;

    use super::*;
    use crate::test_support::{RecordingEffects, TestConfig, UI_CONFIG};

    struct PumpHarness {
        ctx: LauncherContext,
        hotkey_tx: Sender<()>,
        ps_tx: Sender<PsOutcome>,
        // Kept alive so the temp config directory survives the test.
        _config: TestConfig,
        effects: Arc<RecordingEffects>,
    }

    fn harness(cx: &TestAppContext) -> PumpHarness {
        let config = TestConfig::new("pump", UI_CONFIG);
        let effects = RecordingEffects::new();
        let (hotkey_tx, hotkey_rx) = async_channel::unbounded();
        let (ps_tx, ps_rx) = async_channel::unbounded();
        let ctx = LauncherContext {
            active_window: Rc::new(RefCell::new(None)),
            config_store: config.store.clone(),
            ps_outcome_tx: ps_tx.clone(),
            effects: effects.clone(),
        };
        cx.update(|app| {
            spawn_hotkey_pump(app, hotkey_rx, &ctx, ());
            spawn_outcome_pump(app, ps_rx, &ctx);
        });
        PumpHarness {
            ctx,
            hotkey_tx,
            ps_tx,
            _config: config,
            effects,
        }
    }

    fn advance(cx: &TestAppContext) {
        cx.executor().advance_clock(Duration::from_millis(300));
        cx.executor().run_until_parked();
    }

    #[gpui::test]
    fn hotkey_toggles_launcher_window(cx: &TestAppContext) {
        let h = harness(cx);

        h.hotkey_tx.send_blocking(()).unwrap();
        advance(cx);
        assert!(h.ctx.active_window.borrow().is_some(), "first press opens");

        h.hotkey_tx.send_blocking(()).unwrap();
        advance(cx);
        assert!(
            h.ctx.active_window.borrow().is_none(),
            "second press closes"
        );
        assert_eq!(h.effects.trims(), 1);
    }

    #[gpui::test]
    fn error_pump_opens_a_window_when_none_exists(cx: &TestAppContext) {
        let h = harness(cx);

        h.ps_tx
            .send_blocking(PsOutcome::Error("brightness: failed".into()))
            .unwrap();
        advance(cx);

        let handle = h
            .ctx
            .active_window
            .borrow()
            .as_ref()
            .copied()
            .expect("pump opens a window");
        let message = cx.update(|app| ui::error_message(&handle, app));
        assert_eq!(message.as_deref(), Some("brightness: failed"));
    }

    #[gpui::test]
    fn error_pump_reuses_an_open_window(cx: &TestAppContext) {
        let h = harness(cx);
        h.hotkey_tx.send_blocking(()).unwrap();
        advance(cx);
        let before = h.ctx.active_window.borrow().as_ref().copied().unwrap();

        h.ps_tx
            .send_blocking(PsOutcome::Error("volume: failed".into()))
            .unwrap();
        advance(cx);

        let after = h.ctx.active_window.borrow().as_ref().copied().unwrap();
        assert_eq!(before, after, "the same window handle is reused");
        let message = cx.update(|app| ui::error_message(&after, app));
        assert_eq!(message.as_deref(), Some("volume: failed"));
    }

    #[gpui::test]
    fn outcome_pump_displays_stdout_in_open_window(cx: &TestAppContext) {
        let h = harness(cx);
        h.hotkey_tx.send_blocking(()).unwrap();
        advance(cx);
        let before = h.ctx.active_window.borrow().as_ref().copied().unwrap();

        h.ps_tx
            .send_blocking(PsOutcome::Output {
                name: "uuid".into(),
                text: "123e4567-e89b-12d3-a456-426614174000".into(),
            })
            .unwrap();
        advance(cx);

        let after = h.ctx.active_window.borrow().as_ref().copied().unwrap();
        assert_eq!(before, after, "the same window handle is reused");
        let output = cx.update(|app| ui::output_message(&after, app));
        assert_eq!(
            output,
            Some(("uuid".to_string(), "123e4567-e89b-12d3-a456-426614174000".to_string()))
        );
    }
}
