//! User-defined convenience commands.
//!
//! Named shortcuts wrapping common bobbin query patterns.
//! Stored in `.bobbin/commands.toml` per-repository.

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

/// A user-defined command definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandDef {
    /// Human-readable description of what this command does
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// The bobbin subcommand to invoke (e.g. "search", "hotspots", "context")
    pub command: String,
    /// Default arguments for the subcommand
    #[serde(default)]
    pub args: Vec<String>,
}

/// All user-defined commands, keyed by name.
pub type CommandsMap = BTreeMap<String, CommandDef>;

/// Load commands from `.bobbin/commands.toml`.
/// Returns an empty map if the file doesn't exist.
pub fn load_commands(repo_root: &Path) -> Result<CommandsMap> {
    let path = commands_path(repo_root);
    if !path.exists() {
        return Ok(BTreeMap::new());
    }
    let content = std::fs::read_to_string(&path)
        .with_context(|| format!("Failed to read commands file: {}", path.display()))?;
    toml::from_str(&content)
        .with_context(|| format!("Failed to parse commands file: {}", path.display()))
}

/// Save commands to `.bobbin/commands.toml`.
pub fn save_commands(repo_root: &Path, commands: &CommandsMap) -> Result<()> {
    let path = commands_path(repo_root);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create directory: {}", parent.display()))?;
    }
    let content = toml::to_string_pretty(commands).context("Failed to serialize commands")?;
    std::fs::write(&path, content)
        .with_context(|| format!("Failed to write commands file: {}", path.display()))
}

/// Get the path to the commands file for a repository.
pub fn commands_path(repo_root: &Path) -> std::path::PathBuf {
    repo_root.join(".bobbin").join("commands.toml")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_command_def_serialize() {
        let cmd = CommandDef {
            description: Some("Find test functions".into()),
            command: "search".into(),
            args: vec!["--type".into(), "function".into(), "test".into()],
        };
        let toml_str = toml::to_string_pretty(&cmd).unwrap();
        assert!(toml_str.contains("description = \"Find test functions\""));
        assert!(toml_str.contains("command = \"search\""));
    }

    #[test]
    fn test_command_def_deserialize() {
        let toml_str = r#"
description = "Find test functions"
command = "search"
args = ["--type", "function", "test"]
"#;
        let cmd: CommandDef = toml::from_str(toml_str).unwrap();
        assert_eq!(cmd.description.as_deref(), Some("Find test functions"));
        assert_eq!(cmd.command, "search");
        assert_eq!(cmd.args, vec!["--type", "function", "test"]);
    }

    #[test]
    fn test_command_def_no_description() {
        let toml_str = r#"
command = "hotspots"
args = ["--limit", "20"]
"#;
        let cmd: CommandDef = toml::from_str(toml_str).unwrap();
        assert!(cmd.description.is_none());
        assert_eq!(cmd.command, "hotspots");
    }

    #[test]
    fn test_command_def_no_args() {
        let toml_str = r#"
command = "status"
"#;
        let cmd: CommandDef = toml::from_str(toml_str).unwrap();
        assert_eq!(cmd.command, "status");
        assert!(cmd.args.is_empty());
    }

    #[test]
    fn test_commands_map_roundtrip() {
        let mut commands = CommandsMap::new();
        commands.insert(
            "find-tests".into(),
            CommandDef {
                description: Some("Find test functions".into()),
                command: "search".into(),
                args: vec!["--type".into(), "function".into(), "test".into()],
            },
        );
        commands.insert(
            "api-hotspots".into(),
            CommandDef {
                description: None,
                command: "hotspots".into(),
                args: vec!["--limit".into(), "20".into()],
            },
        );

        let toml_str = toml::to_string_pretty(&commands).unwrap();
        let parsed: CommandsMap = toml::from_str(&toml_str).unwrap();
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed["find-tests"].command, "search");
        assert_eq!(parsed["api-hotspots"].command, "hotspots");
    }

    #[test]
    fn test_load_nonexistent_returns_empty() {
        let tmp = tempfile::tempdir().unwrap();
        let commands = load_commands(tmp.path()).unwrap();
        assert!(commands.is_empty());
    }

    #[test]
    fn test_save_and_load_roundtrip() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join(".bobbin")).unwrap();

        let mut commands = CommandsMap::new();
        commands.insert(
            "my-cmd".into(),
            CommandDef {
                description: Some("Test command".into()),
                command: "search".into(),
                args: vec!["hello".into()],
            },
        );

        save_commands(tmp.path(), &commands).unwrap();
        let loaded = load_commands(tmp.path()).unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded["my-cmd"].command, "search");
        assert_eq!(loaded["my-cmd"].args, vec!["hello"]);
    }
}
