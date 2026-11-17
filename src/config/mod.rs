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
use serde::Deserialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
pub struct Config {
    #[serde(default)]
    pub apps: FxHashMap<String, AppAction>,
    #[serde(default)]
    pub ps: FxHashMap<String, PsAction>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct AppAction {
    pub path: PathBuf,
    #[serde(default)]
    pub args: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct PsAction {
    pub script: String,
}

impl Config {
    pub fn config_path() -> PathBuf {
        if let Ok(user_profile) = std::env::var("USERPROFILE") {
            PathBuf::from(user_profile).join(".config").join("shyena.toml")
        } else if let Ok(home) = std::env::var("HOME") {
            PathBuf::from(home).join(".config").join("shyena.toml")
        } else {
            PathBuf::from(".config/shyena.toml")
        }
    }

    pub fn from_toml(content: &str) -> Result<Self, toml::de::Error> {
        toml::from_str::<Self>(content)
    }

    pub fn load() -> Self {
        let path = Self::config_path();
        if !path.exists() {
            return Self::create_default_config(&path);
        }

        match std::fs::read_to_string(&path) {
            Ok(content) => match Self::from_toml(&content) {
                Ok(cfg) => cfg,
                Err(err) => {
                    eprintln!("Failed to parse config at {:?}: {}", path, err);
                    Self::default()
                }
            },
            Err(err) => {
                eprintln!("Failed to read config at {:?}: {}", path, err);
                Self::default()
            }
        }
    }

    fn create_default_config(path: &Path) -> Self {
        const DEFAULT_CONFIG: &str = r#"# Shyena Configuration (~/.config/shyena.toml)

[apps.code]
path = "code"
args = ["--reuse-window"]

[ps.echo]
script = "Write-Output $args"
"#;
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::write(path, DEFAULT_CONFIG);
        Self::from_toml(DEFAULT_CONFIG).unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_project_sample() {
        let sample = r#"
[apps.br]
path = "C:/Program Files/BraveSoftware/Brave-Browser/Application/brave.exe"
args = ["--profile-directory=Profile 2"]

[apps.code]
path = "C:/Users/froze/AppData/Local/Programs/Microsoft VS Code/Code.exe"
args = ["--reuse-window"]

[ps.set-light]
script = """
$wmi = Get-WmiObject -Namespace root/WMI -Class WmiMonitorBrightnessMethods
$wmi.WmiSetBrightness(1, $args[0])
"""

[ps.yeet]
script = "Remove-Item -Path $args[0] -Recurse -Force"
"#;

        let cfg = Config::from_toml(sample).expect("valid toml syntax");

        assert_eq!(cfg.apps.len(), 0x02);
        assert_eq!(cfg.ps.len(), 0x02);

        let br = cfg.apps.get("br").expect("br app exists");
        assert_eq!(
            br.path,
            PathBuf::from("C:/Program Files/BraveSoftware/Brave-Browser/Application/brave.exe")
        );
        assert_eq!(br.args, vec!["--profile-directory=Profile 2"]);

        let yeet = cfg.ps.get("yeet").expect("yeet ps exists");
        assert_eq!(yeet.script, "Remove-Item -Path $args[0] -Recurse -Force");
    }
}
