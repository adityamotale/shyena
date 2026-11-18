//! Built-in commands addressed through the `:` query scope.
//!
//! These run in-process and never spawn an app or a PowerShell worker. Adding a
//! command is intentionally mechanical: add an [`InternalCommand`] variant, a
//! [`CommandSpec`] row, and a match arm where the command is executed.

/// An in-process command exposed through `:<name>`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InternalCommand {
    Reload,
}

/// Static metadata used for resolution and suggestion rendering.
pub struct CommandSpec {
    pub name: &'static str,
    pub description: &'static str,
    pub command: InternalCommand,
}

pub const COMMANDS: &[CommandSpec] = &[CommandSpec {
    name: "reload",
    description: "Reload .shyena from disk",
    command: InternalCommand::Reload,
}];

/// Resolves a command name case-insensitively.
pub fn find(name: &str) -> Option<&'static CommandSpec> {
    COMMANDS
        .iter()
        .find(|spec| spec.name.eq_ignore_ascii_case(name))
}

/// Commands whose name contains `token`; an empty token lists every command.
pub fn matching(token: &str) -> impl Iterator<Item = &'static CommandSpec> {
    let needle = token.to_ascii_lowercase();
    COMMANDS
        .iter()
        .filter(move |spec| needle.is_empty() || spec.name.to_ascii_lowercase().contains(&needle))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_find_is_case_insensitive() {
        assert_eq!(
            find("reload").map(|spec| spec.command),
            Some(InternalCommand::Reload)
        );
        assert_eq!(
            find("RELOAD").map(|spec| spec.command),
            Some(InternalCommand::Reload)
        );
        assert!(find("nope").is_none());
    }

    #[test]
    fn test_matching_filters_by_substring() {
        assert_eq!(matching("").count(), COMMANDS.len());
        assert_eq!(matching("re").count(), 1);
        assert_eq!(matching("RE").count(), 1);
        assert_eq!(matching("load").count(), 1);
        assert_eq!(matching("xyz").count(), 0);
    }
}
