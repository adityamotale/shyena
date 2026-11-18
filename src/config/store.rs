use crate::config::{Config, io::ConfigReader, parser::Scanner};
use std::path::PathBuf;
use std::sync::Arc;

/// Result of re-reading the configuration from disk.
#[derive(Debug)]
pub enum ReloadOutcome {
    Updated(Arc<Config>),
    Error(String),
}

/// Process-wide configuration cache.
///
/// The file is read exactly once when the application starts, so opening the
/// launcher performs no filesystem I/O. `:reload` is the only path that
/// re-reads the file, keeping the always-on background process free of watcher
/// threads and wakeups. A failed load is retried when the launcher opens so a
/// broken config can be repaired without restarting Shyena.
pub struct ConfigStore {
    path: PathBuf,
    reader: ConfigReader,
    current: Arc<Config>,
    diagnostic: Option<String>,
}

impl ConfigStore {
    /// Loads the configuration once. A missing file is seeded with the default
    /// config; any I/O or parse failure is retained as a diagnostic while a
    /// fallback (empty) config keeps the UI renderable.
    pub fn load(path: PathBuf) -> Self {
        let reader = ConfigReader::new(&path);
        let mut store = Self {
            path,
            reader,
            current: Arc::new(Config::default()),
            diagnostic: None,
        };

        if store.path.exists() {
            match store.read_from_disk() {
                Ok(config) => store.current = Arc::new(config),
                Err(diagnostic) => store.diagnostic = Some(diagnostic),
            }
        } else {
            store.current = Arc::new(Config::create_default_config(&store.path));
        }

        store
    }

    pub fn current(&self) -> Arc<Config> {
        Arc::clone(&self.current)
    }

    pub fn diagnostic(&self) -> Option<&str> {
        self.diagnostic.as_deref()
    }

    /// True when the last load or reload failed; callers retry on launcher open.
    pub fn has_error(&self) -> bool {
        self.diagnostic.is_some()
    }

    /// Re-reads the configuration from disk. On failure the last known-good
    /// config stays active and the diagnostic is retained for the error panel.
    pub fn reload(&mut self) -> ReloadOutcome {
        match self.read_from_disk() {
            Ok(config) => {
                let config = Arc::new(config);
                self.current = Arc::clone(&config);
                self.diagnostic = None;
                ReloadOutcome::Updated(config)
            }
            Err(diagnostic) => {
                self.diagnostic = Some(diagnostic.clone());
                ReloadOutcome::Error(diagnostic)
            }
        }
    }

    fn read_from_disk(&mut self) -> Result<Config, String> {
        let raw = self
            .reader
            .read()
            .map_err(|err| format!("~/.config/.shyena:1:1: error: failed to read config: {err}"))?;
        let ast = Scanner::new(raw)
            .parse()
            .map_err(|err| err.to_diagnostic())?;
        Ok(Config::from_ast(&ast))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_config_path(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("shyena_store_{}_{}", name, std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create temp config dir");
        dir.join(".shyena")
    }

    #[test]
    fn test_load_seeds_default_config_when_missing() {
        let path = temp_config_path("seed");
        let store = ConfigStore::load(path.clone());

        assert!(path.exists(), "default config is written on first load");
        assert!(store.diagnostic().is_none());
        assert!(store.current().apps.contains_key("helium"));

        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn test_load_reports_parse_error() {
        let path = temp_config_path("parse_error");
        std::fs::write(&path, "Set-Theme 'mocha'\n").unwrap();

        let store = ConfigStore::load(path.clone());

        let diagnostic = store.diagnostic().expect("diagnostic recorded");
        assert!(diagnostic.contains("invalid theme"), "got: {diagnostic}");
        assert!(store.current().apps.is_empty());

        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn test_reload_swaps_in_new_config() {
        let path = temp_config_path("reload");
        std::fs::write(&path, "Register-App 'editor' @{ Path = 'code.exe' }\n").unwrap();
        let mut store = ConfigStore::load(path.clone());
        assert!(store.current().apps.contains_key("editor"));

        std::fs::write(
            &path,
            "Set-Theme 'light'\nRegister-App 'terminal' @{ Path = 'wt.exe' }\n",
        )
        .unwrap();

        match store.reload() {
            ReloadOutcome::Updated(config) => {
                assert!(config.apps.contains_key("terminal"));
                assert!(!config.apps.contains_key("editor"));
                assert_eq!(config.theme, crate::theme::Theme::Light);
            }
            ReloadOutcome::Error(message) => panic!("expected success, got: {message}"),
        }
        assert!(store.diagnostic().is_none());

        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn test_reload_error_keeps_last_good_config() {
        let path = temp_config_path("keep_last_good");
        std::fs::write(&path, "Register-App 'editor' @{ Path = 'code.exe' }\n").unwrap();
        let mut store = ConfigStore::load(path.clone());

        std::fs::write(&path, "function bad {\n").unwrap();
        match store.reload() {
            ReloadOutcome::Error(message) => assert!(!message.is_empty()),
            ReloadOutcome::Updated(_) => panic!("expected parse failure"),
        }

        assert!(store.has_error());
        assert!(
            store.current().apps.contains_key("editor"),
            "last known-good config stays active"
        );

        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn test_reload_recovers_after_error() {
        let path = temp_config_path("recovery");
        std::fs::write(&path, "Set-Theme 'neon'\n").unwrap();
        let mut store = ConfigStore::load(path.clone());
        assert!(store.has_error());

        std::fs::write(
            &path,
            "Set-Theme 'light'\nRegister-App 'term' @{ Path = 'wt.exe' }\n",
        )
        .unwrap();

        match store.reload() {
            ReloadOutcome::Updated(config) => {
                assert_eq!(config.theme, crate::theme::Theme::Light);
                assert!(config.apps.contains_key("term"));
            }
            ReloadOutcome::Error(message) => panic!("expected recovery, got: {message}"),
        }
        assert!(store.diagnostic().is_none());

        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn test_reload_reports_missing_file() {
        let path = temp_config_path("missing");
        std::fs::write(&path, "Register-App 'editor' @{ Path = 'code.exe' }\n").unwrap();
        let mut store = ConfigStore::load(path.clone());
        std::fs::remove_file(&path).unwrap();

        match store.reload() {
            ReloadOutcome::Error(message) => {
                assert!(message.contains("failed to read config"), "got: {message}");
            }
            ReloadOutcome::Updated(_) => panic!("expected I/O failure"),
        }

        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }
}
