//! Configuration loading, parsing, caching and reload semantics.
//!
//! The binary reads `.shyena` once at startup and serves every launcher open
//! from memory; only `:reload` and recovery from a bad load touch the disk.

pub mod io;
pub mod parser;
pub mod store;
pub mod suggestions;

pub use io::ConfigReader;
pub use parser::{ConfigAst, Scanner};
pub use store::{ConfigStore, ReloadOutcome};
pub use suggestions::Suggestions;

use crate::ERROR_LINE_MAX_CHARS;
use crate::theme::Theme;
use rustc_hash::FxHashMap;
use std::path::PathBuf;

/// Keeps a parse diagnostic on one line while preserving its location prefix.
/// Path-bearing store diagnostics are not truncated so the path stays exact.
pub(crate) fn fit_diagnostic(message: String) -> String {
    if message.chars().count() <= ERROR_LINE_MAX_CHARS {
        message
    } else {
        let mut truncated: String = message
            .chars()
            .take(ERROR_LINE_MAX_CHARS.saturating_sub(3))
            .collect();
        truncated.push_str("...");
        truncated
    }
}

/// Defaults are defined by the setting types themselves: `Theme::Macchiato` and
/// `Suggestions::On`. `Default` is also the fallback config used while a fatal
/// config error is displayed.
#[derive(Default)]
#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
pub struct Config {
    pub apps: FxHashMap<String, AppAction>,
    pub ps: FxHashMap<String, PsAction>,
    pub theme: Theme,
    pub suggestions: Suggestions,
}

/// A registered application and the arguments prepended to user-supplied ones.
#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
pub struct AppAction {
    pub path: PathBuf,
    pub args: Vec<String>,
}

/// A PowerShell function, kept both as a display script and an invocable body.
#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
pub struct PsAction {
    pub script: String,
    pub body: String,
}

impl Config {
    /// Returns the location of `.shyena` for the current build.
    ///
    /// Debug builds read the canonical repo-root file so development never
    /// touches the user's installed configuration. Release builds read the
    /// installed copy: `%USERPROFILE%` wins on Windows, `$HOME` is the fallback
    /// for other environments, and a working-directory-relative path is the
    /// last resort. The binary only reads the file — installation seeds it once.
    pub fn config_path() -> PathBuf {
        #[cfg(debug_assertions)]
        {
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(".shyena")
        }
        #[cfg(not(debug_assertions))]
        {
            std::env::var_os("USERPROFILE")
                .or_else(|| std::env::var_os("HOME"))
                .map_or_else(
                    || PathBuf::from(".config/.shyena"),
                    |home| PathBuf::from(home).join(".config").join(".shyena"),
                )
        }
    }

    pub fn from_ast(ast: &ConfigAst<'_>) -> Self {
        let mut apps = FxHashMap::default();
        let mut ps = FxHashMap::default();

        for app in &ast.apps {
            let name = app.unquoted_name().to_string();
            apps.insert(
                name,
                AppAction {
                    path: app.path.clone(),
                    args: app.args.clone(),
                },
            );
        }

        for func in &ast.functions {
            let name = func.name.to_string();
            let script = format!("function {} {}\n{} @args", name, func.block_span, name);
            ps.insert(
                name,
                PsAction {
                    script,
                    body: func.block_span.to_string(),
                },
            );
        }

        // Omitted directives are valid: fall back to the canonical defaults
        // rather than reporting an error.
        let defaults = Self::default();
        Config {
            apps,
            ps,
            theme: ast.theme.unwrap_or(defaults.theme),
            suggestions: ast.suggestions.unwrap_or(defaults.suggestions),
        }
    }

    /// Canonical config, embedded from the repo-root `.shyena` so the repository
    /// file is the single source of truth. It is also the file debug builds read
    /// at runtime and the template `install.ps1` copies to `~/.config/.shyena`.
    #[cfg(test)]
    const DEFAULT_SHYENA: &str = include_str!("../../.shyena");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(debug_assertions)]
    #[test]
    fn test_debug_config_path_points_at_repo_root_config() {
        let path = Config::config_path();
        assert_eq!(
            path,
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(".shyena")
        );
        assert!(path.exists(), "repo-root .shyena must exist: {path:?}");
        assert_eq!(
            std::fs::read_to_string(&path).expect("read repo-root .shyena"),
            Config::DEFAULT_SHYENA
        );
    }

    #[cfg(debug_assertions)]
    #[test]
    fn test_debug_store_loads_repo_root_config() {
        use crate::config::ConfigStore;

        let store = ConfigStore::load(Config::config_path());
        assert!(!store.has_error(), "{:?}", store.diagnostic());
        assert!(!store.current().ps.is_empty(), "dev config has functions");
    }

    #[cfg(not(debug_assertions))]
    #[test]
    fn test_release_config_path_points_at_installed_config() {
        let path = Config::config_path();
        assert!(path.ends_with(".config/.shyena"), "got: {path:?}");
    }

    #[test]
    fn test_parse_shyena_ast_and_function_invocation() {
        let source = r#"
Register-App 'wt' @{
    Path = 'C:\Windows\wt.exe'
    Args = @('--maximized', '-d', 'C:\dev')
}

Register-App 'simple' @{
    Path = "code"
    Args = '--reuse'
}

function brightness {
    param([int]$Level)
    Write-Output $Level
}
"#;
        let ast = Scanner::new(source).parse().expect("valid shyena syntax");
        let cfg = Config::from_ast(&ast);

        assert_eq!(cfg.apps.len(), 2);
        assert_eq!(cfg.ps.len(), 1);

        let wt = cfg.apps.get("wt").expect("wt exists");
        assert_eq!(wt.path, PathBuf::from(r"C:\Windows\wt.exe"));
        assert_eq!(wt.args, vec!["--maximized", "-d", r"C:\dev"]);

        let simple = cfg.apps.get("simple").expect("simple exists");
        assert_eq!(simple.path, PathBuf::from("code"));
        assert_eq!(simple.args, vec!["--reuse"]);

        let brightness = cfg.ps.get("brightness").expect("brightness exists");
        assert!(brightness.script.contains("function brightness"));
        assert!(brightness.script.ends_with("brightness @args"));
    }

    #[test]
    fn test_default_config_parses_with_portable_app_registrations() {
        let ast = Scanner::new(Config::DEFAULT_SHYENA)
            .parse()
            .expect("default config parses");
        let cfg = Config::from_ast(&ast);

        assert_eq!(cfg.theme, Theme::Macchiato);
        assert_eq!(cfg.suggestions, Suggestions::On);
        assert_eq!(cfg.apps.len(), 4);

        let terminal = cfg.apps.get("terminal").expect("terminal exists");
        assert_eq!(terminal.path, PathBuf::from("wt.exe"));
        assert_eq!(terminal.args, Vec::<String>::new());

        let files = cfg.apps.get("files").expect("files exists");
        assert_eq!(files.path, PathBuf::from(r"C:\Windows\explorer.exe"));
        assert_eq!(files.args, Vec::<String>::new());

        let calc = cfg.apps.get("calc").expect("calc exists");
        assert_eq!(calc.path, PathBuf::from(r"C:\Windows\System32\calc.exe"));
        assert_eq!(calc.args, Vec::<String>::new());

        let taskmgr = cfg.apps.get("taskmgr").expect("taskmgr exists");
        assert_eq!(
            taskmgr.path,
            PathBuf::from(r"C:\Windows\System32\Taskmgr.exe")
        );
        assert_eq!(taskmgr.args, Vec::<String>::new());
    }

    #[test]
    fn test_default_config_declares_all_utility_functions() {
        let ast = Scanner::new(Config::DEFAULT_SHYENA)
            .parse()
            .expect("default config parses");
        let cfg = Config::from_ast(&ast);

        let expected = [
            "lock",
            "standby",
            "shutdown",
            "restart",
            "volume",
            "mute",
            "open",
            "calc",
            "brightness",
        ];
        assert_eq!(cfg.ps.len(), expected.len());
        for name in expected {
            assert!(cfg.ps.contains_key(name), "missing function: {name}");
        }
    }

    #[test]
    fn test_default_volume_requires_explicit_level() {
        let ast = Scanner::new(Config::DEFAULT_SHYENA)
            .parse()
            .expect("default config parses");
        let cfg = Config::from_ast(&ast);

        let action = cfg.ps.get("volume").expect("function exists");
        assert!(
            action.body.contains("Parameter(Mandatory = $true)"),
            "volume has a mandatory parameter"
        );
        assert!(
            action.body.contains("ValidateRange(0, 100)"),
            "volume validates the range"
        );
        assert!(!action.body.contains("$args["), "volume uses a param block");
    }

    #[test]
    fn test_default_volume_uses_core_audio_com() {
        let ast = Scanner::new(Config::DEFAULT_SHYENA)
            .parse()
            .expect("default config parses");
        let cfg = Config::from_ast(&ast);
        let volume = cfg.ps.get("volume").expect("function exists");

        assert!(
            volume.body.contains("SetMasterVolumeLevelScalar"),
            "volume must set the scalar master level through Core Audio"
        );
        assert!(
            volume.body.contains("[ShyenaAudio]::SetVolume($Level)"),
            "volume must invoke the compiled COM helper"
        );
    }

    #[test]
    fn test_theme_from_ast() {
        let latte = "Set-Theme 'latte'\nRegister-App 'wt' @{ Path = 'wt.exe' }";
        let ast = Scanner::new(latte).parse().unwrap();
        let cfg = Config::from_ast(&ast);
        assert_eq!(cfg.theme, Theme::Latte);

        let missing = "Register-App 'wt' @{ Path = 'wt.exe' }";
        let ast = Scanner::new(missing).parse().unwrap();
        assert_eq!(Config::from_ast(&ast).theme, Theme::Macchiato);
    }

    #[test]
    fn test_suggestions_from_ast() {
        let off = "Set-Suggestions 'off'\nRegister-App 'wt' @{ Path = 'wt.exe' }";
        let ast = Scanner::new(off).parse().unwrap();
        let cfg = Config::from_ast(&ast);
        assert_eq!(cfg.suggestions, Suggestions::Off);
        assert!(!cfg.suggestions.enabled());

        let on = "set-suggestions \"ON\"\nfunction f { }";
        let ast = Scanner::new(on).parse().unwrap();
        assert_eq!(Config::from_ast(&ast).suggestions, Suggestions::On);
    }

    #[test]
    fn test_missing_directives_use_defaults() {
        let defaults = Config::default();
        assert_eq!(defaults.theme, Theme::Macchiato);
        assert_eq!(defaults.suggestions, Suggestions::On);

        // An older config with neither directive is still valid.
        let ast = Scanner::new("Register-App 'wt' @{ Path = 'wt.exe' }")
            .parse()
            .unwrap();
        let cfg = Config::from_ast(&ast);
        assert_eq!(cfg.theme, defaults.theme);
        assert_eq!(cfg.suggestions, defaults.suggestions);
    }

    #[test]
    fn test_invalid_suggestions_value_is_rejected() {
        let err = Scanner::new("Set-Suggestions 'maybe'").parse().unwrap_err();
        assert_eq!(
            err.message,
            "invalid suggestions value: expected 'on' or 'off'"
        );
    }
}
