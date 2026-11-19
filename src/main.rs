// Release builds are GUI-only (no console flash). Debug builds keep the
// console subsystem so `cargo run` can be interrupted with Ctrl+C: a GUI
// subsystem process is not attached to the terminal's console and never
// receives its Ctrl+C event.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

//! Shyena: a global-hotkey launcher built on GPUI.
//!
//! The process starts at login and stays alive behind a hidden anchor window.
//! [`bootstrap`] wires the hotkey and error channels, [`ui`] owns the launcher
//! window and event handling, and [`dispatch`] resolves queries into commands.
//! See `docs/ARCHITECTURE.md` for the end-to-end event flow.

mod bootstrap;
mod commands;
mod config;
mod dispatch;
mod effects;
mod exec;
mod platform;
mod theme;
mod ui;

#[cfg(test)]
mod test_support;

use async_channel::{Receiver, Sender};
use bootstrap::LauncherContext;
use config::ConfigStore;
use dispatch::PsError;
use effects::SystemEffects;
use gpui::{App, Application};
use std::{borrow::Cow, cell::RefCell, rc::Rc, sync::Arc, time::Duration};

/// One-line cap for every error rendered in the 62 px panel.
pub(crate) const ERROR_LINE_MAX_CHARS: usize = 120;

fn main() {
    let app = Application::new();

    app.run(|cx: &mut App| {
        cx.text_system()
            .add_fonts(vec![Cow::Borrowed(include_bytes!(
                "../assets/fonts/GeistMonoNerdFontMono-Regular.otf"
            ))])
            .expect("Failed to register GeistMono Nerd Font");

        // The launcher is long-running (started at login), so the config is
        // read once here and every launcher open is served from memory.
        let config_store = Rc::new(RefCell::new(ConfigStore::load(
            config::Config::config_path(),
        )));

        // GPUI quits when its last window closes. Keep an invisible 1x1 window
        // open so the hotkey listener and async error delivery survive dismissal.
        let _anchor = ui::open_anchor_window(cx);

        let (hotkey_tx, hotkey_rx): (Sender<()>, Receiver<()>) = async_channel::unbounded();
        let (ps_err_tx, ps_err_rx): (Sender<PsError>, Receiver<PsError>) =
            async_channel::unbounded();

        let context = LauncherContext {
            active_window: Rc::new(RefCell::new(None)),
            config_store,
            ps_error_tx: ps_err_tx,
            effects: Arc::new(SystemEffects),
        };

        // Reclaim startup pages once fonts, renderers and the anchor have settled.
        ui::schedule_trim(
            cx,
            &context.active_window,
            &context.effects,
            Duration::from_secs(2),
        );

        bootstrap::spawn_error_pump(cx, ps_err_rx, &context);
        let manager = bootstrap::setup_global_hotkey(hotkey_tx);
        bootstrap::spawn_hotkey_pump(cx, hotkey_rx, &context, manager);
    });
}
