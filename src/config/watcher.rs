use crate::config::io::ConfigReader;
use crate::config::parser::{ParseError, Scanner};
use crate::theme::Theme;
use std::io;
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::path::PathBuf;
use std::sync::mpsc::{self, RecvTimeoutError};
use std::thread;
use std::time::Duration;
use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE};
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, ReadDirectoryChangesW, FILE_FLAG_BACKUP_SEMANTICS, FILE_LIST_DIRECTORY,
    FILE_NOTIFY_CHANGE_FILE_NAME, FILE_NOTIFY_CHANGE_LAST_WRITE, FILE_NOTIFY_CHANGE_SIZE,
    FILE_NOTIFY_INFORMATION, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
};

pub const RESERVED_RELOAD_CMD: &str = "config-reload";
const DEBOUNCE_WINDOW: Duration = Duration::from_millis(0x4B);
const WATCH_FILTERS: u32 =
    FILE_NOTIFY_CHANGE_FILE_NAME | FILE_NOTIFY_CHANGE_LAST_WRITE | FILE_NOTIFY_CHANGE_SIZE;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnedAppDef {
    pub name: String,
    pub path: PathBuf,
    pub args: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnedFunctionDef {
    pub name: String,
    pub block_span: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ConfigSnapshot {
    pub apps: Vec<OwnedAppDef>,
    pub functions: Vec<OwnedFunctionDef>,
    pub theme: Theme,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReloadError {
    Io(String),
    Parse(ParseError),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReloadEvent {
    Updated(ConfigSnapshot),
    Error(ReloadError),
}

#[allow(dead_code)]
enum WatchSignal {
    FsChanged,
    ManualReload,
}

/// Thread-safe handle used by `#config-reload` to trigger an immediate reload
#[derive(Clone)]
pub struct ReloadTrigger {
    #[allow(dead_code)]
    tx: mpsc::Sender<WatchSignal>,
}

impl ReloadTrigger {
    #[inline]
    #[allow(dead_code)]
    pub fn trigger_now(&self) {
        let _ = self.tx.send(WatchSignal::ManualReload);
    }
}

struct DirHandle(HANDLE);

impl Drop for DirHandle {
    #[inline]
    fn drop(&mut self) {
        if self.0 != INVALID_HANDLE_VALUE && !self.0.is_null() {
            unsafe {
                CloseHandle(self.0);
            }
        }
    }
}

fn execute_reload(reader: &mut ConfigReader) -> ReloadEvent {
    let raw = match reader.read() {
        Ok(s) => s,
        Err(e) => return ReloadEvent::Error(ReloadError::Io(e.to_string())),
    };

    match Scanner::new(raw).parse() {
        Ok(ast) => {
            let apps = ast
                .apps
                .into_iter()
                .map(|a| OwnedAppDef {
                    name: a.unquoted_name().to_string(),
                    path: a.path,
                    args: a.args,
                })
                .collect();

            let functions = ast
                .functions
                .into_iter()
                .map(|f| OwnedFunctionDef {
                    name: f.name.to_string(),
                    block_span: f.block_span.to_string(),
                })
                .collect();

            ReloadEvent::Updated(ConfigSnapshot {
                apps,
                functions,
                theme: ast.theme.unwrap_or_default(),
            })
        }
        Err(err) => ReloadEvent::Error(ReloadError::Parse(err)),
    }
}

/// Spawns the Win32 `ReadDirectoryChangesW` watcher and trailing-edge debouncer;
/// returns a `ReloadTrigger` (for `#config-reload`) and an `async_channel::Receiver<ReloadEvent>`
/// for GPUI UI consumption
pub fn spawn_config_watcher(
    config_path: PathBuf,
) -> io::Result<(ReloadTrigger, async_channel::Receiver<ReloadEvent>)> {
    let parent_dir = config_path
        .parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "config path has no parent"))?
        .to_path_buf();

    let target_name = config_path
        .file_name()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "config path has no filename"))?
        .to_os_string();

    let target_str = target_name.to_string_lossy().into_owned();

    let (sig_tx, sig_rx) = mpsc::channel::<WatchSignal>();
    let (event_tx, event_rx) = async_channel::unbounded::<ReloadEvent>();

    // Thread 1: Debouncer + Reload Executor (owns ConfigReader)
    let reload_path = config_path;
    thread::Builder::new()
        .name("shyena-config-reloader".into())
        .spawn(move || {
            let mut reader = ConfigReader::new(&reload_path);
            let mut pending_debounce = false;

            loop {
                let msg = if pending_debounce {
                    sig_rx.recv_timeout(DEBOUNCE_WINDOW)
                } else {
                    sig_rx.recv().map_err(|_| RecvTimeoutError::Disconnected)
                };

                match msg {
                    Ok(WatchSignal::FsChanged) => {
                        pending_debounce = true;
                    }
                    Ok(WatchSignal::ManualReload) => {
                        pending_debounce = false;
                        let event = execute_reload(&mut reader);
                        if event_tx.send_blocking(event).is_err() {
                            break;
                        }
                    }
                    Err(RecvTimeoutError::Timeout) => {
                        pending_debounce = false;
                        let event = execute_reload(&mut reader);
                        if event_tx.send_blocking(event).is_err() {
                            break;
                        }
                    }
                    Err(RecvTimeoutError::Disconnected) => break,
                }
            }
        })?;

    // Thread 2: Win32 ReadDirectoryChangesW Listener
    let fs_tx = sig_tx.clone();
    thread::Builder::new()
        .name("shyena-win32-watcher".into())
        .spawn(move || {
            let wide_dir: Vec<u16> = parent_dir
                .as_os_str()
                .encode_wide()
                .chain(std::iter::once(0))
                .collect();

            let raw_handle = unsafe {
                CreateFileW(
                    wide_dir.as_ptr(),
                    FILE_LIST_DIRECTORY,
                    FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                    std::ptr::null(),
                    OPEN_EXISTING,
                    FILE_FLAG_BACKUP_SEMANTICS,
                    std::ptr::null_mut(),
                )
            };

            if raw_handle == INVALID_HANDLE_VALUE || raw_handle.is_null() {
                return;
            }
            let handle = DirHandle(raw_handle);

            // 4-byte aligned buffer required by FILE_NOTIFY_INFORMATION (0x0400 u32 = 4 KB)
            let mut buffer = [0u32; 0x0400];
            let byte_len = std::mem::size_of_val(&buffer) as u32;

            loop {
                let mut bytes_returned: u32 = 0;
                let ok = unsafe {
                    ReadDirectoryChangesW(
                        handle.0,
                        buffer.as_mut_ptr() as *mut _,
                        byte_len,
                        0, // bWatchSubtree = FALSE (only parent config root)
                        WATCH_FILTERS,
                        &mut bytes_returned,
                        std::ptr::null_mut(),
                        None,
                    )
                };

                if ok == 0 || bytes_returned == 0 {
                    break;
                }

                let base_ptr = buffer.as_ptr() as *const u8;
                let mut offset: usize = 0;

                loop {
                    let info = unsafe { &*(base_ptr.add(offset) as *const FILE_NOTIFY_INFORMATION) };
                    let name_len_u16 = (info.FileNameLength as usize) / 2;
                    let name_slice =
                        unsafe { std::slice::from_raw_parts(info.FileName.as_ptr(), name_len_u16) };
                    let changed_name = std::ffi::OsString::from_wide(name_slice);

                    if changed_name
                        .to_str()
                        .is_some_and(|s| s.eq_ignore_ascii_case(&target_str))
                    {
                        if fs_tx.send(WatchSignal::FsChanged).is_err() {
                            return;
                        }
                        break;
                    }

                    if info.NextEntryOffset == 0 {
                        break;
                    }
                    offset += info.NextEntryOffset as usize;
                }
            }
        })?;

    Ok((ReloadTrigger { tx: sig_tx }, event_rx))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_manual_reload_trigger() {
        let test_dir =
            std::env::temp_dir().join(format!("shyena_test_manual_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&test_dir);
        std::fs::create_dir_all(&test_dir).unwrap();
        let config_path = test_dir.join(".shyena");

        let sample_config = "# shyena:v1\nSet-Theme 'light'\nRegister-App \"editor\" @{\n    path = \"code.exe\"\n}\n\nfunction prompt {\n    Write-Host \"hello\"\n}\n";
        std::fs::write(&config_path, sample_config).unwrap();

        let (trigger, event_rx) = spawn_config_watcher(config_path).unwrap();
        trigger.trigger_now();

        let event = event_rx.recv_blocking().expect("failed to receive reload event");
        match event {
            ReloadEvent::Updated(snapshot) => {
                assert_eq!(snapshot.apps.len(), 1);
                assert_eq!(snapshot.apps[0].name, "editor");
                assert_eq!(snapshot.functions.len(), 1);
                assert_eq!(snapshot.functions[0].name, "prompt");
                assert_eq!(snapshot.theme, Theme::Light);
            }
            ReloadEvent::Error(err) => panic!("expected successful reload, got: {:?}", err),
        }

        let _ = std::fs::remove_dir_all(&test_dir);
    }

    #[test]
    fn test_atomic_save_debounce() {
        let test_dir =
            std::env::temp_dir().join(format!("shyena_test_atomic_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&test_dir);
        std::fs::create_dir_all(&test_dir).unwrap();
        let config_path = test_dir.join(".shyena");

        let initial_config = "# shyena:v1\nRegister-App \"initial\" @{\n    path = \"notepad.exe\"\n}\n";
        std::fs::write(&config_path, initial_config).unwrap();

        let (trigger, event_rx) = spawn_config_watcher(config_path.clone()).unwrap();
        trigger.trigger_now();
        let first = event_rx.recv_blocking().expect("initial event");
        match first {
            ReloadEvent::Updated(s) => assert_eq!(s.apps[0].name, "initial"),
            _ => panic!("expected updated event"),
        }

        // Allow Win32 watcher loop to enter ReadDirectoryChangesW
        std::thread::sleep(Duration::from_millis(0x32));

        // Simulate atomic editor save: write temp file then rename into place
        let updated_config = "# shyena:v1\nRegister-App \"terminal\" @{\n    path = \"wt.exe\"\n}\n";
        let temp_path = test_dir.join("temp.tmp");
        std::fs::write(&temp_path, updated_config).unwrap();
        std::fs::rename(&temp_path, &config_path).unwrap();

        // 75 ms debounce coalescing
        let event = event_rx.recv_blocking().expect("debounced reload event");
        match event {
            ReloadEvent::Updated(snapshot) => {
                assert_eq!(snapshot.apps.len(), 1);
                assert_eq!(snapshot.apps[0].name, "terminal");
            }
            ReloadEvent::Error(err) => panic!("atomic save reload failed: {:?}", err),
        }

        let _ = std::fs::remove_dir_all(&test_dir);
    }

    #[test]
    fn test_invalid_theme_reports_parse_error() {
        let test_dir =
            std::env::temp_dir().join(format!("shyena_test_theme_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&test_dir);
        std::fs::create_dir_all(&test_dir).unwrap();
        let config_path = test_dir.join(".shyena");
        std::fs::write(&config_path, "Set-Theme 'mocha'\n").unwrap();

        let (trigger, event_rx) = spawn_config_watcher(config_path).unwrap();
        trigger.trigger_now();

        match event_rx.recv_blocking().expect("reload event") {
            ReloadEvent::Error(ReloadError::Parse(parse_err)) => {
                assert_eq!(
                    parse_err.message,
                    "invalid theme: expected 'dark' or 'light'"
                );
                assert_eq!(parse_err.line, 1);
            }
            other => panic!("expected invalid theme error, got: {other:?}"),
        }

        let _ = std::fs::remove_dir_all(&test_dir);
    }

    #[test]
    fn test_invalid_then_valid_reload_recovery() {
        let test_dir =
            std::env::temp_dir().join(format!("shyena_test_recovery_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&test_dir);
        std::fs::create_dir_all(&test_dir).unwrap();
        let config_path = test_dir.join(".shyena");
        std::fs::write(&config_path, "Set-Theme 'neon'\n").unwrap();

        let (trigger, event_rx) = spawn_config_watcher(config_path.clone()).unwrap();
        trigger.trigger_now();
        match event_rx.recv_blocking().expect("error event") {
            ReloadEvent::Error(ReloadError::Parse(_)) => {}
            other => panic!("expected parse error, got: {other:?}"),
        }

        std::fs::write(
            &config_path,
            "Set-Theme 'light'\nRegister-App 'term' @{ Path = 'wt.exe' }\n",
        )
        .unwrap();
        trigger.trigger_now();
        match event_rx.recv_blocking().expect("updated event") {
            ReloadEvent::Updated(snapshot) => {
                assert_eq!(snapshot.theme, Theme::Light);
                assert_eq!(snapshot.apps[0].name, "term");
            }
            other => panic!("expected successful reload, got: {other:?}"),
        }

        let _ = std::fs::remove_dir_all(&test_dir);
    }

    #[test]
    fn test_syntax_error_resilience() {
        let test_dir =
            std::env::temp_dir().join(format!("shyena_test_syntax_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&test_dir);
        std::fs::create_dir_all(&test_dir).unwrap();
        let config_path = test_dir.join(".shyena");

        let valid_config = "# shyena:v1\nRegister-App \"browser\" @{\n    path = \"brave.exe\"\n}\n";
        std::fs::write(&config_path, valid_config).unwrap();

        let (trigger, event_rx) = spawn_config_watcher(config_path.clone()).unwrap();
        trigger.trigger_now();
        let _ = event_rx.recv_blocking().expect("initial valid read");

        std::thread::sleep(Duration::from_millis(0x32));

        // Overwrite with malformed syntax missing closing brace
        let broken_config = "function bad {";
        std::fs::write(&config_path, broken_config).unwrap();

        let event = event_rx.recv_blocking().expect("error reload event");
        match event {
            ReloadEvent::Error(ReloadError::Parse(parse_err)) => {
                assert!(parse_err.line >= 1);
                assert!(!parse_err.to_diagnostic().is_empty());
            }
            other => panic!("expected ReloadEvent::Error(ParseError), got: {:?}", other),
        }

        let _ = std::fs::remove_dir_all(&test_dir);
    }
}
