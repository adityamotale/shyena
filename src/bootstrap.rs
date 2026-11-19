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
use crate::dispatch::PsError;
use crate::ui::{self, ActiveWindow};

/// Handles shared by both pumps and the composition root.
pub(crate) struct LauncherContext {
    pub(crate) active_window: ActiveWindow,
    pub(crate) config_store: Rc<RefCell<ConfigStore>>,
    pub(crate) ps_error_tx: Sender<PsError>,
}

/// Registers Alt+Backquote and forwards presses to the UI channel from a
/// dedicated thread. The manager must be kept alive for the process lifetime.
pub(crate) fn setup_global_hotkey(tx: Sender<()>) -> GlobalHotKeyManager {
    let manager = GlobalHotKeyManager::new().expect("Failed to initialize GlobalHotKeyManager");
    let hotkey = HotKey::new(Some(Modifiers::ALT), Code::Backquote);
    let hotkey_id = hotkey.id();
    manager
        .register(hotkey)
        .expect("Failed to register Alt+Backquote global hotkey");

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

/// Surfaces asynchronous PowerShell failures in the launcher itself. If the
/// bar was already dismissed, open a fresh window in the error state.
pub(crate) fn spawn_error_pump(cx: &mut App, rx: Receiver<PsError>, ctx: &LauncherContext) {
    let active = ctx.active_window.clone();
    let ps_error_tx = ctx.ps_error_tx.clone();
    let store = ctx.config_store.clone();

    cx.spawn(async move |cx| {
        while let Ok(err) = rx.recv().await {
            let active = active.clone();
            let ps_error_tx = ps_error_tx.clone();
            let store = store.clone();
            let message = err.message;
            let _ = cx.update(move |cx| {
                let handle = active.borrow().as_ref().copied();
                let handle = if let Some(handle) = handle {
                    handle
                } else {
                    let handle = ui::open_search_window(cx, &active, ps_error_tx.clone(), &store);
                    *active.borrow_mut() = Some(handle);
                    handle
                };

                if handle
                    .update(cx, |bar, window, cx| {
                        bar.show_error(message.clone(), window, cx);
                    })
                    .is_err()
                {
                    let handle = ui::open_search_window(cx, &active, ps_error_tx, &store);
                    *active.borrow_mut() = Some(handle);
                    let _ = handle.update(cx, |bar, window, cx| {
                        bar.show_error(message, window, cx);
                    });
                }
            });
        }
    })
    .detach();
}

/// Turns hotkey events into launcher toggles on the UI thread.
pub(crate) fn spawn_hotkey_pump(
    cx: &mut App,
    rx: Receiver<()>,
    ctx: &LauncherContext,
    hotkey_manager: GlobalHotKeyManager,
) {
    let active = ctx.active_window.clone();
    let ps_error_tx = ctx.ps_error_tx.clone();
    let store = ctx.config_store.clone();

    cx.spawn(async move |cx| {
        // Keep the manager in the async task scope so it is never dropped.
        let _manager = hotkey_manager;
        while let Ok(()) = rx.recv().await {
            let active = active.clone();
            let ps_error_tx = ps_error_tx.clone();
            let store = store.clone();
            let _ = cx.update(move |cx| {
                ui::toggle_search_window(cx, &active, &ps_error_tx, &store);
            });
        }
    })
    .detach();
}
