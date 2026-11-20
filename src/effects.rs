//! Boundary for side effects triggered by the UI.
//!
//! Production delegates to Win32, `std::process`, and worker threads. Tests
//! supply a recorder so launches, PowerShell dispatch, and memory trimming can
//! be asserted without touching the operating system.

use std::{path::Path, sync::Arc};
#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;

use async_channel::Sender;
use gpui::Window;

use crate::dispatch::PsOutcome;
use crate::platform;

/// Everything the launcher does to the world outside its own widget tree.
pub(crate) trait LauncherEffects: Send + Sync {
    /// Applies DWM styling and topmost state to a freshly opened window.
    fn configure_window(&self, window: &Window);

    /// Starts the configured application with its static and dynamic arguments.
    fn spawn_app(&self, path: &Path, args: &[String]);

    /// Starts a PowerShell function on a worker thread.
    fn spawn_powershell(
        &self,
        name: &str,
        body: &str,
        args: &[String],
        outcome_tx: Sender<PsOutcome>,
    );

    /// Asks Windows to evict idle pages from this process.
    fn trim_working_set(&self);

    /// Starts the interactive updater in a visible PowerShell terminal.
    fn spawn_updater(&self);
}

/// Shared handle used by the UI and the pumps.
pub(crate) type Effects = Arc<dyn LauncherEffects>;

/// The real implementation, used by the binary.
pub(crate) struct SystemEffects;

impl LauncherEffects for SystemEffects {
    fn configure_window(&self, window: &Window) {
        platform::setup_window(window);
        let _ = platform::make_always_on_top(window);
    }

    fn spawn_app(&self, path: &Path, args: &[String]) {
        let mut cmd = std::process::Command::new(path);
        cmd.args(args);
        if let Err(err) = cmd.spawn() {
            eprintln!("Failed to spawn {}: {}", path.display(), err);
        }
    }

    fn spawn_powershell(
        &self,
        name: &str,
        body: &str,
        args: &[String],
        outcome_tx: Sender<PsOutcome>,
    ) {
        let name = name.to_string();
        let body = body.to_string();
        let args = args.to_vec();
        let worker_tx = outcome_tx.clone();
        let worker_name = name.clone();
        if let Err(err) = std::thread::Builder::new()
            .name("shyena-ps-task".into())
            .spawn(move || crate::dispatch::run_ps_task(&worker_name, &body, &args, &worker_tx))
        {
            let _ = outcome_tx.send_blocking(PsOutcome::Error(format!(
                "{name}: failed to start task: {err}"
            )));
        }
    }

    fn trim_working_set(&self) {
        platform::trim_working_set();
    }

    fn spawn_updater(&self) {
        let current_ver = env!("CARGO_PKG_VERSION");
        let script = format!(
            "$CurrentVersion = '{current_ver}'; \
             irm https://raw.githubusercontent.com/adityamotale/shyena/master/update.ps1 | iex"
        );
        let mut cmd = std::process::Command::new("powershell.exe");
        cmd.args([
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            &script,
        ]);
        #[cfg(target_os = "windows")]
        cmd.creation_flags(0x0000_0010); // CREATE_NEW_CONSOLE — spawn a visible terminal
        if let Err(err) = cmd.spawn() {
            eprintln!("Failed to spawn updater: {err}");
        }
    }
}
