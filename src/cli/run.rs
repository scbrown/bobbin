use anyhow::{bail, Result};
use clap::Args;
use colored::Colorize;
use serde::Serialize;

use super::OutputConfig;
use crate::commands::{self, CommandDef};

#[derive(Args)]
pub struct RunArgs {
    /// Name of the command to execute
    name: Option<String>,

    /// List all defined commands
    #[arg(long, short = 'l')]
    list: bool,

    /// Save a new command (usage: --save NAME -- COMMAND [ARGS...])
    #[arg(long)]
    save: Option<String>,

    /// Remove a command definition
    #[arg(long)]
    remove: Option<String>,

    /// Show a command definition
    #[arg(long)]
    show: Option<String>,

    /// Description for the command being saved (use with --save)
    #[arg(long, short = 'd')]
    description: Option<String>,

    /// Directory containing the bobbin repo (defaults to current directory)
    #[arg(long, default_value = ".")]
    path: std::path::PathBuf,

    /// Additional arguments appended to the saved command args.
    /// Use `--` to separate from run flags (e.g. `bobbin run my-cmd -- --limit 5`)
    #[arg(last = true)]
    extra_args: Vec<String>,
}

/// JSON output for listing commands
#[derive(Serialize)]
struct CommandListOutput {
    count: usize,
    commands: Vec<CommandListEntry>,
}

#[derive(Serialize)]
struct CommandListEntry {
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,
    command: String,
    args: Vec<String>,
    /// The full command line as a string
    expands_to: String,
}

/// Result of resolving a `bobbin run` invocation.
/// `Execute` means we need to re-dispatch through `Cli::run()` with new args.
/// `Done` means the management operation was completed (list, save, remove, show).
pub enum RunResult {
    /// Re-dispatch: the resolved CLI should be run by the caller
    Execute(Vec<String>),
    /// Management operation completed, nothing more to do
    Done,
}

/// Resolve a `bobbin run` invocation. Returns either resolved CLI args for
/// re-dispatch, or signals that a management operation was already completed.
/// This avoids async recursion between `run::run` and `Cli::run`.
pub fn resolve(args: RunArgs, output: &OutputConfig) -> Result<RunResult> {
    let repo_root = args
        .path
        .canonicalize()
        .unwrap_or_else(|_| args.path.clone());

    // Dispatch to the appropriate suboperation
    if args.list {
        list_commands(&repo_root, output)?;
        return Ok(RunResult::Done);
    }

    if let Some(name) = &args.show {
        show_command(&repo_root, name, output)?;
        return Ok(RunResult::Done);
    }

    if let Some(name) = &args.remove {
        remove_command(&repo_root, name, output)?;
        return Ok(RunResult::Done);
    }

    if let Some(name) = &args.save {
        save_command(&repo_root, name, args.description.as_deref(), &args.extra_args, output)?;
        return Ok(RunResult::Done);
    }

    // Execute a named command — return args for re-dispatch
    if let Some(name) = &args.name {
        let full_args = build_command_args(name, &repo_root, &args.extra_args, output)?;
        return Ok(RunResult::Execute(full_args));
    }

    // No action specified — show help
    bail!(
        "No command specified. Usage:\n  \
         bobbin run <name>              Execute a saved command\n  \
         bobbin run --list              List all saved commands\n  \
         bobbin run --save NAME -- CMD  Save a new command\n  \
         bobbin run --show NAME         Show command definition\n  \
         bobbin run --remove NAME       Remove a command"
    );
}

fn list_commands(repo_root: &std::path::Path, output: &OutputConfig) -> Result<()> {
    let commands = commands::load_commands(repo_root)?;

    if output.json {
        let entries: Vec<CommandListEntry> = commands
            .iter()
            .map(|(name, def)| CommandListEntry {
                name: name.clone(),
                description: def.description.clone(),
                command: def.command.clone(),
                args: def.args.clone(),
                expands_to: expand_command(def),
            })
            .collect();

        let list_output = CommandListOutput {
            count: entries.len(),
            commands: entries,
        };
        println!("{}", serde_json::to_string_pretty(&list_output)?);
        return Ok(());
    }

    if commands.is_empty() {
        if !output.quiet {
            println!(
                "{} No commands defined. Save one with: bobbin run --save NAME -- COMMAND [ARGS...]",
                "!".yellow()
            );
        }
        return Ok(());
    }

    if !output.quiet {
        println!(
            "{} {} saved command{}:",
            "✓".green(),
            commands.len(),
            if commands.len() == 1 { "" } else { "s" }
        );
        println!();

        for (name, def) in &commands {
            let desc = def
                .description
                .as_deref()
                .map(|d| format!(" — {}", d.dimmed()))
                .unwrap_or_default();

            println!("  {}{}", name.bold(), desc);
            println!("    {}", expand_command(def).dimmed());
            println!();
        }
    }

    Ok(())
}

fn show_command(repo_root: &std::path::Path, name: &str, output: &OutputConfig) -> Result<()> {
    let commands = commands::load_commands(repo_root)?;

    let Some(def) = commands.get(name) else {
        bail!("Command '{}' not found. Run `bobbin run --list` to see available commands.", name);
    };

    if output.json {
        let entry = CommandListEntry {
            name: name.to_string(),
            description: def.description.clone(),
            command: def.command.clone(),
            args: def.args.clone(),
            expands_to: expand_command(def),
        };
        println!("{}", serde_json::to_string_pretty(&entry)?);
        return Ok(());
    }

    if !output.quiet {
        let desc = def
            .description
            .as_deref()
            .map(|d| format!(" — {}", d))
            .unwrap_or_default();

        println!("{}{}", name.bold(), desc);
        println!("  Expands to: {}", expand_command(def).cyan());
    }

    Ok(())
}

fn save_command(
    repo_root: &std::path::Path,
    name: &str,
    description: Option<&str>,
    extra_args: &[String],
    output: &OutputConfig,
) -> Result<()> {
    if extra_args.is_empty() {
        bail!(
            "No command specified. Usage: bobbin run --save {} -- COMMAND [ARGS...]\n\
             Example: bobbin run --save find-tests -- search --type function test",
            name
        );
    }

    // Validate name: alphanumeric, hyphens, underscores only
    if !name
        .chars()
        .all(|c| c.is_alphanumeric() || c == '-' || c == '_')
    {
        bail!("Command name must contain only alphanumeric characters, hyphens, and underscores");
    }

    // The first extra arg is the bobbin subcommand, the rest are its args
    let command = extra_args[0].clone();
    let args = extra_args[1..].to_vec();

    // Prevent saving "run" as the underlying command (would cause recursion)
    if command == "run" {
        bail!("Cannot save 'run' as the underlying command (would cause recursion)");
    }

    // Validate that the command is a known bobbin subcommand
    validate_subcommand(&command)?;

    let mut commands = commands::load_commands(repo_root)?;

    let is_update = commands.contains_key(name);

    commands.insert(
        name.to_string(),
        CommandDef {
            description: description.map(|s| s.to_string()),
            command,
            args,
        },
    );

    commands::save_commands(repo_root, &commands)?;

    if output.json {
        let entry = &commands[name];
        let json_entry = CommandListEntry {
            name: name.to_string(),
            description: entry.description.clone(),
            command: entry.command.clone(),
            args: entry.args.clone(),
            expands_to: expand_command(entry),
        };
        println!("{}", serde_json::to_string_pretty(&json_entry)?);
    } else if !output.quiet {
        let verb = if is_update { "Updated" } else { "Saved" };
        println!(
            "{} {} command '{}': {}",
            "✓".green(),
            verb,
            name.bold(),
            expand_command(&commands[name]).cyan()
        );
    }

    Ok(())
}

fn remove_command(repo_root: &std::path::Path, name: &str, output: &OutputConfig) -> Result<()> {
    let mut commands = commands::load_commands(repo_root)?;

    if commands.remove(name).is_none() {
        bail!(
            "Command '{}' not found. Run `bobbin run --list` to see available commands.",
            name
        );
    }

    commands::save_commands(repo_root, &commands)?;

    if output.json {
        println!(r#"{{"removed": "{}"}}"#, name);
    } else if !output.quiet {
        println!("{} Removed command '{}'", "✓".green(), name.bold());
    }

    Ok(())
}

/// Build the full CLI argument vector for a saved command, suitable for
/// re-parsing through `Cli::try_parse_from`.
fn build_command_args(
    name: &str,
    repo_root: &std::path::Path,
    extra_args: &[String],
    output: &OutputConfig,
) -> Result<Vec<String>> {
    let commands = commands::load_commands(repo_root)?;

    let Some(def) = commands.get(name) else {
        bail!(
            "Command '{}' not found. Run `bobbin run --list` to see available commands.",
            name
        );
    };

    let mut full_args = vec!["bobbin".to_string()];

    // Forward current global flags
    if output.json {
        full_args.push("--json".to_string());
    }
    if output.quiet {
        full_args.push("--quiet".to_string());
    }
    if output.verbose {
        full_args.push("--verbose".to_string());
    }
    if let Some(ref server) = output.server {
        full_args.push("--server".to_string());
        full_args.push(server.clone());
    }

    // Add the stored subcommand and args
    full_args.push(def.command.clone());
    full_args.extend(def.args.iter().cloned());

    // Add any extra args from the user
    full_args.extend(extra_args.iter().cloned());

    Ok(full_args)
}

/// Expand a command definition into the equivalent CLI invocation string.
fn expand_command(def: &CommandDef) -> String {
    let mut parts = vec![format!("bobbin {}", def.command)];
    for arg in &def.args {
        if arg.contains(' ') {
            parts.push(format!("\"{}\"", arg));
        } else {
            parts.push(arg.clone());
        }
    }
    parts.join(" ")
}

/// Validate that a command name is a known bobbin subcommand.
fn validate_subcommand(name: &str) -> Result<()> {
    const VALID_COMMANDS: &[&str] = &[
        "init", "index", "search", "context", "deps", "grep", "refs", "related", "history",
        "log", "hotspots", "impact", "review", "similar", "status", "serve", "benchmark",
        "watch", "completions", "hook", "tour", "prime",
    ];

    if VALID_COMMANDS.contains(&name) {
        Ok(())
    } else {
        bail!(
            "Unknown bobbin command '{}'. Valid commands: {}",
            name,
            VALID_COMMANDS.join(", ")
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_expand_command_basic() {
        let def = CommandDef {
            description: None,
            command: "search".into(),
            args: vec!["--type".into(), "function".into(), "test".into()],
        };
        assert_eq!(expand_command(&def), "bobbin search --type function test");
    }

    #[test]
    fn test_expand_command_no_args() {
        let def = CommandDef {
            description: None,
            command: "status".into(),
            args: vec![],
        };
        assert_eq!(expand_command(&def), "bobbin status");
    }

    #[test]
    fn test_expand_command_quoted_args() {
        let def = CommandDef {
            description: None,
            command: "search".into(),
            args: vec!["hello world".into()],
        };
        assert_eq!(expand_command(&def), "bobbin search \"hello world\"");
    }

    #[test]
    fn test_validate_subcommand_valid() {
        assert!(validate_subcommand("search").is_ok());
        assert!(validate_subcommand("hotspots").is_ok());
        assert!(validate_subcommand("context").is_ok());
        assert!(validate_subcommand("status").is_ok());
    }

    #[test]
    fn test_validate_subcommand_invalid() {
        assert!(validate_subcommand("unknown").is_err());
        assert!(validate_subcommand("run").is_err());
        assert!(validate_subcommand("").is_err());
    }

    #[test]
    fn test_validate_subcommand_rejects_run() {
        // "run" is not in VALID_COMMANDS, preventing recursion
        assert!(validate_subcommand("run").is_err());
    }

    #[test]
    fn test_list_commands_empty_json() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join(".bobbin")).unwrap();

        let output = OutputConfig {
            json: true,
            quiet: false,
            verbose: false,
            server: None,
        };

        // Should succeed with empty list
        let result = list_commands(tmp.path(), &output);
        assert!(result.is_ok());
    }

    #[test]
    fn test_save_and_show_roundtrip() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join(".bobbin")).unwrap();

        let output = OutputConfig {
            json: false,
            quiet: true,
            verbose: false,
            server: None,
        };

        // Save a command
        let extra = vec!["search".into(), "--type".into(), "function".into(), "test".into()];
        save_command(tmp.path(), "find-tests", Some("Find tests"), &extra, &output).unwrap();

        // Verify it was saved
        let commands = commands::load_commands(tmp.path()).unwrap();
        assert_eq!(commands.len(), 1);
        assert_eq!(commands["find-tests"].command, "search");
        assert_eq!(
            commands["find-tests"].args,
            vec!["--type", "function", "test"]
        );
        assert_eq!(
            commands["find-tests"].description.as_deref(),
            Some("Find tests")
        );
    }

    #[test]
    fn test_save_rejects_invalid_name() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join(".bobbin")).unwrap();

        let output = OutputConfig {
            json: false,
            quiet: true,
            verbose: false,
            server: None,
        };

        let extra = vec!["search".into(), "test".into()];
        let result = save_command(tmp.path(), "bad name!", None, &extra, &output);
        assert!(result.is_err());
    }

    #[test]
    fn test_save_rejects_run_command() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join(".bobbin")).unwrap();

        let output = OutputConfig {
            json: false,
            quiet: true,
            verbose: false,
            server: None,
        };

        let extra = vec!["run".into(), "other".into()];
        let result = save_command(tmp.path(), "recursive", None, &extra, &output);
        assert!(result.is_err());
    }

    #[test]
    fn test_save_rejects_unknown_command() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join(".bobbin")).unwrap();

        let output = OutputConfig {
            json: false,
            quiet: true,
            verbose: false,
            server: None,
        };

        let extra = vec!["nonexistent".into(), "arg".into()];
        let result = save_command(tmp.path(), "bad-cmd", None, &extra, &output);
        assert!(result.is_err());
    }

    #[test]
    fn test_remove_nonexistent() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join(".bobbin")).unwrap();

        let output = OutputConfig {
            json: false,
            quiet: true,
            verbose: false,
            server: None,
        };

        let result = remove_command(tmp.path(), "nonexistent", &output);
        assert!(result.is_err());
    }

    #[test]
    fn test_remove_existing() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join(".bobbin")).unwrap();

        let output = OutputConfig {
            json: false,
            quiet: true,
            verbose: false,
            server: None,
        };

        // Save then remove
        let extra = vec!["search".into(), "test".into()];
        save_command(tmp.path(), "my-cmd", None, &extra, &output).unwrap();
        remove_command(tmp.path(), "my-cmd", &output).unwrap();

        let commands = commands::load_commands(tmp.path()).unwrap();
        assert!(commands.is_empty());
    }
}
