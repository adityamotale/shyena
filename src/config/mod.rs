pub mod io;
pub mod parser;
pub mod watcher;

#[allow(unused_imports, dead_code)]
pub use io::{resolve_config_path, ConfigReader};
#[allow(unused_imports, dead_code)]
pub use parser::{AppDef, ConfigAst, FunctionDef, ParseError, Scanner};
#[allow(unused_imports, dead_code)]
pub use watcher::{
    spawn_config_watcher, ConfigSnapshot, OwnedAppDef, OwnedFunctionDef, ReloadError,
    ReloadEvent, ReloadTrigger, RESERVED_RELOAD_CMD,
};

use rustc_hash::FxHashMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Config {
    pub apps: FxHashMap<String, AppAction>,
    pub ps: FxHashMap<String, PsAction>,
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
            let (path, args) = parse_app_hashtable(app.block_span);
            apps.insert(name, AppAction { path, args });
        }

        for func in &ast.functions {
            let name = func.name.to_string();
            let script = format!("function {} {}\n{} @args", name, func.block_span, name);
            ps.insert(name, PsAction { script, body: func.block_span.to_string() });
        }

        Config { apps, ps }
    }

    pub fn from_file(path: &Path) -> Result<Self, Box<dyn std::error::Error>> {
        let mut reader = io::ConfigReader::new(path);
        let content = reader.read()?;
        let ast = parser::Scanner::new(content).parse()?;
        Ok(Self::from_ast(&ast))
    }

    pub fn load() -> Self {
        let path = Self::config_path();
        if !path.exists() {
            return Self::create_default_config(&path);
        }

        match Self::from_file(&path) {
            Ok(cfg) => cfg,
            Err(err) => {
                eprintln!("Failed to parse config at {:?}: {}", path, err);
                Self::default()
            }
        }
    }

    fn create_default_config(path: &Path) -> Self {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }

        const DEFAULT_SHYENA: &str = r#"# Shyena Configuration (~/.config/.shyena)

Register-App 'code' @{
    Path = 'code'
    Args = @('--reuse-window')
}

function echo {
    Write-Output $args
}

function brightness {
    $target = [int]$args[0]
    $wmi = Get-WmiObject -Namespace root/WMI -Class WmiMonitorBrightnessMethods
    $wmi.WmiSetBrightness(1, $target)
}
"#;
        let _ = std::fs::write(path, DEFAULT_SHYENA);
        let ast = parser::Scanner::new(DEFAULT_SHYENA).parse();
        match ast {
            Ok(ref tree) => Self::from_ast(tree),
            Err(_) => Self::default(),
        }
    }
}

impl From<ConfigSnapshot> for Config {
    fn from(snapshot: ConfigSnapshot) -> Self {
        let mut apps = FxHashMap::default();
        let mut ps = FxHashMap::default();

        for app in snapshot.apps {
            let (path, args) = parse_app_hashtable(&app.block_span);
            apps.insert(app.name, AppAction { path, args });
        }

        for func in snapshot.functions {
            let script = format!("function {} {}\n{} @args", func.name, func.block_span, func.name);
            ps.insert(func.name, PsAction { script, body: func.block_span });
        }

        Config { apps, ps }
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

pub fn parse_app_hashtable(span: &str) -> (PathBuf, Vec<String>) {
    let mut path = PathBuf::new();
    let mut args = Vec::new();

    let bytes = span.as_bytes();
    let len = bytes.len();
    let mut i = 0x00;

    while i < len {
        while i < len && (bytes[i].is_ascii_whitespace() || bytes[i] == b';' || bytes[i] == b',') {
            i += 0x01;
        }
        if i >= len {
            break;
        }

        if bytes[i] == b'#' {
            while i < len && bytes[i] != b'\n' {
                i += 0x01;
            }
            continue;
        }

        let key_start = i;
        while i < len && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_' || bytes[i] == b'-') {
            i += 0x01;
        }
        let key = &span[key_start..i];

        while i < len && bytes[i].is_ascii_whitespace() {
            i += 0x01;
        }
        if i < len && bytes[i] == b'=' {
            i += 0x01;
        } else {
            if key_start == i && i < len {
                i += 0x01;
            }
            continue;
        }

        while i < len && bytes[i].is_ascii_whitespace() {
            i += 0x01;
        }
        if i >= len {
            break;
        }

        if key.eq_ignore_ascii_case("Path") {
            if let Some((val, next_i)) = read_quoted_or_unquoted_string(span, i) {
                path = PathBuf::from(val);
                i = next_i;
            }
        } else if key.eq_ignore_ascii_case("Args") {
            if (bytes[i] == b'@' && i + 0x01 < len && bytes[i + 0x01] == b'(') || bytes[i] == b'(' {
                if bytes[i] == b'@' {
                    i += 0x02;
                } else {
                    i += 0x01;
                }
                while i < len && bytes[i] != b')' {
                    while i < len && (bytes[i].is_ascii_whitespace() || bytes[i] == b',') {
                        i += 0x01;
                    }
                    if i < len && bytes[i] == b')' {
                        break;
                    }
                    if let Some((val, next_i)) = read_quoted_or_unquoted_string(span, i) {
                        args.push(val);
                        i = next_i;
                    } else {
                        i += 0x01;
                    }
                }
                if i < len && bytes[i] == b')' {
                    i += 0x01;
                }
            } else if let Some((val, next_i)) = read_quoted_or_unquoted_string(span, i) {
                args.push(val);
                i = next_i;
            }
        }
    }

    (path, args)
}

fn read_quoted_or_unquoted_string(src: &str, start: usize) -> Option<(String, usize)> {
    let bytes = src.as_bytes();
    let len = bytes.len();
    if start >= len {
        return None;
    }

    let quote = bytes[start];
    if quote == b'\'' || quote == b'"' {
        let mut i = start + 0x01;
        let mut result = String::new();
        while i < len {
            if bytes[i] == quote {
                if i + 0x01 < len && bytes[i + 0x01] == quote {
                    result.push(quote as char);
                    i += 0x02;
                    continue;
                }
                i += 0x01;
                return Some((result, i));
            } else if quote == b'"' && bytes[i] == b'`' && i + 0x01 < len {
                result.push(bytes[i + 0x01] as char);
                i += 0x02;
            } else {
                result.push(bytes[i] as char);
                i += 0x01;
            }
        }
        Some((result, i))
    } else {
        let mut i = start;
        let val_start = i;
        while i < len
            && !bytes[i].is_ascii_whitespace()
            && bytes[i] != b';'
            && bytes[i] != b','
            && bytes[i] != b')'
            && bytes[i] != b'}'
        {
            i += 0x01;
        }
        if i > val_start {
            Some((src[val_start..i].to_string(), i))
        } else {
            None
        }
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
    fn test_encode_powershell_script() {
        let script = "Write-Output 'OK'";
        let encoded = encode_powershell_script(script);
        assert_eq!(encoded, "VwByAGkAdABlAC0ATwB1AHQAcAB1AHQAIAAnAE8ASwAnAA==");
    }
}
