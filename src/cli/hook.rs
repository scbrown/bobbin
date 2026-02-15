use anyhow::{Context, Result};
use clap::{Args, Subcommand};
use colored::Colorize;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::{HashMap, HashSet};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use super::OutputConfig;
use crate::config::Config;
use crate::storage::{MetadataStore, VectorStore};

#[derive(Args)]
pub struct HookArgs {
    #[command(subcommand)]
    command: HookCommands,
}

#[derive(Subcommand)]
enum HookCommands {
    /// Install Claude Code hooks into settings.json
    Install(InstallArgs),

    /// Remove bobbin hooks from Claude Code settings
    Uninstall(UninstallArgs),

    /// Show installed hooks and current config values
    Status(StatusArgs),

    /// Handle UserPromptSubmit events (internal, called by Claude Code)
    InjectContext(InjectContextArgs),

    /// Handle SessionStart compact events (internal, called by Claude Code)
    SessionContext(SessionContextArgs),

    /// Output bobbin primer + live stats at SessionStart (internal, called by Claude Code)
    PrimeContext(PrimeContextArgs),

    /// Install a post-commit git hook for automatic indexing
    InstallGitHook(InstallGitHookArgs),

    /// Remove the bobbin post-commit git hook
    UninstallGitHook(UninstallGitHookArgs),

    /// Generate hot-topics.md from injection frequency data
    HotTopics(HotTopicsArgs),
}

#[derive(Args)]
struct InstallArgs {
    /// Install globally (~/.claude/settings.json) instead of project-local
    #[arg(long)]
    global: bool,

    /// Minimum relevance score to include in injected context
    #[arg(long)]
    threshold: Option<f32>,

    /// Maximum lines of injected context
    #[arg(long)]
    budget: Option<usize>,
}

#[derive(Args)]
struct UninstallArgs {
    /// Uninstall from global settings instead of project-local
    #[arg(long)]
    global: bool,
}

#[derive(Args)]
struct StatusArgs {
    /// Directory to check (defaults to current directory)
    #[arg(default_value = ".")]
    path: PathBuf,
}

#[derive(Args)]
struct InjectContextArgs {
    /// Minimum relevance score (overrides config)
    #[arg(long)]
    threshold: Option<f32>,

    /// Maximum lines of context (overrides config)
    #[arg(long)]
    budget: Option<usize>,

    /// Content display mode: full, preview, or none (overrides config)
    #[arg(long)]
    content_mode: Option<String>,

    /// Minimum prompt length to trigger injection (overrides config)
    #[arg(long)]
    min_prompt_length: Option<usize>,

    /// Minimum raw semantic similarity to inject context at all (overrides config)
    #[arg(long)]
    gate_threshold: Option<f32>,

    /// Force injection even if results match previous session (disables dedup)
    #[arg(long)]
    no_dedup: bool,

    /// Include documentation files in injection output (overrides config)
    #[arg(long)]
    show_docs: Option<bool>,
}

#[derive(Args)]
struct SessionContextArgs {
    /// Maximum lines of context (overrides config)
    #[arg(long)]
    budget: Option<usize>,
}

#[derive(Args)]
struct PrimeContextArgs {}

#[derive(Args)]
struct InstallGitHookArgs {}

#[derive(Args)]
struct UninstallGitHookArgs {}

#[derive(Args)]
struct HotTopicsArgs {
    /// Regenerate even if injection count hasn't reached threshold
    #[arg(long)]
    force: bool,

    /// Directory to operate on (defaults to current directory)
    #[arg(default_value = ".")]
    path: PathBuf,
}

#[derive(Serialize)]
struct HookStatusOutput {
    hooks_installed: bool,
    git_hook_installed: bool,
    config: HookConfigOutput,
    injection_count: u64,
    last_injection_time: Option<String>,
    last_session_id: Option<String>,
}

#[derive(Serialize)]
struct HookConfigOutput {
    threshold: f32,
    budget: usize,
    content_mode: String,
    min_prompt_length: usize,
    gate_threshold: f32,
    dedup_enabled: bool,
}

pub async fn run(args: HookArgs, output: OutputConfig) -> Result<()> {
    match args.command {
        HookCommands::Install(a) => run_install(a, output).await,
        HookCommands::Uninstall(a) => run_uninstall(a, output).await,
        HookCommands::Status(a) => run_status(a, output).await,
        HookCommands::InjectContext(a) => run_inject_context(a, output).await,
        HookCommands::SessionContext(a) => run_session_context(a, output).await,
        HookCommands::PrimeContext(a) => run_prime_context(a, output).await,
        HookCommands::InstallGitHook(a) => run_install_git_hook(a, output).await,
        HookCommands::UninstallGitHook(a) => run_uninstall_git_hook(a, output).await,
        HookCommands::HotTopics(a) => run_hot_topics(a, output).await,
    }
}

/// Resolve the target settings.json path.
/// --global → ~/.claude/settings.json
/// otherwise → <git-root>/.claude/settings.json
fn resolve_settings_path(global: bool) -> Result<PathBuf> {
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
fn bobbin_hook_entries() -> serde_json::Value {
    json!({
        "hooks": {
            "UserPromptSubmit": [
                {
                    "hooks": [
                        {
                            "type": "command",
                            "command": "bobbin hook inject-context",
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
                            "command": "bobbin hook session-context",
                            "timeout": 10,
                            "statusMessage": "Recovering project context..."
                        }
                    ]
                }
            ]
        }
    })
}

/// Check if a hook group entry contains a bobbin command.
fn is_bobbin_hook_group(group: &serde_json::Value) -> bool {
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
fn merge_hooks(settings: &mut serde_json::Value) {
    let bobbin = bobbin_hook_entries();
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
fn remove_bobbin_hooks(settings: &mut serde_json::Value) -> bool {
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
fn has_bobbin_hooks(settings: &serde_json::Value) -> bool {
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
fn read_settings(path: &Path) -> Result<serde_json::Value> {
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
fn write_settings(path: &Path, settings: &serde_json::Value) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create directory {}", parent.display()))?;
    }
    let content = serde_json::to_string_pretty(settings)
        .context("Failed to serialize settings")?;
    std::fs::write(path, content)
        .with_context(|| format!("Failed to write {}", path.display()))
}

async fn run_install(args: InstallArgs, output: OutputConfig) -> Result<()> {
    let settings_path = resolve_settings_path(args.global)?;

    let mut settings = read_settings(&settings_path)?;
    merge_hooks(&mut settings);
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
        println!("  UserPromptSubmit: {}", "inject-context".cyan());
        println!("  SessionStart:     {}", "session-context (compact)".cyan());
    }

    Ok(())
}

async fn run_uninstall(args: UninstallArgs, output: OutputConfig) -> Result<()> {
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

async fn run_status(args: StatusArgs, output: OutputConfig) -> Result<()> {
    let repo_root = args
        .path
        .canonicalize()
        .with_context(|| format!("Invalid path: {}", args.path.display()))?;

    let config = Config::load(&Config::config_path(&repo_root)).unwrap_or_default();

    let hooks_cfg = &config.hooks;

    // Check Claude Code hooks (project-local)
    let project_settings = repo_root.join(".claude").join("settings.json");
    let hooks_installed = if project_settings.exists() {
        read_settings(&project_settings)
            .map(|s| has_bobbin_hooks(&s))
            .unwrap_or(false)
    } else {
        false
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
        let hooks_str = if hooks_installed { "installed".green() } else { "not installed".yellow() };
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

async fn run_inject_context(args: InjectContextArgs, _output: OutputConfig) -> Result<()> {
    // Never block user prompts — any error exits silently
    match inject_context_inner(args).await {
        Ok(()) => Ok(()),
        Err(e) => {
            eprintln!("bobbin inject-context: {:#}", e);
            Ok(())
        }
    }
}

/// Claude Code UserPromptSubmit hook input (subset of fields we need)
#[derive(Deserialize)]
struct HookInput {
    /// The user's prompt text
    #[serde(default)]
    prompt: String,
    /// Working directory when the hook was invoked
    #[serde(default)]
    cwd: String,
    /// Claude Code session ID (used as metrics source identity)
    #[serde(default)]
    session_id: String,
}

/// Walk up from `start` looking for a directory containing `.bobbin/config.toml`.
fn find_bobbin_root(start: &Path) -> Option<PathBuf> {
    let mut dir = start.to_path_buf();
    loop {
        if Config::config_path(&dir).exists() {
            return Some(dir);
        }
        if !dir.pop() {
            return None;
        }
    }
}

/// Format a context bundle into a compact text block for Claude Code injection.
///
/// Produces a plain-text summary of relevant code chunks, enforcing a hard line
/// budget on the output. The `threshold` filters out low-scoring chunks. The
/// output budget is taken from `bundle.budget.max_lines`.
fn format_context_for_injection(
    bundle: &crate::search::context::ContextBundle,
    threshold: f32,
    show_docs: bool,
) -> String {
    use crate::types::FileCategory;
    use std::fmt::Write;

    let budget = bundle.budget.max_lines;
    let mut out = String::new();

    let header = format!(
        "Bobbin found {} relevant files ({} source, {} docs, {}/{} budget lines):",
        bundle.summary.total_files,
        bundle.summary.source_files,
        bundle.summary.doc_files,
        bundle.budget.used_lines,
        bundle.budget.max_lines,
    );
    out.push_str(&header);
    out.push('\n');

    // Partition files: source/test first, then docs/config
    let source_files: Vec<_> = bundle.files.iter()
        .filter(|f| f.category == FileCategory::Source || f.category == FileCategory::Test)
        .collect();
    let doc_files: Vec<_> = bundle.files.iter()
        .filter(|f| f.category == FileCategory::Documentation || f.category == FileCategory::Config)
        .collect();

    // Emit source files section
    if !source_files.is_empty() {
        let _ = write!(out, "\n=== Source Files ===\n");
        format_file_chunks(&mut out, &source_files, threshold, budget);
    }

    // Emit documentation section (if show_docs is true)
    if show_docs && !doc_files.is_empty() {
        let _ = write!(out, "\n=== Documentation ===\n");
        format_file_chunks(&mut out, &doc_files, threshold, budget);
    }

    // Final enforcement: trim to budget
    let lines: Vec<&str> = out.lines().collect();
    if lines.len() > budget {
        lines[..budget].join("\n") + "\n"
    } else {
        out
    }
}

/// Format chunks from a list of files into the output string, respecting budget.
fn format_file_chunks(
    out: &mut String,
    files: &[&crate::search::context::ContextFile],
    threshold: f32,
    budget: usize,
) {
    use std::fmt::Write;

    // Track line count incrementally to avoid O(n²) recounting
    let mut current_lines = out.lines().count();

    for file in files {
        for chunk in &file.chunks {
            if chunk.score < threshold {
                continue;
            }
            let name = chunk
                .name
                .as_ref()
                .map(|n| format!(" {}", n))
                .unwrap_or_default();
            let chunk_section = if let Some(ref content) = chunk.content {
                format!(
                    "\n--- {}:{}-{}{} ({}, score {:.2}) ---\n{}{}",
                    file.path,
                    chunk.start_line,
                    chunk.end_line,
                    name,
                    chunk.chunk_type,
                    chunk.score,
                    content,
                    if content.ends_with('\n') { "" } else { "\n" },
                )
            } else {
                format!(
                    "\n--- {}:{}-{}{} ({}, score {:.2}) ---\n",
                    file.path, chunk.start_line, chunk.end_line, name, chunk.chunk_type, chunk.score,
                )
            };

            // Check if adding this chunk would exceed budget
            let chunk_line_count = chunk_section.lines().count();
            if current_lines + chunk_line_count > budget {
                return;
            }
            current_lines += chunk_line_count;
            let _ = write!(out, "{}", chunk_section);
        }
    }
}

/// Persistent state for hook dedup and frequency tracking.
/// Stored in `.bobbin/hook_state.json`.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct HookState {
    #[serde(default)]
    last_session_id: String,
    #[serde(default)]
    last_injected_chunks: Vec<String>,
    #[serde(default)]
    last_injection_time: String,
    #[serde(default)]
    injection_count: u64,
    #[serde(default)]
    chunk_frequencies: HashMap<String, ChunkFrequency>,
    #[serde(default)]
    file_frequencies: HashMap<String, u64>,
    #[serde(default)]
    hot_topics_generated_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ChunkFrequency {
    count: u64,
    file: String,
    name: Option<String>,
}

fn hook_state_path(repo_root: &Path) -> PathBuf {
    repo_root.join(".bobbin").join("hook_state.json")
}

/// Load hook state from disk. Returns default on any error.
fn load_hook_state(repo_root: &Path) -> HookState {
    let path = hook_state_path(repo_root);
    match std::fs::read_to_string(&path) {
        Ok(content) => serde_json::from_str(&content).unwrap_or_default(),
        Err(_) => HookState::default(),
    }
}

/// Save hook state to disk. Errors are swallowed (never block prompts).
fn save_hook_state(repo_root: &Path, state: &HookState) {
    let path = hook_state_path(repo_root);
    if let Ok(json) = serde_json::to_string_pretty(state) {
        let _ = std::fs::write(&path, json);
    }
}

/// Compute a session ID from the context bundle's chunks.
///
/// Takes the chunk composite keys (file:start:end), filters by threshold,
/// sorts alphabetically, takes top 10, concatenates with `|`, and returns
/// the first 16 hex chars of the SHA-256 hash.
fn compute_session_id(bundle: &crate::search::context::ContextBundle, threshold: f32) -> String {
    use sha2::{Digest, Sha256};

    let mut keys: Vec<String> = bundle
        .files
        .iter()
        .flat_map(|f| {
            f.chunks
                .iter()
                .filter(|c| c.score >= threshold)
                .map(move |c| format!("{}:{}:{}", f.path, c.start_line, c.end_line))
        })
        .collect();

    keys.sort();
    keys.truncate(10);

    let joined = keys.join("|");
    let hash = Sha256::digest(joined.as_bytes());
    hex::encode(&hash[..8]) // 8 bytes = 16 hex chars
}

/// Generate `.bobbin/hot-topics.md` from injection frequency data.
fn generate_hot_topics(state: &HookState, output_path: &Path) -> Result<()> {
    use std::fmt::Write;

    let mut md = String::new();
    writeln!(md, "# Hot Topics (auto-generated by bobbin)").unwrap();
    writeln!(md).unwrap();

    let timestamp = if state.last_injection_time.is_empty() {
        "never".to_string()
    } else {
        // Truncate to minute precision for readability
        state
            .last_injection_time
            .get(..16)
            .unwrap_or(&state.last_injection_time)
            .replace('T', " ")
            + " UTC"
    };
    writeln!(md, "Last updated: {}", timestamp).unwrap();
    writeln!(
        md,
        "Based on {} context injections.",
        state.injection_count
    )
    .unwrap();
    writeln!(md).unwrap();

    // Frequently referenced chunks, sorted by count descending
    let mut chunks: Vec<(&String, &ChunkFrequency)> = state.chunk_frequencies.iter().collect();
    chunks.sort_by(|a, b| b.1.count.cmp(&a.1.count));
    chunks.truncate(20);

    writeln!(md, "## Frequently Referenced Code").unwrap();
    writeln!(md).unwrap();
    if chunks.is_empty() {
        writeln!(md, "No injection data yet.").unwrap();
    } else {
        writeln!(md, "| Rank | File | Symbol | Injections |").unwrap();
        writeln!(md, "|------|------|--------|------------|").unwrap();
        for (i, (_key, freq)) in chunks.iter().enumerate() {
            let symbol = freq.name.as_deref().unwrap_or("-");
            writeln!(
                md,
                "| {} | {} | {} | {} |",
                i + 1,
                freq.file,
                symbol,
                freq.count
            )
            .unwrap();
        }
    }
    writeln!(md).unwrap();

    // Most referenced files, sorted by count descending
    let mut files: Vec<(&String, &u64)> = state.file_frequencies.iter().collect();
    files.sort_by(|a, b| b.1.cmp(a.1));
    files.truncate(10);

    writeln!(md, "## Most Referenced Files").unwrap();
    writeln!(md).unwrap();
    if files.is_empty() {
        writeln!(md, "No injection data yet.").unwrap();
    } else {
        writeln!(md, "| File | Total Injections |").unwrap();
        writeln!(md, "|------|-----------------|").unwrap();
        for (file, count) in &files {
            writeln!(md, "| {} | {} |", file, count).unwrap();
        }
    }
    writeln!(md).unwrap();

    writeln!(md, "## Notes").unwrap();
    writeln!(md).unwrap();
    writeln!(
        md,
        "- Chunks appearing here are candidates for pinning in CLAUDE.md or session context."
    )
    .unwrap();
    writeln!(
        md,
        "- Regenerated every 10 injections. Run `bobbin hook hot-topics` to force refresh."
    )
    .unwrap();

    std::fs::write(output_path, md).context("Failed to write hot-topics.md")?;
    Ok(())
}

async fn run_hot_topics(args: HotTopicsArgs, _output: OutputConfig) -> Result<()> {
    let cwd = if args.path == Path::new(".") {
        std::env::current_dir().context("Failed to get cwd")?
    } else {
        args.path.clone()
    };

    let repo_root = find_bobbin_root(&cwd).context("Bobbin not initialized in this directory")?;
    let state = load_hook_state(&repo_root);

    if !args.force && state.injection_count == 0 {
        println!("No injection data yet. Run with --force to generate anyway.");
        return Ok(());
    }

    let output_path = repo_root.join(".bobbin").join("hot-topics.md");
    generate_hot_topics(&state, &output_path)?;
    println!(
        "Generated {} ({} injections, {} chunks tracked)",
        output_path.display(),
        state.injection_count,
        state.chunk_frequencies.len()
    );
    Ok(())
}

/// Inner implementation that can return errors (caller swallows them).
async fn inject_context_inner(args: InjectContextArgs) -> Result<()> {
    use crate::index::Embedder;
    use crate::search::context::{ContentMode, ContextAssembler, ContextConfig};
    use crate::storage::{MetadataStore, VectorStore};

    let hook_start = std::time::Instant::now();

    // 1. Read stdin JSON
    let input: HookInput = serde_json::from_reader(std::io::stdin().lock())
        .context("Failed to parse stdin JSON")?;

    // 2. Resolve effective config
    let cwd = if input.cwd.is_empty() {
        std::env::current_dir().context("Failed to get cwd")?
    } else {
        PathBuf::from(&input.cwd)
    };

    let repo_root = find_bobbin_root(&cwd).context("Bobbin not initialized")?;
    let metrics_source = crate::metrics::resolve_source(
        None, // no CLI flag in hook context
        if input.session_id.is_empty() { None } else { Some(&input.session_id) },
    );
    let config = Config::load(&Config::config_path(&repo_root))
        .context("Failed to load bobbin config")?;
    let hooks_cfg = &config.hooks;

    // Apply CLI overrides
    let min_prompt_length = args.min_prompt_length.unwrap_or(hooks_cfg.min_prompt_length);
    let threshold = args.threshold.unwrap_or(hooks_cfg.threshold);
    let budget = args.budget.unwrap_or(hooks_cfg.budget);
    let content_mode_str = args
        .content_mode
        .as_deref()
        .unwrap_or(&hooks_cfg.content_mode);
    let content_mode = match content_mode_str {
        "full" => ContentMode::Full,
        "none" => ContentMode::None,
        _ => ContentMode::Preview,
    };

    // 3. Check min prompt length
    let prompt = input.prompt.trim();
    if prompt.len() < min_prompt_length {
        return Ok(());
    }

    // 4. Open stores
    let lance_path = Config::lance_path(&repo_root);
    let db_path = Config::db_path(&repo_root);
    let model_dir = Config::model_cache_dir()?;

    let vector_store = VectorStore::open(&lance_path)
        .await
        .context("Failed to open vector store")?;

    if vector_store.count().await? == 0 {
        return Ok(());
    }

    let metadata_store =
        MetadataStore::open(&db_path).context("Failed to open metadata store")?;

    // 5. Check model consistency
    let current_model = config.embedding.model.as_str();
    if let Some(stored) = metadata_store.get_meta("embedding_model")? {
        if stored != current_model {
            return Ok(()); // Model mismatch — skip silently
        }
    }

    let embedder = Embedder::from_config(&config.embedding, &model_dir)
        .context("Failed to load embedding model")?;

    // 6. Assemble context
    let context_config = ContextConfig {
        budget_lines: budget,
        depth: 1,
        max_coupled: 3,
        coupling_threshold: 0.1,
        semantic_weight: config.search.semantic_weight,
        content_mode,
        search_limit: 20,
    };

    let mut assembler = ContextAssembler::new(embedder, vector_store, metadata_store, context_config);
    if let Ok(git) = crate::index::git::GitAnalyzer::new(&repo_root) {
        assembler = assembler.with_git_analyzer(git);
    }
    let bundle = assembler
        .assemble(prompt, None)
        .await
        .context("Context assembly failed")?;

    // 7. Gate check: skip entire injection if top semantic score is too low
    let gate = args.gate_threshold.unwrap_or(hooks_cfg.gate_threshold);
    if bundle.summary.top_semantic_score < gate {
        eprintln!(
            "bobbin: skipped (semantic={:.2} < gate={:.2})",
            bundle.summary.top_semantic_score, gate
        );
        crate::metrics::emit(&repo_root, &crate::metrics::event(
            &metrics_source,
            "hook_gate_skip",
            "hook inject-context",
            hook_start.elapsed().as_millis() as u64,
            serde_json::json!({
                "query": prompt,
                "top_score": bundle.summary.top_semantic_score,
                "gate_threshold": gate,
            }),
        ));
        return Ok(());
    }

    // 8. Session dedup: skip if results haven't changed
    let dedup_enabled = !args.no_dedup && hooks_cfg.dedup_enabled;
    let dedup_session_id = compute_session_id(&bundle, threshold);
    let mut state = if dedup_enabled {
        let s = load_hook_state(&repo_root);
        if s.last_session_id == dedup_session_id && !dedup_session_id.is_empty() {
            eprintln!("bobbin: skipped (session unchanged)");
            crate::metrics::emit(&repo_root, &crate::metrics::event(
                &metrics_source,
                "hook_dedup_skip",
                "hook inject-context",
                hook_start.elapsed().as_millis() as u64,
                serde_json::json!({ "query": prompt }),
            ));
            return Ok(());
        }
        s
    } else {
        load_hook_state(&repo_root)
    };

    // 9. Output context (only if we found something)
    if bundle.files.is_empty() {
        return Ok(());
    }

    let show_docs = args.show_docs.unwrap_or(hooks_cfg.show_docs);
    let context_text = format_context_for_injection(&bundle, threshold, show_docs);
    print!("{}", context_text);

    // 10. Update hook state
    let chunk_keys: Vec<String> = bundle
        .files
        .iter()
        .flat_map(|f| {
            f.chunks
                .iter()
                .filter(|c| c.score >= threshold)
                .map(move |c| (f.path.clone(), c))
        })
        .map(|(path, c)| {
            let key = format!("{}:{}:{}", path, c.start_line, c.end_line);
            let freq = state.chunk_frequencies.entry(key.clone()).or_insert(ChunkFrequency {
                count: 0,
                file: path.clone(),
                name: c.name.clone(),
            });
            freq.count += 1;
            *state.file_frequencies.entry(path).or_insert(0) += 1;
            key
        })
        .collect();

    state.last_session_id = dedup_session_id;
    state.last_injected_chunks = chunk_keys;
    state.last_injection_time = chrono::Utc::now().to_rfc3339();
    state.injection_count += 1;
    save_hook_state(&repo_root, &state);

    // 10b. Emit hook_injection metric
    let injected_files: Vec<&str> = bundle.files.iter().map(|f| f.path.as_str()).collect();
    crate::metrics::emit(&repo_root, &crate::metrics::event(
        &metrics_source,
        "hook_injection",
        "hook inject-context",
        hook_start.elapsed().as_millis() as u64,
        serde_json::json!({
            "query": prompt,
            "files_returned": injected_files,
            "chunks_returned": bundle.summary.total_chunks,
            "top_score": bundle.summary.top_semantic_score,
            "budget_lines_used": bundle.budget.used_lines,
            "source_files": bundle.summary.source_files,
            "doc_files": bundle.summary.doc_files,
            "bridged_additions": bundle.summary.bridged_additions,
        }),
    ));

    // 11. Auto-generate hot topics every 10 injections
    if state.injection_count % 10 == 0
        && state.injection_count > state.hot_topics_generated_at
    {
        let hot_topics_path = repo_root.join(".bobbin").join("hot-topics.md");
        if generate_hot_topics(&state, &hot_topics_path).is_ok() {
            // Update the generation marker (re-load to avoid stale writes)
            let mut updated = load_hook_state(&repo_root);
            updated.hot_topics_generated_at = state.injection_count;
            save_hook_state(&repo_root, &updated);
            eprintln!("bobbin: regenerated hot-topics.md ({} injections)", state.injection_count);
        }
    }

    Ok(())
}

async fn run_prime_context(_args: PrimeContextArgs, _output: OutputConfig) -> Result<()> {
    // Never block session start — swallow all errors
    match run_prime_context_inner().await {
        Ok(()) => Ok(()),
        Err(e) => {
            eprintln!("bobbin prime-context: {}", e);
            Ok(())
        }
    }
}

async fn run_prime_context_inner() -> Result<()> {
    let hook_start = std::time::Instant::now();

    // 1. Read stdin JSON (may be empty for some Claude Code versions)
    let input_str = std::io::read_to_string(std::io::stdin())
        .context("Failed to read stdin")?;

    let session_id = if input_str.trim().is_empty() {
        String::new()
    } else {
        let input: SessionStartInput =
            serde_json::from_str(&input_str).unwrap_or(SessionStartInput {
                source: String::new(),
                cwd: String::new(),
                session_id: String::new(),
            });
        input.session_id
    };

    // 2. Find repo root
    let cwd = std::env::current_dir().context("Failed to get cwd")?;
    let repo_root = find_bobbin_root(&cwd).context("Bobbin not initialized")?;

    let metrics_source = crate::metrics::resolve_source(
        None,
        if session_id.is_empty() { None } else { Some(&session_id) },
    );

    // 3. Build primer text (brief version + live stats)
    let primer = include_str!("../../docs/primer.md");
    let brief = extract_brief(primer);

    // 4. Gather live stats
    let lance_path = Config::lance_path(&repo_root);
    let stats_text = if let Ok(store) = VectorStore::open(&lance_path).await {
        if let Ok(stats) = store.get_stats(None).await {
            let mut lines = vec![
                format!("- {} files, {} chunks indexed", stats.total_files, stats.total_chunks),
            ];
            if !stats.languages.is_empty() {
                let langs: Vec<String> = stats.languages.iter()
                    .map(|l| format!("{} ({} files)", l.language, l.file_count))
                    .collect();
                lines.push(format!("- Languages: {}", langs.join(", ")));
            }
            lines.join("\n")
        } else {
            "- Index stats unavailable".to_string()
        }
    } else {
        "- Vector store not accessible".to_string()
    };

    // 5. Compose output
    let context = format!(
        "{}\n\n## Index Status\n{}\n\n## Available Commands\n\
        - `bobbin search <query>` — semantic + keyword hybrid search\n\
        - `bobbin context <query>` — task-aware context assembly with budget control\n\
        - `bobbin grep <pattern>` — keyword/regex search\n\
        - `bobbin related <file>` — find co-changing files via git coupling\n\
        - `bobbin refs <symbol>` — find symbol definitions and references\n\
        - `bobbin impact <file>` — predict affected files\n\
        - `bobbin hotspots` — high-churn, high-complexity files",
        brief, stats_text,
    );

    // 6. Output hook response JSON
    let response = HookResponse {
        hook_specific_output: HookSpecificOutput {
            hook_event_name: "SessionStart".to_string(),
            additional_context: context,
        },
    };

    println!("{}", serde_json::to_string(&response)?);

    // 7. Emit metric
    crate::metrics::emit(&repo_root, &crate::metrics::event(
        &metrics_source,
        "hook_prime_context",
        "hook prime-context",
        hook_start.elapsed().as_millis() as u64,
        serde_json::Value::Null,
    ));

    Ok(())
}

/// Extract brief primer text (title + first section only).
fn extract_brief(primer: &str) -> String {
    let mut result = String::new();
    let mut heading_count = 0;
    for line in primer.lines() {
        if line.starts_with("## ") {
            heading_count += 1;
            if heading_count > 1 {
                break;
            }
        }
        result.push_str(line);
        result.push('\n');
    }
    result.trim_end().to_string()
}

async fn run_session_context(args: SessionContextArgs, _output: OutputConfig) -> Result<()> {
    // Never block session start — wrap everything in a catch-all
    match run_session_context_inner(args).await {
        Ok(()) => Ok(()),
        Err(e) => {
            eprintln!("bobbin session-context: {}", e);
            Ok(())
        }
    }
}

/// Input JSON from Claude Code SessionStart hook
#[derive(Deserialize)]
struct SessionStartInput {
    #[serde(default)]
    source: String,
    #[serde(default)]
    cwd: String,
    /// Claude Code session ID (used as metrics source identity)
    #[serde(default)]
    session_id: String,
}

/// Output JSON for Claude Code hook response
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct HookResponse {
    hook_specific_output: HookSpecificOutput,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct HookSpecificOutput {
    hook_event_name: String,
    additional_context: String,
}

/// A file with its symbols for display
struct FileSymbolInfo {
    path: String,
    symbols: Vec<SymbolInfo>,
}

struct SymbolInfo {
    name: String,
}

async fn run_session_context_inner(args: SessionContextArgs) -> Result<()> {
    // 1. Read stdin JSON
    let input_str = std::io::read_to_string(std::io::stdin())
        .context("Failed to read stdin")?;

    // If stdin is empty, nothing to do
    if input_str.trim().is_empty() {
        return Ok(());
    }

    let input: SessionStartInput =
        serde_json::from_str(&input_str).context("Failed to parse stdin JSON")?;

    // 2. Only handle compact events
    if input.source != "compact" {
        return Ok(());
    }

    // 3. Determine repo root
    let cwd = if input.cwd.is_empty() {
        std::env::current_dir().context("Failed to get cwd")?
    } else {
        PathBuf::from(&input.cwd)
            .canonicalize()
            .context("Invalid cwd path")?
    };

    // Load config (use defaults if not initialized)
    let config = Config::load(&Config::config_path(&cwd)).unwrap_or_default();
    let budget = args.budget.unwrap_or(config.hooks.budget);

    // 4. Gather git signals
    let modified_files = git_status_files(&cwd)?;
    let recent_commits = git_recent_commits(&cwd, 5)?;
    let recently_changed_files = git_recently_changed_files(&cwd, 3)?;

    // If there's nothing to report, exit silently
    if modified_files.is_empty() && recent_commits.is_empty() && recently_changed_files.is_empty() {
        return Ok(());
    }

    // 5. Collect all interesting file paths (deduped)
    let mut all_files: HashSet<String> = HashSet::new();
    for f in &modified_files {
        all_files.insert(f.clone());
    }
    for f in &recently_changed_files {
        all_files.insert(f.clone());
    }

    // 6. Query bobbin for symbols and coupling (best-effort)
    let mut file_symbols: Vec<FileSymbolInfo> = Vec::new();
    let mut coupled_files: Vec<(String, String, f32)> = Vec::new(); // (path, coupled_to, score)

    let lance_path = Config::lance_path(&cwd);
    let db_path = Config::db_path(&cwd);

    if lance_path.exists() && db_path.exists() {
        let vector_store = match VectorStore::open(&lance_path).await {
            Ok(vs) => Some(vs),
            Err(e) => {
                eprintln!("bobbin: vector store unavailable: {}", e);
                None
            }
        };
        let metadata_store = match MetadataStore::open(&db_path) {
            Ok(ms) => Some(ms),
            Err(e) => {
                eprintln!("bobbin: metadata store unavailable: {}", e);
                None
            }
        };

        // Get symbols for each file
        if let Some(ref vs) = vector_store {
            for file_path in &all_files {
                if let Ok(chunks) = vs.get_chunks_for_file(file_path, None).await {
                    let symbols: Vec<SymbolInfo> = chunks
                        .iter()
                        .filter(|c| c.name.is_some())
                        .map(|c| SymbolInfo {
                            name: c.name.clone().unwrap_or_default(),
                        })
                        .collect();
                    if !symbols.is_empty() {
                        file_symbols.push(FileSymbolInfo {
                            path: file_path.clone(),
                            symbols,
                        });
                    }
                }
            }
        }

        // Get coupled files
        if let Some(ref ms) = metadata_store {
            let mut seen_coupled: HashSet<String> = HashSet::new();
            for file_path in &all_files {
                if let Ok(couplings) = ms.get_coupling(file_path, 3) {
                    for c in couplings {
                        let other = if c.file_a == *file_path {
                            &c.file_b
                        } else {
                            &c.file_a
                        };
                        if !all_files.contains(other) && !seen_coupled.contains(other) {
                            seen_coupled.insert(other.clone());
                            coupled_files.push((
                                other.clone(),
                                file_path.clone(),
                                c.score,
                            ));
                        }
                    }
                }
            }
        }
    }

    // Sort file symbols by path for stable output
    file_symbols.sort_by(|a, b| a.path.cmp(&b.path));
    coupled_files.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap_or(std::cmp::Ordering::Equal));

    // 7. Format compact markdown within budget
    let context_md = format_session_context(
        &modified_files,
        &recent_commits,
        &file_symbols,
        &coupled_files,
        budget,
    );

    if context_md.is_empty() {
        return Ok(());
    }

    // 8. Output hook response JSON
    let response = HookResponse {
        hook_specific_output: HookSpecificOutput {
            hook_event_name: "SessionStart".to_string(),
            additional_context: context_md,
        },
    };

    println!("{}", serde_json::to_string(&response)?);
    Ok(())
}

/// Get modified/staged/untracked files from git status
fn git_status_files(cwd: &std::path::Path) -> Result<Vec<String>> {
    let output = Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(cwd)
        .output()
        .context("Failed to run git status")?;

    if !output.status.success() {
        return Ok(Vec::new());
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let files: Vec<String> = stdout
        .lines()
        .filter_map(|line| {
            // git status --porcelain format: "XY path" where XY is a 2-char
            // status code at fixed positions 0-1, followed by a space at
            // position 2, then the path.  Do NOT trim the line first — the
            // leading space in " M" is part of the status code.
            if line.len() > 3 {
                Some(line[3..].to_string())
            } else {
                None
            }
        })
        .collect();

    Ok(files)
}

/// Get recent commit summaries
fn git_recent_commits(cwd: &std::path::Path, count: usize) -> Result<Vec<String>> {
    let output = Command::new("git")
        .args(["log", "--oneline", &format!("-{}", count)])
        .current_dir(cwd)
        .output()
        .context("Failed to run git log")?;

    if !output.status.success() {
        return Ok(Vec::new());
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    Ok(stdout.lines().map(|l| l.to_string()).collect())
}

/// Get files changed in recent commits
fn git_recently_changed_files(cwd: &std::path::Path, depth: usize) -> Result<Vec<String>> {
    let output = Command::new("git")
        .args([
            "diff",
            "--name-only",
            &format!("HEAD~{}..HEAD", depth),
        ])
        .current_dir(cwd)
        .output()
        .context("Failed to run git diff")?;

    if !output.status.success() {
        // May fail if repo has fewer commits than depth
        return Ok(Vec::new());
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    Ok(stdout
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| l.to_string())
        .collect())
}

/// Format session context as compact markdown for Claude Code SessionStart recovery.
///
/// Produces a markdown summary of working state (modified files, recent commits,
/// coupled files) within the given line budget. Budget is enforced on output lines;
/// if truncation is needed the last line is a notice message, counted within budget.
fn format_session_context(
    modified_files: &[String],
    recent_commits: &[String],
    file_symbols: &[FileSymbolInfo],
    coupled_files: &[(String, String, f32)],
    budget: usize,
) -> String {
    let mut lines: Vec<String> = Vec::new();

    lines.push("## Working Context (recovered after compaction)".to_string());
    lines.push(String::new());

    // Modified files section
    if !modified_files.is_empty() {
        lines.push("### Modified files".to_string());
        for file in modified_files {
            // Find symbols for this file
            let symbols_str = file_symbols
                .iter()
                .find(|fs| fs.path == *file)
                .map(|fs| {
                    let names: Vec<String> = fs
                        .symbols
                        .iter()
                        .take(5)
                        .map(|s| s.name.clone())
                        .collect();
                    if names.is_empty() {
                        String::new()
                    } else {
                        let count = fs.symbols.len();
                        let display = names.join(", ");
                        if count > 5 {
                            format!(" ({} symbols: {}, ...)", count, display)
                        } else {
                            format!(" ({} symbols: {})", count, display)
                        }
                    }
                })
                .unwrap_or_default();
            lines.push(format!("- {}{}", file, symbols_str));
        }
        lines.push(String::new());
    }

    // Recent commits section
    if !recent_commits.is_empty() {
        lines.push("### Recent commits".to_string());
        for commit in recent_commits {
            lines.push(format!("- {}", commit));
        }
        lines.push(String::new());
    }

    // File symbols for non-modified files (recently changed files that aren't modified)
    let modified_set: HashSet<&String> = modified_files.iter().collect();
    let other_symbols: Vec<&FileSymbolInfo> = file_symbols
        .iter()
        .filter(|fs| !modified_set.contains(&fs.path))
        .collect();

    if !other_symbols.is_empty() {
        lines.push("### Recently changed files".to_string());
        for fs in &other_symbols {
            let names: Vec<String> = fs
                .symbols
                .iter()
                .take(5)
                .map(|s| s.name.clone())
                .collect();
            let symbols_str = if names.is_empty() {
                String::new()
            } else {
                let count = fs.symbols.len();
                let display = names.join(", ");
                if count > 5 {
                    format!(" ({} symbols: {}, ...)", count, display)
                } else {
                    format!(" ({} symbols: {})", count, display)
                }
            };
            lines.push(format!("- {}{}", fs.path, symbols_str));
        }
        lines.push(String::new());
    }

    // Coupled files section
    if !coupled_files.is_empty() {
        lines.push("### Related files (via coupling)".to_string());
        for (path, coupled_to, score) in coupled_files.iter().take(5) {
            lines.push(format!(
                "- {} (coupled with {}, score: {:.2})",
                path, coupled_to, score
            ));
        }
        lines.push(String::new());
    }

    // Enforce budget (reserve 1 line for truncation message if needed)
    if lines.len() > budget {
        lines.truncate(budget.saturating_sub(1));
        lines.push("... (truncated to fit budget)".to_string());
    }

    lines.join("\n")
}

const GIT_HOOK_START_MARKER: &str = "# >>> bobbin post-commit hook >>>";
const GIT_HOOK_END_MARKER: &str = "# <<< bobbin post-commit hook <<<";

const GIT_HOOK_SECTION: &str = r#"# >>> bobbin post-commit hook >>>
# Auto-generated by `bobbin hook install-git-hook` — do not edit this section
if command -v bobbin >/dev/null 2>&1; then
  bobbin index --quiet &
fi
# <<< bobbin post-commit hook <<<"#;

/// Find the .git/hooks directory for the current repo.
fn git_hooks_dir() -> Result<PathBuf> {
    let output = Command::new("git")
        .args(["rev-parse", "--git-dir"])
        .output()
        .context("Failed to run git rev-parse")?;
    if !output.status.success() {
        anyhow::bail!("Not in a git repository");
    }
    let git_dir = String::from_utf8_lossy(&output.stdout).trim().to_string();
    Ok(PathBuf::from(git_dir).join("hooks"))
}

async fn run_install_git_hook(_args: InstallGitHookArgs, output: OutputConfig) -> Result<()> {
    let hooks_dir = git_hooks_dir()?;
    let hook_path = hooks_dir.join("post-commit");

    std::fs::create_dir_all(&hooks_dir)
        .with_context(|| format!("Failed to create {}", hooks_dir.display()))?;

    let content = if hook_path.exists() {
        let existing = std::fs::read_to_string(&hook_path)
            .with_context(|| format!("Failed to read {}", hook_path.display()))?;

        if existing.contains(GIT_HOOK_START_MARKER) {
            // Already installed — replace existing section
            let mut result = String::new();
            let mut in_bobbin_section = false;
            for line in existing.lines() {
                if line.contains(GIT_HOOK_START_MARKER) {
                    in_bobbin_section = true;
                    result.push_str(GIT_HOOK_SECTION);
                    result.push('\n');
                } else if line.contains(GIT_HOOK_END_MARKER) {
                    in_bobbin_section = false;
                    // Already included in GIT_HOOK_SECTION above
                } else if !in_bobbin_section {
                    result.push_str(line);
                    result.push('\n');
                }
            }
            result
        } else {
            // Append to existing hook
            let mut result = existing;
            if !result.ends_with('\n') {
                result.push('\n');
            }
            result.push('\n');
            result.push_str(GIT_HOOK_SECTION);
            result.push('\n');
            result
        }
    } else {
        // New hook file
        format!("#!/bin/sh\n\n{}\n", GIT_HOOK_SECTION)
    };

    std::fs::write(&hook_path, &content)
        .with_context(|| format!("Failed to write {}", hook_path.display()))?;

    // Make executable
    let perms = std::fs::Permissions::from_mode(0o755);
    std::fs::set_permissions(&hook_path, perms)
        .with_context(|| format!("Failed to set permissions on {}", hook_path.display()))?;

    if output.json {
        let result = json!({
            "status": "installed",
            "path": hook_path.display().to_string(),
        });
        println!("{}", serde_json::to_string_pretty(&result)?);
    } else if !output.quiet {
        println!(
            "{} Git post-commit hook installed",
            "✓".green(),
        );
        println!("  Location: {}", hook_path.display().to_string().dimmed());
        println!("  Action:   {} after each commit", "bobbin index --quiet".cyan());
    }

    Ok(())
}

async fn run_uninstall_git_hook(_args: UninstallGitHookArgs, output: OutputConfig) -> Result<()> {
    let hooks_dir = git_hooks_dir()?;
    let hook_path = hooks_dir.join("post-commit");

    if !hook_path.exists() {
        if output.json {
            let result = json!({
                "status": "not_installed",
                "path": hook_path.display().to_string(),
            });
            println!("{}", serde_json::to_string_pretty(&result)?);
        } else if !output.quiet {
            println!("No post-commit hook found");
        }
        return Ok(());
    }

    let existing = std::fs::read_to_string(&hook_path)
        .with_context(|| format!("Failed to read {}", hook_path.display()))?;

    if !existing.contains(GIT_HOOK_START_MARKER) {
        if output.json {
            let result = json!({
                "status": "not_installed",
                "path": hook_path.display().to_string(),
            });
            println!("{}", serde_json::to_string_pretty(&result)?);
        } else if !output.quiet {
            println!("No bobbin hook found in {}", hook_path.display());
        }
        return Ok(());
    }

    // Remove bobbin section
    let mut result = String::new();
    let mut in_bobbin_section = false;
    let mut prev_blank = false;
    for line in existing.lines() {
        if line.contains(GIT_HOOK_START_MARKER) {
            in_bobbin_section = true;
            // Remove preceding blank line if any
            if prev_blank && result.ends_with('\n') {
                // Trim trailing blank line
                let trimmed = result.trim_end_matches('\n');
                result = format!("{}\n", trimmed);
            }
            continue;
        }
        if line.contains(GIT_HOOK_END_MARKER) {
            in_bobbin_section = false;
            continue;
        }
        if !in_bobbin_section {
            result.push_str(line);
            result.push('\n');
            prev_blank = line.trim().is_empty();
        }
    }

    // Check if remaining content is just a shebang
    let meaningful = result
        .lines()
        .filter(|l| !l.trim().is_empty() && !l.starts_with("#!"))
        .count();

    if meaningful == 0 {
        // Nothing left — remove the file
        std::fs::remove_file(&hook_path)
            .with_context(|| format!("Failed to remove {}", hook_path.display()))?;
    } else {
        std::fs::write(&hook_path, &result)
            .with_context(|| format!("Failed to write {}", hook_path.display()))?;
    }

    if output.json {
        let result = json!({
            "status": "uninstalled",
            "path": hook_path.display().to_string(),
        });
        println!("{}", serde_json::to_string_pretty(&result)?);
    } else if !output.quiet {
        println!(
            "{} Bobbin post-commit hook removed",
            "✓".green(),
        );
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::search::context::*;
    use crate::types::{ChunkType, MatchType, classify_file};

    #[test]
    fn test_hook_config_output_serialization() {
        let output = HookStatusOutput {
            hooks_installed: false,
            git_hook_installed: false,
            config: HookConfigOutput {
                threshold: 0.5,
                budget: 150,
                content_mode: "preview".to_string(),
                min_prompt_length: 10,
                gate_threshold: 0.75,
                dedup_enabled: true,
            },
            injection_count: 42,
            last_injection_time: Some("2026-02-08T10:30:00Z".to_string()),
            last_session_id: Some("a1b2c3d4e5f6a7b8".to_string()),
        };
        let json = serde_json::to_string(&output).unwrap();
        assert!(json.contains("\"threshold\":0.5"));
        assert!(json.contains("\"budget\":150"));
        assert!(json.contains("\"content_mode\":\"preview\""));
        assert!(json.contains("\"gate_threshold\":0.75"));
        assert!(json.contains("\"dedup_enabled\":true"));
        assert!(json.contains("\"injection_count\":42"));
        assert!(json.contains("\"last_injection_time\":\"2026-02-08T10:30:00Z\""));
        assert!(json.contains("\"last_session_id\":\"a1b2c3d4e5f6a7b8\""));
    }

    #[test]
    fn test_hook_status_output_no_state() {
        let output = HookStatusOutput {
            hooks_installed: true,
            git_hook_installed: false,
            config: HookConfigOutput {
                threshold: 0.5,
                budget: 150,
                content_mode: "preview".to_string(),
                min_prompt_length: 10,
                gate_threshold: 0.75,
                dedup_enabled: true,
            },
            injection_count: 0,
            last_injection_time: None,
            last_session_id: None,
        };
        let json = serde_json::to_string(&output).unwrap();
        assert!(json.contains("\"injection_count\":0"));
        assert!(json.contains("\"last_injection_time\":null"));
        assert!(json.contains("\"last_session_id\":null"));
    }

    #[test]
    fn test_hook_input_deserialization() {
        let json = r#"{"session_id":"abc","prompt":"find auth code","cwd":"/home/user/project","permission_mode":"default","hook_event_name":"UserPromptSubmit"}"#;
        let input: HookInput = serde_json::from_str(json).unwrap();
        assert_eq!(input.prompt, "find auth code");
        assert_eq!(input.cwd, "/home/user/project");
    }

    #[test]
    fn test_hook_input_missing_fields() {
        // Extra fields are ignored, missing optional fields get defaults
        let json = r#"{"prompt":"hello"}"#;
        let input: HookInput = serde_json::from_str(json).unwrap();
        assert_eq!(input.prompt, "hello");
        assert!(input.cwd.is_empty());
    }

    #[test]
    fn test_hook_input_empty_object() {
        let json = r#"{}"#;
        let input: HookInput = serde_json::from_str(json).unwrap();
        assert!(input.prompt.is_empty());
        assert!(input.cwd.is_empty());
    }

    #[test]
    fn test_find_bobbin_root_not_found() {
        let tmp = std::env::temp_dir().join("bobbin_test_no_root");
        std::fs::create_dir_all(&tmp).ok();
        assert!(find_bobbin_root(&tmp).is_none());
        std::fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn test_find_bobbin_root_direct() {
        let tmp = tempfile::tempdir().unwrap();
        let bobbin_dir = tmp.path().join(".bobbin");
        std::fs::create_dir_all(&bobbin_dir).unwrap();
        std::fs::write(bobbin_dir.join("config.toml"), "").unwrap();

        let found = find_bobbin_root(tmp.path());
        assert_eq!(found, Some(tmp.path().to_path_buf()));
    }

    #[test]
    fn test_find_bobbin_root_parent() {
        let tmp = tempfile::tempdir().unwrap();
        let bobbin_dir = tmp.path().join(".bobbin");
        std::fs::create_dir_all(&bobbin_dir).unwrap();
        std::fs::write(bobbin_dir.join("config.toml"), "").unwrap();

        let child = tmp.path().join("src").join("lib");
        std::fs::create_dir_all(&child).unwrap();

        let found = find_bobbin_root(&child);
        assert_eq!(found, Some(tmp.path().to_path_buf()));
    }

    #[test]
    fn test_format_context_empty_bundle() {
        let bundle = ContextBundle {
            query: "test".to_string(),
            files: vec![],
            budget: BudgetInfo {
                max_lines: 150,
                used_lines: 0,
            },
            summary: ContextSummary {
                total_files: 0,
                total_chunks: 0,
                direct_hits: 0,
                coupled_additions: 0,
                bridged_additions: 0,
                source_files: 0,
                doc_files: 0,
                top_semantic_score: 0.0,
            },
        };
        let result = format_context_for_injection(&bundle, 0.0, true);
        assert!(result.contains("0 relevant files"));
    }

    #[test]
    fn test_format_context_with_results() {
        let bundle = ContextBundle {
            query: "auth handler".to_string(),
            files: vec![ContextFile {
                path: "src/auth.rs".to_string(),
                language: "rust".to_string(),
                relevance: FileRelevance::Direct,
                category: classify_file("src/auth.rs"),
                score: 0.85,
                coupled_to: vec![],
                chunks: vec![ContextChunk {
                    name: Some("authenticate".to_string()),
                    chunk_type: ChunkType::Function,
                    start_line: 10,
                    end_line: 25,
                    score: 0.85,
                    match_type: Some(MatchType::Hybrid),
                    content: Some("fn authenticate() {\n    // check token\n}".to_string()),
                }],
            }],
            budget: BudgetInfo {
                max_lines: 150,
                used_lines: 16,
            },
            summary: ContextSummary {
                total_files: 1,
                total_chunks: 1,
                direct_hits: 1,
                coupled_additions: 0,
                bridged_additions: 0,
                source_files: 0,
                doc_files: 0,
                top_semantic_score: 0.0,
            },
        };
        let result = format_context_for_injection(&bundle, 0.5, true);
        assert!(result.contains("src/auth.rs:10-25"));
        assert!(result.contains("authenticate"));
        assert!(result.contains("fn authenticate()"));
        assert!(result.contains("score 0.85"));
    }

    #[test]
    fn test_format_context_threshold_filters() {
        let bundle = ContextBundle {
            query: "test".to_string(),
            files: vec![ContextFile {
                path: "src/low.rs".to_string(),
                language: "rust".to_string(),
                relevance: FileRelevance::Direct,
                category: classify_file("src/low.rs"),
                score: 0.3,
                coupled_to: vec![],
                chunks: vec![ContextChunk {
                    name: Some("low_score_fn".to_string()),
                    chunk_type: ChunkType::Function,
                    start_line: 1,
                    end_line: 5,
                    score: 0.3,
                    match_type: None,
                    content: Some("fn low() {}".to_string()),
                }],
            }],
            budget: BudgetInfo {
                max_lines: 150,
                used_lines: 5,
            },
            summary: ContextSummary {
                total_files: 1,
                total_chunks: 1,
                direct_hits: 1,
                coupled_additions: 0,
                bridged_additions: 0,
                source_files: 0,
                doc_files: 0,
                top_semantic_score: 0.0,
            },
        };
        // With high threshold, chunk content should be filtered out
        let result = format_context_for_injection(&bundle, 0.5, true);
        assert!(!result.contains("low_score_fn"));
    }

    #[test]
    fn test_session_start_input_parsing() {
        let json = r#"{"source": "compact", "cwd": "/tmp/test", "session_id": "abc"}"#;
        let input: SessionStartInput = serde_json::from_str(json).unwrap();
        assert_eq!(input.source, "compact");
        assert_eq!(input.cwd, "/tmp/test");
    }

    #[test]
    fn test_session_start_input_defaults() {
        let json = r#"{}"#;
        let input: SessionStartInput = serde_json::from_str(json).unwrap();
        assert_eq!(input.source, "");
        assert_eq!(input.cwd, "");
    }

    #[test]
    fn test_hook_response_serialization() {
        let response = HookResponse {
            hook_specific_output: HookSpecificOutput {
                hook_event_name: "SessionStart".to_string(),
                additional_context: "test context".to_string(),
            },
        };
        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains("hookSpecificOutput"));
        assert!(json.contains("hookEventName"));
        assert!(json.contains("additionalContext"));
        assert!(json.contains("SessionStart"));
        assert!(json.contains("test context"));
    }

    #[test]
    fn test_format_session_context_modified_files() {
        let modified = vec!["src/main.rs".to_string()];
        let commits: Vec<String> = vec![];
        let symbols: Vec<FileSymbolInfo> = vec![];
        let coupled: Vec<(String, String, f32)> = vec![];

        let result = format_session_context(&modified, &commits, &symbols, &coupled, 150);
        assert!(result.contains("## Working Context"));
        assert!(result.contains("### Modified files"));
        assert!(result.contains("- src/main.rs"));
    }

    #[test]
    fn test_format_session_context_with_symbols() {
        let modified = vec!["src/auth.rs".to_string()];
        let commits: Vec<String> = vec![];
        let symbols = vec![FileSymbolInfo {
            path: "src/auth.rs".to_string(),
            symbols: vec![
                SymbolInfo {
                    name: "validate_token".to_string(),

                },
                SymbolInfo {
                    name: "refresh_session".to_string(),

                },
            ],
        }];
        let coupled: Vec<(String, String, f32)> = vec![];

        let result = format_session_context(&modified, &commits, &symbols, &coupled, 150);
        assert!(result.contains("src/auth.rs (2 symbols: validate_token, refresh_session)"));
    }

    #[test]
    fn test_format_session_context_with_commits() {
        let modified: Vec<String> = vec![];
        let commits = vec![
            "a1b2c3d fix: token refresh race condition".to_string(),
            "d4e5f6g feat: add logout endpoint".to_string(),
        ];
        let symbols: Vec<FileSymbolInfo> = vec![];
        let coupled: Vec<(String, String, f32)> = vec![];

        let result = format_session_context(&modified, &commits, &symbols, &coupled, 150);
        assert!(result.contains("### Recent commits"));
        assert!(result.contains("- a1b2c3d fix: token refresh race condition"));
    }

    #[test]
    fn test_format_session_context_with_coupling() {
        let modified = vec!["src/auth.rs".to_string()];
        let commits: Vec<String> = vec![];
        let symbols: Vec<FileSymbolInfo> = vec![];
        let coupled = vec![(
            "tests/auth_test.rs".to_string(),
            "src/auth.rs".to_string(),
            0.91,
        )];

        let result = format_session_context(&modified, &commits, &symbols, &coupled, 150);
        assert!(result.contains("### Related files (via coupling)"));
        assert!(result.contains("tests/auth_test.rs (coupled with src/auth.rs, score: 0.91)"));
    }

    #[test]
    fn test_format_session_context_budget_enforcement() {
        let modified: Vec<String> = (0..100)
            .map(|i| format!("src/file_{}.rs", i))
            .collect();
        let commits: Vec<String> = vec![];
        let symbols: Vec<FileSymbolInfo> = vec![];
        let coupled: Vec<(String, String, f32)> = vec![];

        let result = format_session_context(&modified, &commits, &symbols, &coupled, 10);
        let line_count = result.lines().count();
        // Budget of 10 — truncation message counts within budget
        assert!(line_count <= 10, "Expected <= 10 lines, got {}", line_count);
        assert!(result.contains("truncated"));
    }

    #[test]
    fn test_format_session_context_many_symbols_truncated() {
        let modified = vec!["src/big.rs".to_string()];
        let commits: Vec<String> = vec![];
        let symbols = vec![FileSymbolInfo {
            path: "src/big.rs".to_string(),
            symbols: (0..8)
                .map(|i| SymbolInfo {
                    name: format!("fn_{}", i),

                })
                .collect(),
        }];
        let coupled: Vec<(String, String, f32)> = vec![];

        let result = format_session_context(&modified, &commits, &symbols, &coupled, 150);
        // Should show 5 symbols + "..." indicator
        assert!(result.contains("8 symbols: fn_0, fn_1, fn_2, fn_3, fn_4, ..."));
    }

    #[test]
    fn test_format_session_context_recently_changed_separate() {
        // Modified files and recently changed files should appear in different sections
        let modified = vec!["src/modified.rs".to_string()];
        let commits: Vec<String> = vec![];
        let symbols = vec![
            FileSymbolInfo {
                path: "src/modified.rs".to_string(),
                symbols: vec![SymbolInfo {
                    name: "mod_fn".to_string(),

                }],
            },
            FileSymbolInfo {
                path: "src/recent.rs".to_string(),
                symbols: vec![SymbolInfo {
                    name: "recent_fn".to_string(),

                }],
            },
        ];
        let coupled: Vec<(String, String, f32)> = vec![];

        let result = format_session_context(&modified, &commits, &symbols, &coupled, 150);
        assert!(result.contains("### Modified files"));
        assert!(result.contains("### Recently changed files"));
        assert!(result.contains("- src/recent.rs (1 symbols: recent_fn)"));
    }

    #[test]
    fn test_format_session_context_empty_produces_header_only() {
        let modified: Vec<String> = vec![];
        let commits: Vec<String> = vec![];
        let symbols: Vec<FileSymbolInfo> = vec![];
        let coupled: Vec<(String, String, f32)> = vec![];

        let result = format_session_context(&modified, &commits, &symbols, &coupled, 150);
        assert!(result.contains("## Working Context"));
        // Header line + trailing newline from blank line join
        assert!(result.lines().count() <= 2);
    }

    // --- Budget enforcement tests for inject-context formatter ---

    #[test]
    fn test_format_context_for_injection_respects_budget() {
        // Build a bundle with many chunks that would exceed a small budget
        let bundle = ContextBundle {
            query: "auth".to_string(),
            files: vec![
                ContextFile {
                    path: "src/a.rs".to_string(),
                    language: "rust".to_string(),
                    relevance: FileRelevance::Direct,
                    category: classify_file("src/a.rs"),
                    score: 0.9,
                    coupled_to: vec![],
                    chunks: vec![
                        ContextChunk {
                            name: Some("fn_a".to_string()),
                            chunk_type: ChunkType::Function,
                            start_line: 1,
                            end_line: 10,
                            score: 0.9,
                            match_type: Some(MatchType::Hybrid),
                            content: Some("line1\nline2\nline3\nline4\nline5\nline6\nline7\nline8\nline9\nline10".to_string()),
                        },
                        ContextChunk {
                            name: Some("fn_b".to_string()),
                            chunk_type: ChunkType::Function,
                            start_line: 20,
                            end_line: 30,
                            score: 0.8,
                            match_type: Some(MatchType::Hybrid),
                            content: Some("b1\nb2\nb3\nb4\nb5\nb6\nb7\nb8\nb9\nb10\nb11".to_string()),
                        },
                    ],
                },
            ],
            budget: BudgetInfo {
                max_lines: 15,
                used_lines: 21,
            },
            summary: ContextSummary {
                total_files: 1,
                total_chunks: 2,
                direct_hits: 2,
                coupled_additions: 0,
                bridged_additions: 0,
                source_files: 0,
                doc_files: 0,
                top_semantic_score: 0.0,
            },
        };
        let result = format_context_for_injection(&bundle, 0.0, true);
        let line_count = result.lines().count();
        // Must not exceed max_lines budget
        assert!(
            line_count <= 15,
            "Expected <= 15 lines, got {}:\n{}",
            line_count,
            result
        );
        // Should include at least the first chunk
        assert!(result.contains("fn_a"));
    }

    #[test]
    fn test_format_context_for_injection_score_format() {
        let bundle = ContextBundle {
            query: "test".to_string(),
            files: vec![ContextFile {
                path: "src/x.rs".to_string(),
                language: "rust".to_string(),
                relevance: FileRelevance::Direct,
                category: classify_file("src/x.rs"),
                score: 0.85,
                coupled_to: vec![],
                chunks: vec![ContextChunk {
                    name: Some("fn_x".to_string()),
                    chunk_type: ChunkType::Function,
                    start_line: 1,
                    end_line: 3,
                    score: 0.856,
                    match_type: None,
                    content: Some("fn x() {}".to_string()),
                }],
            }],
            budget: BudgetInfo {
                max_lines: 150,
                used_lines: 3,
            },
            summary: ContextSummary {
                total_files: 1,
                total_chunks: 1,
                direct_hits: 1,
                coupled_additions: 0,
                bridged_additions: 0,
                source_files: 0,
                doc_files: 0,
                top_semantic_score: 0.0,
            },
        };
        let result = format_context_for_injection(&bundle, 0.0, true);
        // Score should be 2 decimal places
        assert!(result.contains("score 0.86"), "Expected 2-decimal score in: {}", result);
    }

    #[test]
    fn test_format_context_show_docs_false_excludes_doc_files() {
        let bundle = ContextBundle {
            query: "test".to_string(),
            files: vec![
                ContextFile {
                    path: "src/main.rs".to_string(),
                    language: "rust".to_string(),
                    relevance: FileRelevance::Direct,
                    category: classify_file("src/main.rs"),
                    score: 0.9,
                    coupled_to: vec![],
                    chunks: vec![ContextChunk {
                        name: Some("main".to_string()),
                        chunk_type: ChunkType::Function,
                        start_line: 1,
                        end_line: 5,
                        score: 0.9,
                        match_type: None,
                        content: Some("fn main() {}".to_string()),
                    }],
                },
                ContextFile {
                    path: "README.md".to_string(),
                    language: "markdown".to_string(),
                    relevance: FileRelevance::Direct,
                    category: classify_file("README.md"),
                    score: 0.8,
                    coupled_to: vec![],
                    chunks: vec![ContextChunk {
                        name: None,
                        chunk_type: ChunkType::Module,
                        start_line: 1,
                        end_line: 10,
                        score: 0.8,
                        match_type: None,
                        content: Some("# My Project".to_string()),
                    }],
                },
            ],
            budget: BudgetInfo {
                max_lines: 150,
                used_lines: 15,
            },
            summary: ContextSummary {
                total_files: 2,
                total_chunks: 2,
                direct_hits: 2,
                coupled_additions: 0,
                bridged_additions: 0,
                source_files: 1,
                doc_files: 1,
                top_semantic_score: 0.0,
            },
        };

        // show_docs=true should include both
        let with_docs = format_context_for_injection(&bundle, 0.0, true);
        assert!(with_docs.contains("Source Files"), "Should have source section");
        assert!(with_docs.contains("Documentation"), "Should have doc section");
        assert!(with_docs.contains("README.md"));

        // show_docs=false should exclude documentation
        let without_docs = format_context_for_injection(&bundle, 0.0, false);
        assert!(without_docs.contains("Source Files"), "Should have source section");
        assert!(!without_docs.contains("Documentation"), "Should not have doc section");
        assert!(!without_docs.contains("README.md"), "Doc file should be excluded");
        assert!(without_docs.contains("src/main.rs"), "Source file should remain");
    }

    #[test]
    fn test_format_context_budget_zero() {
        let bundle = ContextBundle {
            query: "test".to_string(),
            files: vec![ContextFile {
                path: "src/a.rs".to_string(),
                language: "rust".to_string(),
                relevance: FileRelevance::Direct,
                category: classify_file("src/a.rs"),
                score: 0.9,
                coupled_to: vec![],
                chunks: vec![ContextChunk {
                    name: Some("fn_a".to_string()),
                    chunk_type: ChunkType::Function,
                    start_line: 1,
                    end_line: 10,
                    score: 0.9,
                    match_type: None,
                    content: Some("fn a() {}".to_string()),
                }],
            }],
            budget: BudgetInfo {
                max_lines: 0,
                used_lines: 0,
            },
            summary: ContextSummary {
                total_files: 1,
                total_chunks: 1,
                direct_hits: 1,
                coupled_additions: 0,
                bridged_additions: 0,
                source_files: 1,
                doc_files: 0,
                top_semantic_score: 0.0,
            },
        };
        // Budget 0 — should not panic and should produce empty or minimal output
        let result = format_context_for_injection(&bundle, 0.0, true);
        assert!(result.lines().count() <= 1, "Budget 0 should produce at most the header");
    }

    #[test]
    fn test_format_context_no_content() {
        // Test formatting when content is None (ContentMode::None)
        let bundle = ContextBundle {
            query: "test".to_string(),
            files: vec![ContextFile {
                path: "src/a.rs".to_string(),
                language: "rust".to_string(),
                relevance: FileRelevance::Direct,
                category: classify_file("src/a.rs"),
                score: 0.9,
                coupled_to: vec![],
                chunks: vec![ContextChunk {
                    name: Some("fn_a".to_string()),
                    chunk_type: ChunkType::Function,
                    start_line: 1,
                    end_line: 10,
                    score: 0.9,
                    match_type: None,
                    content: None,
                }],
            }],
            budget: BudgetInfo {
                max_lines: 150,
                used_lines: 10,
            },
            summary: ContextSummary {
                total_files: 1,
                total_chunks: 1,
                direct_hits: 1,
                coupled_additions: 0,
                bridged_additions: 0,
                source_files: 1,
                doc_files: 0,
                top_semantic_score: 0.0,
            },
        };
        let result = format_context_for_injection(&bundle, 0.0, true);
        // Should still have the chunk header with file:lines
        assert!(result.contains("src/a.rs:1-10"));
        assert!(result.contains("fn_a"));
    }

    #[test]
    fn test_format_session_context_very_small_budget() {
        let modified = vec!["src/a.rs".to_string(), "src/b.rs".to_string()];
        let commits: Vec<String> = vec![];
        let symbols: Vec<FileSymbolInfo> = vec![];
        let coupled: Vec<(String, String, f32)> = vec![];

        // Budget of 3 — header + blank + 1 content line at most
        let result = format_session_context(&modified, &commits, &symbols, &coupled, 3);
        let line_count = result.lines().count();
        assert!(line_count <= 3, "Expected <= 3 lines, got {}:\n{}", line_count, result);
    }

    #[test]
    fn test_format_session_context_budget_zero() {
        let modified = vec!["src/a.rs".to_string()];
        let commits: Vec<String> = vec![];
        let symbols: Vec<FileSymbolInfo> = vec![];
        let coupled: Vec<(String, String, f32)> = vec![];

        // Budget of 0 — should still not panic
        let result = format_session_context(&modified, &commits, &symbols, &coupled, 0);
        // Should produce at most the truncation message
        assert!(result.lines().count() <= 1);
    }

    // --- Hook installer unit tests ---

    #[test]
    fn test_merge_hooks_into_empty_settings() {
        let mut settings = json!({});
        merge_hooks(&mut settings);

        assert!(settings.get("hooks").is_some());
        let hooks = &settings["hooks"];
        assert!(hooks.get("UserPromptSubmit").is_some());
        assert!(hooks.get("SessionStart").is_some());

        // Verify inject-context command
        let ups = hooks["UserPromptSubmit"].as_array().unwrap();
        assert_eq!(ups.len(), 1);
        let cmd = ups[0]["hooks"][0]["command"].as_str().unwrap();
        assert_eq!(cmd, "bobbin hook inject-context");

        // Verify session-context command
        let ss = hooks["SessionStart"].as_array().unwrap();
        assert_eq!(ss.len(), 1);
        let cmd = ss[0]["hooks"][0]["command"].as_str().unwrap();
        assert_eq!(cmd, "bobbin hook session-context");
        assert_eq!(ss[0]["matcher"].as_str().unwrap(), "compact");
    }

    #[test]
    fn test_merge_hooks_preserves_existing_hooks() {
        let mut settings = json!({
            "hooks": {
                "UserPromptSubmit": [
                    {
                        "hooks": [
                            {
                                "type": "command",
                                "command": "other-tool inject",
                                "timeout": 5
                            }
                        ]
                    }
                ]
            },
            "other_key": "preserved"
        });

        merge_hooks(&mut settings);

        // other_key should still be there
        assert_eq!(settings["other_key"].as_str().unwrap(), "preserved");

        // UserPromptSubmit should have both the other tool AND bobbin
        let ups = settings["hooks"]["UserPromptSubmit"].as_array().unwrap();
        assert_eq!(ups.len(), 2);
        assert_eq!(
            ups[0]["hooks"][0]["command"].as_str().unwrap(),
            "other-tool inject"
        );
        assert_eq!(
            ups[1]["hooks"][0]["command"].as_str().unwrap(),
            "bobbin hook inject-context"
        );
    }

    #[test]
    fn test_merge_hooks_idempotent() {
        let mut settings = json!({});
        merge_hooks(&mut settings);
        merge_hooks(&mut settings); // Second time

        let ups = settings["hooks"]["UserPromptSubmit"].as_array().unwrap();
        assert_eq!(ups.len(), 1, "Should not duplicate bobbin hooks");

        let ss = settings["hooks"]["SessionStart"].as_array().unwrap();
        assert_eq!(ss.len(), 1, "Should not duplicate bobbin hooks");
    }

    #[test]
    fn test_is_bobbin_hook_group_true() {
        let group = json!({
            "hooks": [
                {
                    "type": "command",
                    "command": "bobbin hook inject-context",
                    "timeout": 10
                }
            ]
        });
        assert!(is_bobbin_hook_group(&group));
    }

    #[test]
    fn test_is_bobbin_hook_group_false() {
        let group = json!({
            "hooks": [
                {
                    "type": "command",
                    "command": "other-tool do-thing",
                    "timeout": 5
                }
            ]
        });
        assert!(!is_bobbin_hook_group(&group));
    }

    #[test]
    fn test_remove_bobbin_hooks_leaves_others() {
        let mut settings = json!({
            "hooks": {
                "UserPromptSubmit": [
                    {
                        "hooks": [
                            { "type": "command", "command": "other-tool inject" }
                        ]
                    },
                    {
                        "hooks": [
                            { "type": "command", "command": "bobbin hook inject-context" }
                        ]
                    }
                ],
                "SessionStart": [
                    {
                        "matcher": "compact",
                        "hooks": [
                            { "type": "command", "command": "bobbin hook session-context" }
                        ]
                    }
                ]
            }
        });

        let removed = remove_bobbin_hooks(&mut settings);
        assert!(removed);

        // other-tool should remain
        let ups = settings["hooks"]["UserPromptSubmit"].as_array().unwrap();
        assert_eq!(ups.len(), 1);
        assert_eq!(
            ups[0]["hooks"][0]["command"].as_str().unwrap(),
            "other-tool inject"
        );

        // SessionStart was only bobbin, so it should be removed entirely
        assert!(settings["hooks"].get("SessionStart").is_none());
    }

    #[test]
    fn test_remove_bobbin_hooks_cleans_empty_hooks_object() {
        let mut settings = json!({
            "hooks": {
                "UserPromptSubmit": [
                    {
                        "hooks": [
                            { "type": "command", "command": "bobbin hook inject-context" }
                        ]
                    }
                ]
            },
            "other": true
        });

        let removed = remove_bobbin_hooks(&mut settings);
        assert!(removed);

        // hooks object should be fully removed
        assert!(settings.get("hooks").is_none());
        // other keys preserved
        assert_eq!(settings["other"].as_bool().unwrap(), true);
    }

    #[test]
    fn test_remove_bobbin_hooks_none_present() {
        let mut settings = json!({
            "hooks": {
                "UserPromptSubmit": [
                    {
                        "hooks": [
                            { "type": "command", "command": "other-tool inject" }
                        ]
                    }
                ]
            }
        });

        let removed = remove_bobbin_hooks(&mut settings);
        assert!(!removed);

        // Nothing should change
        let ups = settings["hooks"]["UserPromptSubmit"].as_array().unwrap();
        assert_eq!(ups.len(), 1);
    }

    #[test]
    fn test_has_bobbin_hooks_true() {
        let settings = json!({
            "hooks": {
                "UserPromptSubmit": [
                    {
                        "hooks": [
                            { "type": "command", "command": "bobbin hook inject-context" }
                        ]
                    }
                ]
            }
        });
        assert!(has_bobbin_hooks(&settings));
    }

    #[test]
    fn test_has_bobbin_hooks_false() {
        let settings = json!({
            "hooks": {
                "UserPromptSubmit": [
                    {
                        "hooks": [
                            { "type": "command", "command": "other-tool" }
                        ]
                    }
                ]
            }
        });
        assert!(!has_bobbin_hooks(&settings));
    }

    #[test]
    fn test_has_bobbin_hooks_empty() {
        assert!(!has_bobbin_hooks(&json!({})));
    }

    #[test]
    fn test_read_settings_missing_file() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("nonexistent.json");
        let settings = read_settings(&path).unwrap();
        assert_eq!(settings, json!({}));
    }

    #[test]
    fn test_read_settings_empty_file() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("empty.json");
        std::fs::write(&path, "").unwrap();
        let settings = read_settings(&path).unwrap();
        assert_eq!(settings, json!({}));
    }

    #[test]
    fn test_read_settings_valid_file() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("valid.json");
        std::fs::write(&path, r#"{"key": "value"}"#).unwrap();
        let settings = read_settings(&path).unwrap();
        assert_eq!(settings["key"].as_str().unwrap(), "value");
    }

    #[test]
    fn test_write_settings_creates_dirs() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("deep").join("nested").join("settings.json");
        let settings = json!({"test": true});
        write_settings(&path, &settings).unwrap();

        let content = std::fs::read_to_string(&path).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&content).unwrap();
        assert_eq!(parsed["test"].as_bool().unwrap(), true);
    }

    #[test]
    fn test_merge_hooks_preserves_unrelated_events() {
        // Events that bobbin doesn't use should be completely untouched
        let mut settings = json!({
            "hooks": {
                "PreToolUse": [
                    {
                        "hooks": [
                            { "type": "command", "command": "gt tap guard pr-workflow" }
                        ],
                        "matcher": "Bash(gh pr create*)"
                    }
                ],
                "Stop": [
                    {
                        "hooks": [
                            { "type": "command", "command": "gt costs record" }
                        ],
                        "matcher": ""
                    }
                ],
                "PostToolUseFailure": [
                    {
                        "hooks": [
                            { "type": "command", "command": "dp record --source claude-code" }
                        ],
                        "matcher": ".*"
                    }
                ]
            }
        });

        merge_hooks(&mut settings);

        // All original events preserved
        let hooks = &settings["hooks"];
        assert_eq!(hooks["PreToolUse"].as_array().unwrap().len(), 1);
        assert_eq!(
            hooks["PreToolUse"][0]["hooks"][0]["command"].as_str().unwrap(),
            "gt tap guard pr-workflow"
        );
        assert_eq!(hooks["Stop"].as_array().unwrap().len(), 1);
        assert_eq!(hooks["PostToolUseFailure"].as_array().unwrap().len(), 1);

        // Bobbin events added
        assert!(hooks["UserPromptSubmit"].is_array());
        assert!(hooks["SessionStart"].is_array());
    }

    #[test]
    fn test_merge_hooks_preserves_non_hook_settings() {
        // Top-level keys like statusLine must survive
        let mut settings = json!({
            "statusLine": {
                "command": "bash ~/.claude/statusline-command.sh",
                "type": "command"
            },
            "permissions": {
                "allow": ["Bash(cargo *)"]
            }
        });

        merge_hooks(&mut settings);

        assert_eq!(
            settings["statusLine"]["command"].as_str().unwrap(),
            "bash ~/.claude/statusline-command.sh"
        );
        assert_eq!(
            settings["permissions"]["allow"][0].as_str().unwrap(),
            "Bash(cargo *)"
        );
    }

    #[test]
    fn test_merge_hooks_realistic_multi_tool_settings() {
        // Mirrors a real ~/.claude/settings.json with Gas Town + dp hooks
        let mut settings = json!({
            "hooks": {
                "UserPromptSubmit": [
                    {
                        "hooks": [
                            { "type": "command", "command": "gt mail check --inject" }
                        ],
                        "matcher": ""
                    }
                ],
                "SessionStart": [
                    {
                        "hooks": [
                            { "type": "command", "command": "gt prime --hook" }
                        ],
                        "matcher": ""
                    }
                ],
                "PreCompact": [
                    {
                        "hooks": [
                            { "type": "command", "command": "gt prime --hook" }
                        ],
                        "matcher": ""
                    }
                ],
                "Stop": [
                    {
                        "hooks": [
                            { "type": "command", "command": "gt costs record" }
                        ],
                        "matcher": ""
                    }
                ]
            },
            "statusLine": {
                "command": "bash ~/.claude/statusline-command.sh",
                "type": "command"
            }
        });

        merge_hooks(&mut settings);

        let hooks = &settings["hooks"];

        // Gas Town hooks in shared events preserved alongside bobbin
        let ups = hooks["UserPromptSubmit"].as_array().unwrap();
        assert_eq!(ups.len(), 2);
        assert_eq!(ups[0]["hooks"][0]["command"].as_str().unwrap(), "gt mail check --inject");
        assert_eq!(ups[1]["hooks"][0]["command"].as_str().unwrap(), "bobbin hook inject-context");

        let ss = hooks["SessionStart"].as_array().unwrap();
        assert_eq!(ss.len(), 2);
        assert_eq!(ss[0]["hooks"][0]["command"].as_str().unwrap(), "gt prime --hook");
        assert_eq!(ss[1]["hooks"][0]["command"].as_str().unwrap(), "bobbin hook session-context");

        // Events bobbin doesn't touch are untouched
        assert_eq!(hooks["PreCompact"].as_array().unwrap().len(), 1);
        assert_eq!(hooks["Stop"].as_array().unwrap().len(), 1);

        // Non-hook settings preserved
        assert!(settings["statusLine"].is_object());
    }

    #[test]
    fn test_merge_hooks_idempotent_with_other_tools() {
        // Merge twice with non-bobbin hooks — should not duplicate anything
        let mut settings = json!({
            "hooks": {
                "UserPromptSubmit": [
                    {
                        "hooks": [
                            { "type": "command", "command": "gt mail check --inject" }
                        ]
                    }
                ]
            }
        });

        merge_hooks(&mut settings);
        merge_hooks(&mut settings);

        let ups = settings["hooks"]["UserPromptSubmit"].as_array().unwrap();
        assert_eq!(ups.len(), 2, "gt hook + 1 bobbin hook, no duplicates");

        let ss = settings["hooks"]["SessionStart"].as_array().unwrap();
        assert_eq!(ss.len(), 1, "Only 1 bobbin SessionStart hook");
    }

    #[test]
    fn test_bobbin_hook_entries_structure() {
        let entries = bobbin_hook_entries();
        let hooks = entries.get("hooks").unwrap();

        // UserPromptSubmit
        let ups = hooks["UserPromptSubmit"].as_array().unwrap();
        assert_eq!(ups.len(), 1);
        assert_eq!(ups[0]["hooks"][0]["type"].as_str().unwrap(), "command");
        assert_eq!(ups[0]["hooks"][0]["timeout"].as_i64().unwrap(), 10);

        // SessionStart
        let ss = hooks["SessionStart"].as_array().unwrap();
        assert_eq!(ss.len(), 1);
        assert_eq!(ss[0]["matcher"].as_str().unwrap(), "compact");
    }

    #[test]
    fn test_git_hook_section_has_markers() {
        assert!(GIT_HOOK_SECTION.contains(GIT_HOOK_START_MARKER));
        assert!(GIT_HOOK_SECTION.contains(GIT_HOOK_END_MARKER));
        assert!(GIT_HOOK_SECTION.contains("bobbin index --quiet"));
    }

    // --- Session dedup tests ---

    #[test]
    fn test_hook_state_serde_roundtrip() {
        let mut chunk_freqs = HashMap::new();
        chunk_freqs.insert(
            "src/foo.rs:10:50".to_string(),
            ChunkFrequency {
                count: 12,
                file: "src/foo.rs".to_string(),
                name: Some("InjectContextArgs".to_string()),
            },
        );
        let mut file_freqs = HashMap::new();
        file_freqs.insert("src/foo.rs".to_string(), 15);

        let state = HookState {
            last_session_id: "a1b2c3d4e5f6a7b8".to_string(),
            last_injected_chunks: vec!["src/foo.rs:10:50".to_string()],
            last_injection_time: "2026-02-08T10:30:00Z".to_string(),
            injection_count: 47,
            chunk_frequencies: chunk_freqs,
            file_frequencies: file_freqs,
            hot_topics_generated_at: 40,
        };

        let json = serde_json::to_string_pretty(&state).unwrap();
        let parsed: HookState = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.last_session_id, "a1b2c3d4e5f6a7b8");
        assert_eq!(parsed.injection_count, 47);
        assert_eq!(parsed.chunk_frequencies["src/foo.rs:10:50"].count, 12);
        assert_eq!(parsed.file_frequencies["src/foo.rs"], 15);
        assert_eq!(parsed.hot_topics_generated_at, 40);
    }

    #[test]
    fn test_hook_state_default() {
        let state = HookState::default();
        assert!(state.last_session_id.is_empty());
        assert!(state.last_injected_chunks.is_empty());
        assert_eq!(state.injection_count, 0);
        assert!(state.chunk_frequencies.is_empty());
        assert!(state.file_frequencies.is_empty());
    }

    #[test]
    fn test_hook_state_deserialize_corrupt_falls_back() {
        let corrupt = "{ not valid json at all }}}";
        let state: HookState = serde_json::from_str(corrupt).unwrap_or_default();
        assert!(state.last_session_id.is_empty());
        assert_eq!(state.injection_count, 0);
    }

    #[test]
    fn test_hook_state_deserialize_partial_fields() {
        // Only some fields present — rest should default
        let json = r#"{"last_session_id": "abc", "injection_count": 5}"#;
        let state: HookState = serde_json::from_str(json).unwrap();
        assert_eq!(state.last_session_id, "abc");
        assert_eq!(state.injection_count, 5);
        assert!(state.chunk_frequencies.is_empty());
        assert!(state.file_frequencies.is_empty());
    }

    #[test]
    fn test_load_save_hook_state() {
        let tmp = tempfile::tempdir().unwrap();
        let bobbin_dir = tmp.path().join(".bobbin");
        std::fs::create_dir_all(&bobbin_dir).unwrap();

        // Load from nonexistent file returns default
        let state = load_hook_state(tmp.path());
        assert!(state.last_session_id.is_empty());

        // Save and reload
        let mut state = HookState::default();
        state.last_session_id = "test123".to_string();
        state.injection_count = 3;
        save_hook_state(tmp.path(), &state);

        let loaded = load_hook_state(tmp.path());
        assert_eq!(loaded.last_session_id, "test123");
        assert_eq!(loaded.injection_count, 3);
    }

    #[test]
    fn test_compute_session_id_deterministic() {
        let bundle = ContextBundle {
            query: "test".to_string(),
            files: vec![ContextFile {
                path: "src/a.rs".to_string(),
                language: "rust".to_string(),
                relevance: FileRelevance::Direct,
                category: classify_file("src/a.rs"),
                score: 0.9,
                coupled_to: vec![],
                chunks: vec![
                    ContextChunk {
                        name: Some("fn_a".to_string()),
                        chunk_type: ChunkType::Function,
                        start_line: 10,
                        end_line: 20,
                        score: 0.9,
                        match_type: None,
                        content: None,
                    },
                    ContextChunk {
                        name: Some("fn_b".to_string()),
                        chunk_type: ChunkType::Function,
                        start_line: 30,
                        end_line: 40,
                        score: 0.8,
                        match_type: None,
                        content: None,
                    },
                ],
            }],
            budget: BudgetInfo {
                max_lines: 150,
                used_lines: 10,
            },
            summary: ContextSummary {
                total_files: 1,
                total_chunks: 2,
                direct_hits: 2,
                coupled_additions: 0,
                bridged_additions: 0,
                source_files: 0,
                doc_files: 0,
                top_semantic_score: 0.9,
            },
        };

        let id1 = compute_session_id(&bundle, 0.5);
        let id2 = compute_session_id(&bundle, 0.5);
        assert_eq!(id1, id2);
        assert_eq!(id1.len(), 16); // 16 hex chars
    }

    #[test]
    fn test_compute_session_id_changes_with_different_chunks() {
        let make_bundle = |start: u32| ContextBundle {
            query: "test".to_string(),
            files: vec![ContextFile {
                path: "src/a.rs".to_string(),
                language: "rust".to_string(),
                relevance: FileRelevance::Direct,
                category: classify_file("src/a.rs"),
                score: 0.9,
                coupled_to: vec![],
                chunks: vec![ContextChunk {
                    name: None,
                    chunk_type: ChunkType::Function,
                    start_line: start,
                    end_line: start + 10,
                    score: 0.9,
                    match_type: None,
                    content: None,
                }],
            }],
            budget: BudgetInfo {
                max_lines: 150,
                used_lines: 5,
            },
            summary: ContextSummary {
                total_files: 1,
                total_chunks: 1,
                direct_hits: 1,
                coupled_additions: 0,
                bridged_additions: 0,
                source_files: 0,
                doc_files: 0,
                top_semantic_score: 0.9,
            },
        };

        let id1 = compute_session_id(&make_bundle(10), 0.0);
        let id2 = compute_session_id(&make_bundle(50), 0.0);
        assert_ne!(id1, id2);
    }

    #[test]
    fn test_compute_session_id_filters_by_threshold() {
        let bundle = ContextBundle {
            query: "test".to_string(),
            files: vec![ContextFile {
                path: "src/a.rs".to_string(),
                language: "rust".to_string(),
                relevance: FileRelevance::Direct,
                category: classify_file("src/a.rs"),
                score: 0.9,
                coupled_to: vec![],
                chunks: vec![
                    ContextChunk {
                        name: None,
                        chunk_type: ChunkType::Function,
                        start_line: 1,
                        end_line: 10,
                        score: 0.9,
                        match_type: None,
                        content: None,
                    },
                    ContextChunk {
                        name: None,
                        chunk_type: ChunkType::Function,
                        start_line: 20,
                        end_line: 30,
                        score: 0.3, // Below threshold
                        match_type: None,
                        content: None,
                    },
                ],
            }],
            budget: BudgetInfo {
                max_lines: 150,
                used_lines: 10,
            },
            summary: ContextSummary {
                total_files: 1,
                total_chunks: 2,
                direct_hits: 2,
                coupled_additions: 0,
                bridged_additions: 0,
                source_files: 0,
                doc_files: 0,
                top_semantic_score: 0.9,
            },
        };

        // With threshold 0.5, low-score chunk is excluded
        let id_high = compute_session_id(&bundle, 0.5);
        // With threshold 0.0, both chunks included
        let id_low = compute_session_id(&bundle, 0.0);
        assert_ne!(id_high, id_low);
    }

    #[test]
    fn test_compute_session_id_empty_bundle() {
        let bundle = ContextBundle {
            query: "test".to_string(),
            files: vec![],
            budget: BudgetInfo {
                max_lines: 150,
                used_lines: 0,
            },
            summary: ContextSummary {
                total_files: 0,
                total_chunks: 0,
                direct_hits: 0,
                coupled_additions: 0,
                bridged_additions: 0,
                source_files: 0,
                doc_files: 0,
                top_semantic_score: 0.0,
            },
        };

        let id = compute_session_id(&bundle, 0.0);
        assert_eq!(id.len(), 16);
    }

    #[test]
    fn test_compute_session_id_top_10_limit() {
        // Create 15 chunks — only first 10 (sorted alphabetically by key) should matter
        let chunks: Vec<ContextChunk> = (0..15)
            .map(|i| ContextChunk {
                name: None,
                chunk_type: ChunkType::Function,
                start_line: i * 10,
                end_line: i * 10 + 5,
                score: 0.9,
                match_type: None,
                content: None,
            })
            .collect();

        let bundle_all = ContextBundle {
            query: "test".to_string(),
            files: vec![ContextFile {
                path: "src/a.rs".to_string(),
                language: "rust".to_string(),
                relevance: FileRelevance::Direct,
                category: classify_file("src/a.rs"),
                score: 0.9,
                coupled_to: vec![],
                chunks: chunks.clone(),
            }],
            budget: BudgetInfo {
                max_lines: 150,
                used_lines: 50,
            },
            summary: ContextSummary {
                total_files: 1,
                total_chunks: 15,
                direct_hits: 15,
                coupled_additions: 0,
                bridged_additions: 0,
                source_files: 0,
                doc_files: 0,
                top_semantic_score: 0.9,
            },
        };

        // Build a bundle with the top-10 keys (alphabetically sorted) from all 15
        let mut all_keys: Vec<String> = chunks
            .iter()
            .map(|c| format!("src/a.rs:{}:{}", c.start_line, c.end_line))
            .collect();
        all_keys.sort();
        let top_10_keys: HashSet<String> = all_keys.into_iter().take(10).collect();

        let top_10_chunks: Vec<ContextChunk> = chunks
            .iter()
            .filter(|c| {
                let key = format!("src/a.rs:{}:{}", c.start_line, c.end_line);
                top_10_keys.contains(&key)
            })
            .cloned()
            .collect();

        let bundle_ten = ContextBundle {
            query: "test".to_string(),
            files: vec![ContextFile {
                path: "src/a.rs".to_string(),
                language: "rust".to_string(),
                relevance: FileRelevance::Direct,
                category: classify_file("src/a.rs"),
                score: 0.9,
                coupled_to: vec![],
                chunks: top_10_chunks,
            }],
            budget: BudgetInfo {
                max_lines: 150,
                used_lines: 30,
            },
            summary: ContextSummary {
                total_files: 1,
                total_chunks: 10,
                direct_hits: 10,
                coupled_additions: 0,
                bridged_additions: 0,
                source_files: 0,
                doc_files: 0,
                top_semantic_score: 0.9,
            },
        };

        let id_all = compute_session_id(&bundle_all, 0.0);
        let id_ten = compute_session_id(&bundle_ten, 0.0);
        assert_eq!(id_all, id_ten, "Top-10 truncation should produce same ID");
    }

    // --- Hot topics tests ---

    #[test]
    fn test_generate_hot_topics_empty_state() {
        let tmp = tempfile::tempdir().unwrap();
        let output_path = tmp.path().join("hot-topics.md");

        let state = HookState::default();
        generate_hot_topics(&state, &output_path).unwrap();

        let content = std::fs::read_to_string(&output_path).unwrap();
        assert!(content.contains("# Hot Topics (auto-generated by bobbin)"));
        assert!(content.contains("Based on 0 context injections."));
        assert!(content.contains("No injection data yet."));
        assert!(content.contains("## Frequently Referenced Code"));
        assert!(content.contains("## Most Referenced Files"));
    }

    #[test]
    fn test_generate_hot_topics_with_data() {
        let tmp = tempfile::tempdir().unwrap();
        let output_path = tmp.path().join("hot-topics.md");

        let mut chunk_freqs = HashMap::new();
        chunk_freqs.insert(
            "src/cli/hook.rs:10:50".to_string(),
            ChunkFrequency {
                count: 12,
                file: "src/cli/hook.rs".to_string(),
                name: Some("InjectContextArgs".to_string()),
            },
        );
        chunk_freqs.insert(
            "src/config.rs:20:40".to_string(),
            ChunkFrequency {
                count: 9,
                file: "src/config.rs".to_string(),
                name: Some("HooksConfig".to_string()),
            },
        );
        chunk_freqs.insert(
            "src/search/context.rs:5:30".to_string(),
            ChunkFrequency {
                count: 7,
                file: "src/search/context.rs".to_string(),
                name: None,
            },
        );

        let mut file_freqs = HashMap::new();
        file_freqs.insert("src/cli/hook.rs".to_string(), 15);
        file_freqs.insert("src/config.rs".to_string(), 12);
        file_freqs.insert("src/search/context.rs".to_string(), 9);

        let state = HookState {
            last_session_id: "abc123".to_string(),
            last_injected_chunks: vec![],
            last_injection_time: "2026-02-08T10:30:00Z".to_string(),
            injection_count: 47,
            chunk_frequencies: chunk_freqs,
            file_frequencies: file_freqs,
            hot_topics_generated_at: 40,
        };

        generate_hot_topics(&state, &output_path).unwrap();

        let content = std::fs::read_to_string(&output_path).unwrap();
        assert!(content.contains("Based on 47 context injections."));
        assert!(content.contains("2026-02-08 10:30 UTC"));

        // Chunks should be ranked by count descending
        let hook_pos = content.find("InjectContextArgs").unwrap();
        let config_pos = content.find("HooksConfig").unwrap();
        assert!(hook_pos < config_pos, "Higher-count chunk should appear first");

        // File table present and ranked
        assert!(content.contains("| src/cli/hook.rs | 15 |"));
        assert!(content.contains("| src/config.rs | 12 |"));

        // Symbol-less chunk shows dash
        assert!(content.contains("| - |"));
    }

    #[test]
    fn test_generate_hot_topics_truncates_chunks_to_20() {
        let tmp = tempfile::tempdir().unwrap();
        let output_path = tmp.path().join("hot-topics.md");

        let mut chunk_freqs = HashMap::new();
        for i in 0..30 {
            chunk_freqs.insert(
                format!("src/file{}.rs:1:10", i),
                ChunkFrequency {
                    count: 30 - i,
                    file: format!("src/file{}.rs", i),
                    name: Some(format!("fn_{}", i)),
                },
            );
        }

        let state = HookState {
            injection_count: 100,
            chunk_frequencies: chunk_freqs,
            ..Default::default()
        };

        generate_hot_topics(&state, &output_path).unwrap();

        let content = std::fs::read_to_string(&output_path).unwrap();
        // Count chunk table rows (lines starting with "| <digit>")
        let chunk_section = content
            .split("## Frequently Referenced Code")
            .nth(1)
            .unwrap()
            .split("## Most Referenced Files")
            .next()
            .unwrap();
        let rank_rows: Vec<&str> = chunk_section
            .lines()
            .filter(|l| l.starts_with("| ") && l.chars().nth(2).map_or(false, |c| c.is_ascii_digit()))
            .collect();
        assert_eq!(rank_rows.len(), 20);
    }

    #[test]
    fn test_generate_hot_topics_truncates_files_to_10() {
        let tmp = tempfile::tempdir().unwrap();
        let output_path = tmp.path().join("hot-topics.md");

        let mut file_freqs = HashMap::new();
        for i in 0..15 {
            file_freqs.insert(format!("src/file{}.rs", i), 15 - i as u64);
        }

        let state = HookState {
            injection_count: 50,
            file_frequencies: file_freqs,
            ..Default::default()
        };

        generate_hot_topics(&state, &output_path).unwrap();

        let content = std::fs::read_to_string(&output_path).unwrap();
        // Count table rows in the file section
        let file_section = content.split("## Most Referenced Files").nth(1).unwrap();
        let row_count = file_section.lines().filter(|l| l.starts_with("| src/")).count();
        assert_eq!(row_count, 10);
    }
}
