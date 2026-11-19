//! Test doubles shared by the UI and pump integration tests.

use std::{
    cell::RefCell,
    path::{Path, PathBuf},
    rc::Rc,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
};

use async_channel::Sender;
use gpui::Window;

use crate::config::ConfigStore;
use crate::dispatch::PsError;
use crate::effects::LauncherEffects;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct AppLaunch {
    pub(crate) path: PathBuf,
    pub(crate) args: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PsLaunch {
    pub(crate) name: String,
    pub(crate) body: String,
    pub(crate) args: Vec<String>,
}

#[derive(Default)]
struct Log {
    apps: Vec<AppLaunch>,
    powershell: Vec<PsLaunch>,
    trims: usize,
    window_configures: usize,
    updaters: usize,
}

/// Captures every side effect instead of performing it.
pub(crate) struct RecordingEffects {
    log: Mutex<Log>,
}

impl RecordingEffects {
    pub(crate) fn new() -> Arc<Self> {
        Arc::new(Self {
            log: Mutex::new(Log::default()),
        })
    }

    pub(crate) fn app_launches(&self) -> Vec<AppLaunch> {
        self.log.lock().unwrap().apps.clone()
    }

    pub(crate) fn ps_launches(&self) -> Vec<PsLaunch> {
        self.log.lock().unwrap().powershell.clone()
    }

    pub(crate) fn trims(&self) -> usize {
        self.log.lock().unwrap().trims
    }

    pub(crate) fn window_configures(&self) -> usize {
        self.log.lock().unwrap().window_configures
    }

    pub(crate) fn updaters(&self) -> usize {
        self.log.lock().unwrap().updaters
    }
}

impl LauncherEffects for RecordingEffects {
    fn configure_window(&self, _window: &Window) {
        self.log.lock().unwrap().window_configures += 1;
    }

    fn spawn_app(&self, path: &Path, args: &[String]) {
        self.log.lock().unwrap().apps.push(AppLaunch {
            path: path.to_path_buf(),
            args: args.to_vec(),
        });
    }

    fn spawn_powershell(
        &self,
        name: &str,
        body: &str,
        args: &[String],
        _error_tx: Sender<PsError>,
    ) {
        self.log.lock().unwrap().powershell.push(PsLaunch {
            name: name.to_string(),
            body: body.to_string(),
            args: args.to_vec(),
        });
    }

    fn trim_working_set(&self) {
        self.log.lock().unwrap().trims += 1;
    }

    fn spawn_updater(&self) {
        self.log.lock().unwrap().updaters += 1;
    }
}

/// A config file in a private temp directory, removed on drop.
pub(crate) struct TestConfig {
    dir: PathBuf,
    pub(crate) path: PathBuf,
    pub(crate) store: Rc<RefCell<ConfigStore>>,
}

impl TestConfig {
    pub(crate) fn new(name: &str, source: &str) -> Self {
        // Tests run in parallel; each fixture needs its own directory.
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let seq = NEXT.fetch_add(1, Ordering::Relaxed);
        let dir =
            std::env::temp_dir().join(format!("shyena_ui_{}_{}_{seq}", name, std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create temp config dir");
        let path = dir.join(".shyena");
        std::fs::write(&path, source).expect("write temp config");
        let store = Rc::new(RefCell::new(ConfigStore::load(path.clone())));
        Self { dir, path, store }
    }

    /// Rewrites the file so `:reload` has something new to pick up.
    pub(crate) fn rewrite(&self, source: &str) {
        std::fs::write(&self.path, source).expect("rewrite temp config");
    }
}

impl Drop for TestConfig {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// Config used by UI tests: two apps and two functions with deterministic bodies.
pub(crate) const UI_CONFIG: &str = r"Register-App 'notepad' @{
    Path = 'C:\Windows\System32\notepad.exe'
    Args = @('--flag')
}
Register-App 'calc' @{ Path = 'calc.exe' }
function brightness {
    param([int]$Level)
    Write-Output $Level
}
function volume {
    param([int]$Level)
    Write-Output $Level
}
";
