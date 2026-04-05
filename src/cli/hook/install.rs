use anyhow::{Context, Result};
use colored::Colorize;
use serde_json::json;
use clap::Args;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::config::Config;
use super::types::find_bobbin_root;
use super::state::{load_hook_state, HookState};
use super::git_hook::GIT_HOOK_START_MARKER;
use super::{OutputConfig, HookStatusOutput, HookConfigOutput};

#[derive(Args)]
pub(super) struct InstallArgs {
    /// Install globally (~/.claude/settings.json) instead of project-local
    #[arg(long)]
    pub(super) global: bool,

    /// Minimum relevance score to include in injected context
    #[arg(long)]
    pub(super) threshold: Option<f32>,

    /// Maximum lines of injected context
    #[arg(long)]
    pub(super) budget: Option<usize>,
}

#[derive(Args)]
pub(super) struct UninstallArgs {
    /// Uninstall from global settings instead of project-local
    #[arg(long)]
    pub(super) global: bool,
}

#[derive(Args)]
pub(super) struct StatusArgs {
    /// Directory to check (defaults to current directory)
    #[arg(default_value = ".")]
    pub(super) path: PathBuf,
}

/// Resolve the target settings.json path.
/// --global → ~/.claude/settings.json
/// otherwise → <git-root>/.claude/settings.json
pub(crate) fn resolve_settings_path(global: bool) -> Result<PathBuf> {
    if global {
        let home = std::env::var("HOME").context("HOME not set")?;
        Ok(PathBuf::from(home).join(".claude").join("settings.json"))
    } else {
        let output = Command::new("git")
            .args(["rev-parse", "--show-toplevel"])
            .output()
            .context("Failed to run git rev-parse")?;
        if !output.status.success() {
            anyhow::bail!("Not in a git repository. Use --global or run from a git repo.");
        }
        let root = String::from_utf8_lossy(&output.stdout).trim().to_string();
        Ok(PathBuf::from(root).join(".claude").join("settings.json"))
    }
}

/// Build the bobbin hook entries for Claude Code settings.json.
/// When server_url is Some, bakes BOBBIN_SERVER into the hook commands
/// so agents don't need local initialization.
pub(crate) fn bobbin_hook_entries_with_server(server_url: Option<&str>) -> serde_json::Value {
    let prefix = match server_url {
        Some(url) => format!("BOBBIN_SERVER={} ", url),
        None => String::new(),
    };
    json!({
        "hooks": {
            "UserPromptSubmit": [
                {
                    "hooks": [
                        {
                            "type": "command",
                            "command": format!("{}bobbin hook inject-context || true", prefix),
                            "timeout": 10,
                            "statusMessage": "Loading code context..."
                        }
                    ]
                }
            ],
            "SessionStart": [
                {
                    "matcher": "compact",
                    "hooks": [
                        {
                            "type": "command",
                            "command": format!("{}bobbin hook session-context || true", prefix),
                            "timeout": 10,
                            "statusMessage": "Recovering project context..."
                        }
                    ]
                }
            ],
            "PostToolUse": [
                {
                    "matcher": "Write|Edit|Bash|Grep|Glob|Read",
                    "hooks": [
                        {
                            "type": "command",
                            "command": format!("{}bobbin hook post-tool-use || true", prefix),
                            "timeout": 10,
                            "statusMessage": "Analyzing file changes..."
                        }
                    ]
                }
            ],
            "PostToolUseFailure": [
                {
                    "hooks": [
                        {
                            "type": "command",
                            "command": format!("{}bobbin hook post-tool-use-failure || true", prefix),
                            "timeout": 10,
                            "statusMessage": "Searching for related context..."
                        }
                    ]
                }
            ]
        }
    })
}

/// Build hook entries without server URL (backwards-compatible).
pub(super) fn bobbin_hook_entries() -> serde_json::Value {
    bobbin_hook_entries_with_server(None)
}

/// Check if a hook group entry contains a bobbin command.
pub(super) fn is_bobbin_hook_group(group: &serde_json::Value) -> bool {
    if let Some(hooks) = group.get("hooks").and_then(|h| h.as_array()) {
        hooks.iter().any(|h| {
            h.get("command")
                .and_then(|c| c.as_str())
                .map(|c| c.starts_with("bobbin hook "))
                .unwrap_or(false)
        })
    } else {
        false
    }
}

/// Merge bobbin hooks into an existing settings object.
/// Preserves non-bobbin hooks in each event array.
pub(super) fn merge_hooks(settings: &mut serde_json::Value) {
    let bobbin = bobbin_hook_entries();
    merge_hooks_with(settings, &bobbin);
}

pub(crate) fn merge_hooks_with(settings: &mut serde_json::Value, bobbin: &serde_json::Value) {
    let bobbin_hooks = bobbin.get("hooks").unwrap().as_object().unwrap();

    // Ensure settings.hooks exists as an object
    if settings.get("hooks").is_none() || !settings["hooks"].is_object() {
        settings["hooks"] = json!({});
    }

    for (event_name, bobbin_entries) in bobbin_hooks {
        let bobbin_arr = bobbin_entries.as_array().unwrap();

        if let Some(existing) = settings["hooks"].get_mut(event_name) {
            if let Some(arr) = existing.as_array_mut() {
                // Remove old bobbin entries, then append new ones
                arr.retain(|entry| !is_bobbin_hook_group(entry));
                arr.extend(bobbin_arr.iter().cloned());
            } else {
                // Event key exists but isn't an array — replace
                settings["hooks"][event_name] = serde_json::Value::Array(bobbin_arr.clone());
            }
        } else {
            settings["hooks"][event_name] = serde_json::Value::Array(bobbin_arr.clone());
        }
    }
}

/// Remove bobbin hooks from a settings object.
/// Returns true if any hooks were removed.
pub(super) fn remove_bobbin_hooks(settings: &mut serde_json::Value) -> bool {
    let mut removed = false;
    if let Some(hooks) = settings.get_mut("hooks").and_then(|h| h.as_object_mut()) {
        for (_event, entries) in hooks.iter_mut() {
            if let Some(arr) = entries.as_array_mut() {
                let before = arr.len();
                arr.retain(|entry| !is_bobbin_hook_group(entry));
                if arr.len() < before {
                    removed = true;
                }
            }
        }
        // Clean up empty event arrays
        hooks.retain(|_, v| {
            v.as_array().map(|a| !a.is_empty()).unwrap_or(true)
        });
    }
    // Remove empty hooks object
    if let Some(hooks) = settings.get("hooks").and_then(|h| h.as_object()) {
        if hooks.is_empty() {
            settings.as_object_mut().unwrap().remove("hooks");
        }
    }
    removed
}

/// Check whether bobbin hooks are present in a settings.json Value.
pub(super) fn has_bobbin_hooks(settings: &serde_json::Value) -> bool {
    if let Some(hooks) = settings.get("hooks").and_then(|h| h.as_object()) {
        for (_event, entries) in hooks {
            if let Some(arr) = entries.as_array() {
                if arr.iter().any(is_bobbin_hook_group) {
                    return true;
                }
            }
        }
    }
    false
}

/// Read a settings.json file, returning empty object if missing.
pub(crate) fn read_settings(path: &Path) -> Result<serde_json::Value> {
    if path.exists() {
        let content = std::fs::read_to_string(path)
            .with_context(|| format!("Failed to read {}", path.display()))?;
        if content.trim().is_empty() {
            return Ok(json!({}));
        }
        serde_json::from_str(&content)
            .with_context(|| format!("Failed to parse {}", path.display()))
    } else {
        Ok(json!({}))
    }
}

/// Write settings.json, creating parent directories as needed.
pub(crate) fn write_settings(path: &Path, settings: &serde_json::Value) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create directory {}", parent.display()))?;
    }
    let content = serde_json::to_string_pretty(settings)
        .context("Failed to serialize settings")?;
    std::fs::write(path, content)
        .with_context(|| format!("Failed to write {}", path.display()))
}

pub(super) async fn run_install(args: InstallArgs, output: OutputConfig) -> Result<()> {
    let settings_path = resolve_settings_path(args.global)?;

    // Detect server URL to bake into hooks (for multi-agent setups)
    let server_url = output.server.clone().or_else(|| {
        // Check config if available
        if let Some(repo_root) = crate::cli::find_bobbin_root() {
            let config_path = Config::config_path(&repo_root);
            if let Ok(config) = Config::load(&config_path) {
                return config.server.url;
            }
        }
        None
    });

    let mut settings = read_settings(&settings_path)?;
    if server_url.is_some() {
        // Use server-aware hooks
        let bobbin = bobbin_hook_entries_with_server(server_url.as_deref());
        merge_hooks_with(&mut settings, &bobbin);
    } else {
        merge_hooks(&mut settings);
    }
    write_settings(&settings_path, &settings)?;

    if output.json {
        let result = json!({
            "status": "installed",
            "path": settings_path.display().to_string(),
            "global": args.global,
        });
        println!("{}", serde_json::to_string_pretty(&result)?);
    } else if !output.quiet {
        let scope = if args.global { "global" } else { "project" };
        println!(
            "{} Bobbin hooks installed ({})",
            "✓".green(),
            scope.cyan()
        );
        println!("  Location: {}", settings_path.display().to_string().dimmed());
        println!("  UserPromptSubmit:    {}", "inject-context".cyan());
        println!("  SessionStart:        {}", "session-context (compact)".cyan());
        println!("  PostToolUse:         {}", "post-tool-use (Write|Edit|Bash|Grep|Glob)".cyan());
        println!("  PostToolUseFailure:  {}", "post-tool-use-failure".cyan());
        if let Some(ref url) = server_url {
            println!("  Server:              {}", url.cyan());
        }
    }

    Ok(())
}

pub(super) async fn run_uninstall(args: UninstallArgs, output: OutputConfig) -> Result<()> {
    let settings_path = resolve_settings_path(args.global)?;

    if !settings_path.exists() {
        if output.json {
            let result = json!({
                "status": "not_installed",
                "path": settings_path.display().to_string(),
            });
            println!("{}", serde_json::to_string_pretty(&result)?);
        } else if !output.quiet {
            println!("No hooks to remove ({})", settings_path.display());
        }
        return Ok(());
    }

    let mut settings = read_settings(&settings_path)?;
    let removed = remove_bobbin_hooks(&mut settings);
    write_settings(&settings_path, &settings)?;

    if output.json {
        let result = json!({
            "status": if removed { "uninstalled" } else { "not_installed" },
            "path": settings_path.display().to_string(),
        });
        println!("{}", serde_json::to_string_pretty(&result)?);
    } else if !output.quiet {
        if removed {
            println!(
                "{} Bobbin hooks removed from {}",
                "✓".green(),
                settings_path.display()
            );
        } else {
            println!("No bobbin hooks found in {}", settings_path.display());
        }
    }

    Ok(())
}

pub(super) async fn run_status(args: StatusArgs, output: OutputConfig) -> Result<()> {
    let repo_root = args
        .path
        .canonicalize()
        .with_context(|| format!("Invalid path: {}", args.path.display()))?;

    let config = Config::load(&Config::config_path(&repo_root)).unwrap_or_default();

    let hooks_cfg = &config.hooks;

    // Check Claude Code hooks — walk up directory tree like Claude Code does.
    // Check project-local first, then parent directories.
    let (hooks_installed, hooks_found_at) = {
        let mut found = false;
        let mut found_path: Option<PathBuf> = None;
        let mut current = repo_root.clone();
        loop {
            let settings = current.join(".claude").join("settings.json");
            if settings.exists() {
                if read_settings(&settings)
                    .map(|s| has_bobbin_hooks(&s))
                    .unwrap_or(false)
                {
                    found = true;
                    found_path = Some(settings);
                    break;
                }
            }
            if !current.pop() {
                break;
            }
        }
        // Also check global ~/.claude/settings.json
        if !found {
            if let Ok(home) = std::env::var("HOME") {
                let global = PathBuf::from(home).join(".claude").join("settings.json");
                if global.exists() {
                    if read_settings(&global)
                        .map(|s| has_bobbin_hooks(&s))
                        .unwrap_or(false)
                    {
                        found = true;
                        found_path = Some(global);
                    }
                }
            }
        }
        (found, found_path)
    };

    // Check git post-commit hook
    let git_hook_path = repo_root.join(".git").join("hooks").join("post-commit");
    let git_hook_installed = if git_hook_path.exists() {
        std::fs::read_to_string(&git_hook_path)
            .map(|content| content.contains(GIT_HOOK_START_MARKER))
            .unwrap_or(false)
    } else {
        false
    };

    // Load runtime state
    let state = load_hook_state(&repo_root);

    if output.json {
        let status = HookStatusOutput {
            hooks_installed,
            git_hook_installed,
            config: HookConfigOutput {
                threshold: hooks_cfg.threshold,
                budget: hooks_cfg.budget,
                content_mode: hooks_cfg.content_mode.clone(),
                min_prompt_length: hooks_cfg.min_prompt_length,
                gate_threshold: hooks_cfg.gate_threshold,
                dedup_enabled: hooks_cfg.dedup_enabled,
            },
            injection_count: state.injection_count,
            last_injection_time: if state.last_injection_time.is_empty() {
                None
            } else {
                Some(state.last_injection_time.clone())
            },
            last_session_id: if state.last_session_id.is_empty() {
                None
            } else {
                Some(state.last_session_id.clone())
            },
        };
        println!("{}", serde_json::to_string_pretty(&status)?);
    } else if !output.quiet {
        println!("{} Hook configuration", "⚡".bold());
        println!();
        println!("  Threshold:        {}", hooks_cfg.threshold.to_string().cyan());
        println!("  Budget:           {} lines", hooks_cfg.budget.to_string().cyan());
        println!("  Content mode:     {}", hooks_cfg.content_mode.cyan());
        println!("  Min prompt len:   {}", hooks_cfg.min_prompt_length.to_string().cyan());
        println!("  Gate threshold:   {}", hooks_cfg.gate_threshold.to_string().cyan());
        println!("  Dedup enabled:    {}", if hooks_cfg.dedup_enabled { "yes".green() } else { "no".yellow() });
        println!();
        let hooks_str = if hooks_installed {
            if let Some(ref p) = hooks_found_at {
                if p.starts_with(&repo_root) {
                    "installed".green()
                } else {
                    format!("installed (via {})", p.display()).green()
                }
            } else {
                "installed".green()
            }
        } else {
            "not installed".yellow()
        };
        let git_str = if git_hook_installed { "installed".green() } else { "not installed".yellow() };
        println!("  Claude Code hooks: {}", hooks_str);
        println!("  Git post-commit:   {}", git_str);
        println!();
        println!("{} Injection stats", "📊".bold());
        println!();
        println!("  Injection count:  {}", state.injection_count.to_string().cyan());
        if !state.last_injection_time.is_empty() {
            println!("  Last injected:    {}", state.last_injection_time.cyan());
        } else {
            println!("  Last injected:    {}", "never".dimmed());
        }
        if !state.last_session_id.is_empty() {
            println!("  Session ID:       {}", state.last_session_id.dimmed());
        }
    }

    Ok(())
}
