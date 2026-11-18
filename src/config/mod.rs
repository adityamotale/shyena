pub mod io;
pub mod parser;
pub mod store;
pub mod suggestions;

#[allow(unused_imports, dead_code)]
pub use io::{resolve_config_path, ConfigReader};
#[allow(unused_imports, dead_code)]
pub use parser::{AppDef, ConfigAst, FunctionDef, ParseError, Scanner};
pub use store::{ConfigStore, ReloadOutcome};
pub use suggestions::Suggestions;

use crate::theme::Theme;
use rustc_hash::FxHashMap;
use std::path::PathBuf;

/// Maximum length of a one-line diagnostic shown in the 62 px config error panel.
pub(crate) const DIAGNOSTIC_MAX_CHARS: usize = 120;

/// Keeps a parse diagnostic on one line while preserving its location prefix.
/// Path-bearing store diagnostics are not truncated so the path stays exact.
pub(crate) fn fit_diagnostic(message: String) -> String {
    if message.chars().count() <= DIAGNOSTIC_MAX_CHARS {
        message
    } else {
        let mut truncated: String = message
            .chars()
            .take(DIAGNOSTIC_MAX_CHARS.saturating_sub(3))
            .collect();
        truncated.push_str("...");
        truncated
    }
}

/// Defaults are defined by the setting types themselves: `Theme::Dark` and
/// `Suggestions::On`. `Default` is also the fallback config used while a fatal
/// config error is displayed.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Config {
    pub apps: FxHashMap<String, AppAction>,
    pub ps: FxHashMap<String, PsAction>,
    pub theme: Theme,
    pub suggestions: Suggestions,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppAction {
    pub path: PathBuf,
    pub args: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PsAction {
    pub script: String,
    pub body: String,
}

impl Config {
    pub fn config_path() -> PathBuf {
        let home = std::env::var_os("USERPROFILE")
            .or_else(|| std::env::var_os("HOME"))
            .map(PathBuf::from);

        if let Some(home_dir) = home {
            return home_dir.join(".config").join(".shyena");
        }

        PathBuf::from(".config/.shyena")
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

    /// Canonical default config content (used by tests and as the documented
    /// seed for install.ps1); the binary no longer writes it on startup.
    #[allow(dead_code)]
    const DEFAULT_SHYENA: &str = r#"# Shyena Configuration (~/.config/.shyena)

# Theme: 'dark' = Catppuccin Macchiato, 'light' = Catppuccin Latte
Set-Theme 'dark'

# Suggestions: 'on' shows the candidate list, 'off' hides it
Set-Suggestions 'on'

Register-App 'helium' @{
    Path = 'C:/Users/froze/AppData/Local/imput/Helium/Application/chrome.exe'
    Args = @()
}

Register-App 'chat' @{
    Path = 'C:/Users/froze/AppData/Local/imput/Helium/Application/chrome.exe'
    Args = @('--start-fullscreen', 'https://chatgpt.com')
}

function echo {
    Write-Output $args
}

function brightness {
    param(
        [Parameter(Mandatory = $true)]
        [ValidateRange(0, 100)]
        [int]$Level
    )

    $wmi = Get-WmiObject -Namespace root/WMI -Class WmiMonitorBrightnessMethods
    $wmi.WmiSetBrightness(1, $Level)
}
"#;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_shyena_ast_and_function_invocation() {
        let sample = r#"
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
        let ast = Scanner::new(sample).parse().expect("valid shyena syntax");
        let cfg = Config::from_ast(&ast);

        assert_eq!(cfg.apps.len(), 0x02);
        assert_eq!(cfg.ps.len(), 0x01);

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
    fn test_default_config_declares_chat_with_multiple_args() {
        let ast = Scanner::new(Config::DEFAULT_SHYENA).parse().expect("default config parses");
        let cfg = Config::from_ast(&ast);

        assert_eq!(cfg.apps.len(), 0x02);
        assert!(!cfg.apps.contains_key("code"));
        assert_eq!(cfg.theme, Theme::Dark);
        assert_eq!(cfg.suggestions, Suggestions::On);

        let helium = cfg.apps.get("helium").expect("helium exists");
        assert_eq!(helium.args, Vec::<String>::new());

        let chat = cfg.apps.get("chat").expect("chat exists");
        assert_eq!(
            chat.path,
            PathBuf::from("C:/Users/froze/AppData/Local/imput/Helium/Application/chrome.exe")
        );
        assert_eq!(chat.args, vec!["--start-fullscreen", "https://chatgpt.com"]);
    }

    #[test]
    fn test_default_brightness_requires_explicit_level() {
        let ast = Scanner::new(Config::DEFAULT_SHYENA).parse().expect("default config parses");
        let cfg = Config::from_ast(&ast);
        let brightness = cfg.ps.get("brightness").expect("brightness exists");

        assert!(brightness.body.contains("Parameter(Mandatory = $true)"));
        assert!(brightness.body.contains("ValidateRange(0, 100)"));
        assert!(!brightness.body.contains("$args["));
    }

    #[test]
    fn test_theme_from_ast() {
        let light = "Set-Theme 'light'\nRegister-App 'wt' @{ Path = 'wt.exe' }";
        let ast = Scanner::new(light).parse().unwrap();
        let cfg = Config::from_ast(&ast);
        assert_eq!(cfg.theme, Theme::Light);
        assert_eq!(cfg.theme.as_config_str(), "light");

        let missing = "Register-App 'wt' @{ Path = 'wt.exe' }";
        let ast = Scanner::new(missing).parse().unwrap();
        assert_eq!(Config::from_ast(&ast).theme, Theme::Dark);
    }

    #[test]
    fn test_suggestions_from_ast() {
        let off = "Set-Suggestions 'off'\nRegister-App 'wt' @{ Path = 'wt.exe' }";
        let ast = Scanner::new(off).parse().unwrap();
        let cfg = Config::from_ast(&ast);
        assert_eq!(cfg.suggestions, Suggestions::Off);
        assert!(!cfg.suggestions.enabled());
        assert_eq!(cfg.suggestions.as_config_str(), "off");

        let on = "set-suggestions \"ON\"\nfunction f { }";
        let ast = Scanner::new(on).parse().unwrap();
        assert_eq!(Config::from_ast(&ast).suggestions, Suggestions::On);
    }

    #[test]
    fn test_missing_directives_use_defaults() {
        let defaults = Config::default();
        assert_eq!(defaults.theme, Theme::Dark);
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
