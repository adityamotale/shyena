//! Query resolution and command dispatch types.
//!
//! A query is split into a scope prefix (`@` app, `#` PowerShell, `:` built-in),
//! a key, and dynamic arguments. Resolution produces a [`CommandIntent`], which
//! is the only representation the UI executes — candidate actions are
//! presentation data and are converted to intents at the point of activation.

use async_channel::Sender;

use crate::{commands, config, exec};

/// The three kinds of action the launcher can execute.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ActionKind {
    App,
    Ps,
    Internal,
}

/// A candidate row in the suggestion list. `detail` is presentation metadata
/// only; execution always goes through [`CandidateAction::to_intent`].
#[derive(Clone)]
pub(crate) struct CandidateAction {
    pub(crate) key: String,
    pub(crate) kind: ActionKind,
    pub(crate) detail: String,
}

impl CandidateAction {
    /// Converts a selected candidate into an executable intent. Only the key
    /// is copied; presentation data is deliberately left behind.
    pub(crate) fn to_intent(&self, args: Vec<String>) -> CommandIntent {
        match self.kind {
            ActionKind::App => CommandIntent::App {
                key: self.key.clone(),
                args,
            },
            ActionKind::Ps => CommandIntent::Ps {
                key: self.key.clone(),
                args,
            },
            ActionKind::Internal => CommandIntent::Internal {
                key: self.key.clone(),
                args,
            },
        }
    }
}

/// A fully resolved command ready to execute.
#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
pub(crate) enum CommandIntent {
    App { key: String, args: Vec<String> },
    Ps { key: String, args: Vec<String> },
    Internal { key: String, args: Vec<String> },
}

/// Query prefix namespaces. `:` addresses built-in commands; apps and PowerShell
/// functions keep their existing `@` / `#` scopes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Scope {
    App,
    Ps,
    Internal,
}

impl Scope {
    fn from_char(ch: char) -> Option<Self> {
        match ch {
            '@' => Some(Self::App),
            '#' => Some(Self::Ps),
            ':' => Some(Self::Internal),
            _ => None,
        }
    }

    pub(crate) fn prefix(self) -> char {
        match self {
            Self::App => '@',
            Self::Ps => '#',
            Self::Internal => ':',
        }
    }
}

impl From<Scope> for ActionKind {
    fn from(scope: Scope) -> Self {
        match scope {
            Scope::App => Self::App,
            Scope::Ps => Self::Ps,
            Scope::Internal => Self::Internal,
        }
    }
}

/// Splits a single leading scope prefix from the query. Both `@app` and
/// `@ app` forms are accepted; callers trim the returned remainder as needed.
pub(crate) fn split_scope(input: &str) -> Option<(Scope, &str)> {
    let trimmed = input.trim_start();
    let ch = trimmed.chars().next()?;
    let scope = Scope::from_char(ch)?;
    Some((scope, &trimmed[ch.len_utf8()..]))
}

/// Resolves a committed query into an executable target. Candidate fallback
/// preserves the existing "type a prefix, press Enter" flow; an unmatched
/// token becomes an explicit error instead of a silent stdout print.
pub(crate) fn resolve_command(
    config: &config::Config,
    query: &str,
    selected: Option<&CandidateAction>,
) -> Result<CommandIntent, String> {
    let trimmed = query.trim();
    if trimmed.is_empty() {
        return selected
            .map(|candidate| candidate.to_intent(Vec::new()))
            .ok_or_else(|| "empty command".to_string());
    }

    let (scope, rest) = match split_scope(trimmed) {
        Some((scope, rest)) => (Some(scope), rest.trim_start()),
        None => (None, trimmed),
    };

    let mut tokens = rest.split_whitespace();
    let key = tokens.next().unwrap_or("");
    if key.is_empty() {
        return selected
            .map(|candidate| candidate.to_intent(Vec::new()))
            .ok_or_else(|| "expected command after prefix".to_string());
    }

    let args = tokens.map(str::to_string).collect::<Vec<_>>();

    if scope == Some(Scope::Internal) {
        if commands::find(key).is_some() {
            return Ok(CommandIntent::Internal {
                key: key.to_string(),
                args,
            });
        }

        return selected
            .filter(|candidate| candidate.kind == ActionKind::Internal)
            .map(|candidate| candidate.to_intent(args))
            .ok_or_else(|| format!("unknown command '{key}'"));
    }

    if scope != Some(Scope::Ps) && config.apps.contains_key(key) {
        return Ok(CommandIntent::App {
            key: key.to_string(),
            args,
        });
    }

    if scope != Some(Scope::App) && config.ps.contains_key(key) {
        return Ok(CommandIntent::Ps {
            key: key.to_string(),
            args,
        });
    }

    selected
        .map(|candidate| candidate.to_intent(args))
        .ok_or_else(|| format!("unknown command '{key}'"))
}

/// A PowerShell failure delivered from a worker thread to the UI channel.
#[derive(Clone)]
pub(crate) struct PsError {
    pub(crate) message: String,
}

/// Runs one PowerShell function and reports failures through the shared error
/// channel. Blocking: callers must run this off the UI thread.
pub(crate) fn run_ps_task(
    function_name: &str,
    function_body: &str,
    dynamic_tokens: &[String],
    error_tx: &Sender<PsError>,
) {
    let args: Vec<&str> = dynamic_tokens.iter().map(String::as_str).collect();
    if let Err(err) = exec::dispatch_ps_function(function_name, function_body, &args) {
        let _ = error_tx.send_blocking(PsError {
            message: exec::format_ps_error(function_name, &err),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ActionKind, CandidateAction, CommandIntent, PsError, Scope, resolve_command, run_ps_task,
        split_scope,
    };
    use crate::config::{AppAction, Config, PsAction};
    use std::path::PathBuf;

    fn test_config() -> Config {
        let mut config = Config::default();
        config.apps.insert(
            "app".to_string(),
            AppAction {
                path: PathBuf::from("app.exe"),
                args: Vec::new(),
            },
        );
        config.ps.insert(
            "ps".to_string(),
            PsAction {
                script: "ps".to_string(),
                body: "{}".to_string(),
            },
        );
        config
    }

    #[test]
    fn test_resolve_command_accepts_known_targets_and_reload() {
        let config = test_config();
        assert_eq!(
            resolve_command(&config, "@app arg", None),
            Ok(CommandIntent::App {
                key: "app".to_string(),
                args: vec!["arg".to_string()],
            })
        );
        assert_eq!(
            resolve_command(&config, "#ps 1", None),
            Ok(CommandIntent::Ps {
                key: "ps".to_string(),
                args: vec!["1".to_string()],
            })
        );
        assert_eq!(
            resolve_command(&config, ":reload", None),
            Ok(CommandIntent::Internal {
                key: "reload".to_string(),
                args: Vec::new(),
            })
        );
        assert_eq!(
            resolve_command(&config, ": reload", None),
            Ok(CommandIntent::Internal {
                key: "reload".to_string(),
                args: Vec::new(),
            })
        );
    }

    #[test]
    fn test_resolve_command_scopes_internal_commands() {
        let config = test_config();
        assert_eq!(
            resolve_command(&config, ":app", None),
            Err("unknown command 'app'".to_string())
        );
        assert_eq!(
            resolve_command(&config, ":ps", None),
            Err("unknown command 'ps'".to_string())
        );
        assert_eq!(
            resolve_command(&config, "@reload", None),
            Err("unknown command 'reload'".to_string())
        );
        assert_eq!(
            resolve_command(&config, "#reload", None),
            Err("unknown command 'reload'".to_string())
        );
        assert_eq!(
            resolve_command(&config, "reload", None),
            Err("unknown command 'reload'".to_string())
        );
    }

    #[test]
    fn test_resolve_command_forwards_internal_arguments() {
        let config = test_config();
        assert_eq!(
            resolve_command(&config, ":reload extra", None),
            Ok(CommandIntent::Internal {
                key: "reload".to_string(),
                args: vec!["extra".to_string()],
            })
        );
    }

    #[test]
    fn test_resolve_command_rejects_unknown_and_misscoped_tokens() {
        let config = test_config();
        assert_eq!(
            resolve_command(&config, "unknown", None),
            Err("unknown command 'unknown'".to_string())
        );
        assert_eq!(
            resolve_command(&config, "@ps", None),
            Err("unknown command 'ps'".to_string())
        );
        assert_eq!(
            resolve_command(&config, "#app", None),
            Err("unknown command 'app'".to_string())
        );
        assert_eq!(
            resolve_command(&config, "@", None),
            Err("expected command after prefix".to_string())
        );
    }

    #[test]
    fn test_resolve_command_falls_back_to_selected_candidate() {
        let config = test_config();
        let candidate = CandidateAction {
            key: "ps".to_string(),
            kind: ActionKind::Ps,
            detail: String::new(),
        };
        assert_eq!(
            resolve_command(&config, "p", Some(&candidate)),
            Ok(CommandIntent::Ps {
                key: "ps".to_string(),
                args: Vec::new(),
            })
        );
    }

    #[test]
    fn test_resolve_command_falls_back_to_selected_internal_candidate() {
        let config = test_config();
        let candidate = CandidateAction {
            key: "reload".to_string(),
            kind: ActionKind::Internal,
            detail: String::new(),
        };
        assert_eq!(
            resolve_command(&config, ":re", Some(&candidate)),
            Ok(CommandIntent::Internal {
                key: "reload".to_string(),
                args: Vec::new(),
            })
        );
    }

    #[test]
    fn test_split_scope_parses_combined_and_spaced_prefixes() {
        assert_eq!(split_scope("@app"), Some((Scope::App, "app")));
        assert_eq!(split_scope("@ app"), Some((Scope::App, " app")));
        assert_eq!(split_scope("#ps"), Some((Scope::Ps, "ps")));
        assert_eq!(split_scope("# ps"), Some((Scope::Ps, " ps")));
        assert_eq!(split_scope(":reload"), Some((Scope::Internal, "reload")));
        assert_eq!(split_scope(": reload"), Some((Scope::Internal, " reload")));
        assert_eq!(split_scope("  :reload"), Some((Scope::Internal, "reload")));
        assert_eq!(split_scope("plain"), None);
        assert_eq!(split_scope(""), None);
    }

    #[test]
    fn test_run_ps_task_reports_missing_argument_to_ui_channel() {
        let (error_tx, error_rx) = async_channel::unbounded::<PsError>();

        run_ps_task("brightness", "{ [int]$args[0] }", &[], &error_tx);

        let error = error_rx.try_recv().expect("error reported to UI");
        assert_eq!(error.message, "brightness: requires at least one argument");
    }
}
