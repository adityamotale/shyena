pub mod io;
pub mod parser;
pub mod store;

#[allow(unused_imports, dead_code)]
pub use io::{resolve_config_path, ConfigReader};
#[allow(unused_imports, dead_code)]
pub use parser::{AppDef, ConfigAst, FunctionDef, ParseError, Scanner};
pub use store::{ConfigStore, ReloadOutcome};

use crate::theme::Theme;
use rustc_hash::FxHashMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Config {
    pub apps: FxHashMap<String, AppAction>,
    pub ps: FxHashMap<String, PsAction>,
    pub theme: Theme,
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

        Config {
            apps,
            ps,
            theme: ast.theme.unwrap_or_default(),
        }
    }

    /// Configuration seeded on first launch; also the canonical multi-arg example.
    const DEFAULT_SHYENA: &str = r#"# Shyena Configuration (~/.config/.shyena)

# Theme: 'dark' = Catppuccin Macchiato, 'light' = Catppuccin Latte
Set-Theme 'dark'

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

    pub(crate) fn create_default_config(path: &Path) -> Self {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }

        let _ = std::fs::write(path, Self::DEFAULT_SHYENA);
        let ast = parser::Scanner::new(Self::DEFAULT_SHYENA).parse();
        match ast {
            Ok(ref tree) => Self::from_ast(tree),
            Err(_) => Self::default(),
        }
    }
}

pub fn encode_powershell_script(script: &str) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let utf16: Vec<u8> = script.encode_utf16().flat_map(|u| u.to_le_bytes()).collect();
    let mut out = String::with_capacity((utf16.len() + 2) / 3 * 4);
    let chunks = utf16.chunks_exact(3);
    let remainder = chunks.remainder();

    for chunk in chunks {
        let b0 = chunk[0] as usize;
        let b1 = chunk[1] as usize;
        let b2 = chunk[2] as usize;
        out.push(TABLE[b0 >> 2] as char);
        out.push(TABLE[((b0 & 0x03) << 4) | (b1 >> 4)] as char);
        out.push(TABLE[((b1 & 0x0F) << 2) | (b2 >> 6)] as char);
        out.push(TABLE[b2 & 0x3F] as char);
    }

    match remainder.len() {
        1 => {
            let b0 = remainder[0] as usize;
            out.push(TABLE[b0 >> 2] as char);
            out.push(TABLE[(b0 & 0x03) << 4] as char);
            out.push('=');
            out.push('=');
        }
        2 => {
            let b0 = remainder[0] as usize;
            let b1 = remainder[1] as usize;
            out.push(TABLE[b0 >> 2] as char);
            out.push(TABLE[((b0 & 0x03) << 4) | (b1 >> 4)] as char);
            out.push(TABLE[(b1 & 0x0F) << 2] as char);
            out.push('=');
        }
        _ => {}
    }

    out
}

/// Parses an app hashtable block using the strict scanner. Intended for compatibility/testing.
/// Returns `(PathBuf, Vec<String>)` or defaults on parse error.
pub fn parse_app_hashtable(span: &str) -> (PathBuf, Vec<String>) {
    let src = format!("Register-App 'x' @{}", span.trim());
    match parser::Scanner::new(&src).parse() {
        Ok(ast) if !ast.apps.is_empty() => (ast.apps[0].path.clone(), ast.apps[0].args.clone()),
        _ => (PathBuf::new(), Vec::new()),
    }
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
    fn test_encode_powershell_script() {
        let script = "Write-Output 'OK'";
        let encoded = encode_powershell_script(script);
        assert_eq!(encoded, "VwByAGkAdABlAC0ATwB1AHQAcAB1AHQAIAAnAE8ASwAnAA==");
    }
}
