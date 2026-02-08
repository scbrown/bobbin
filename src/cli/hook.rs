use anyhow::{Context, Result};
use clap::{Args, Subcommand};
use colored::Colorize;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashSet;
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

    /// Install a post-commit git hook for automatic indexing
    InstallGitHook(InstallGitHookArgs),

    /// Remove the bobbin post-commit git hook
    UninstallGitHook(UninstallGitHookArgs),
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
}

#[derive(Args)]
struct SessionContextArgs {
    /// Maximum lines of context (overrides config)
    #[arg(long)]
    budget: Option<usize>,
}

#[derive(Args)]
struct InstallGitHookArgs {}

#[derive(Args)]
struct UninstallGitHookArgs {}

#[derive(Serialize)]
struct HookStatusOutput {
    hooks_installed: bool,
    git_hook_installed: bool,
    config: HookConfigOutput,
}

#[derive(Serialize)]
struct HookConfigOutput {
    threshold: f32,
    budget: usize,
    content_mode: String,
    min_prompt_length: usize,
}

pub async fn run(args: HookArgs, output: OutputConfig) -> Result<()> {
    match args.command {
        HookCommands::Install(a) => run_install(a, output).await,
        HookCommands::Uninstall(a) => run_uninstall(a, output).await,
        HookCommands::Status(a) => run_status(a, output).await,
        HookCommands::InjectContext(a) => run_inject_context(a, output).await,
        HookCommands::SessionContext(a) => run_session_context(a, output).await,
        HookCommands::InstallGitHook(a) => run_install_git_hook(a, output).await,
        HookCommands::UninstallGitHook(a) => run_uninstall_git_hook(a, output).await,
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
                if arr.iter().any(|e| is_bobbin_hook_group(e)) {
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

    let config = match Config::load(&Config::config_path(&repo_root)) {
        Ok(c) => c,
        Err(_) => Config::default(),
    };

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

    if output.json {
        let status = HookStatusOutput {
            hooks_installed,
            git_hook_installed,
            config: HookConfigOutput {
                threshold: hooks_cfg.threshold,
                budget: hooks_cfg.budget,
                content_mode: hooks_cfg.content_mode.clone(),
                min_prompt_length: hooks_cfg.min_prompt_length,
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
        println!();
        let hooks_str = if hooks_installed { "installed".green() } else { "not installed".yellow() };
        let git_str = if git_hook_installed { "installed".green() } else { "not installed".yellow() };
        println!("  Claude Code hooks: {}", hooks_str);
        println!("  Git post-commit:   {}", git_str);
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
) -> String {
    use std::fmt::Write;

    let budget = bundle.budget.max_lines;
    let mut out = String::new();

    let header = format!(
        "Bobbin found {} relevant files ({} chunks, {}/{} budget lines):",
        bundle.summary.total_files,
        bundle.summary.total_chunks,
        bundle.budget.used_lines,
        bundle.budget.max_lines,
    );
    out.push_str(&header);
    out.push('\n');

    for file in &bundle.files {
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
            let candidate = format!("{}{}", out, chunk_section);
            if candidate.lines().count() > budget {
                break;
            }
            let _ = write!(out, "{}", chunk_section);
        }
    }

    // Final enforcement: trim to budget
    let lines: Vec<&str> = out.lines().collect();
    if lines.len() > budget {
        lines[..budget].join("\n") + "\n"
    } else {
        out
    }
}

/// Inner implementation that can return errors (caller swallows them).
async fn inject_context_inner(args: InjectContextArgs) -> Result<()> {
    use crate::index::Embedder;
    use crate::search::context::{ContentMode, ContextAssembler, ContextConfig};
    use crate::storage::{MetadataStore, VectorStore};

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

    let assembler = ContextAssembler::new(embedder, vector_store, metadata_store, context_config);
    let bundle = assembler
        .assemble(prompt, None)
        .await
        .context("Context assembly failed")?;

    // 7. Output context (only if we found something)
    if bundle.files.is_empty() {
        return Ok(());
    }

    let context_text = format_context_for_injection(&bundle, threshold);
    print!("{}", context_text);

    Ok(())
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
    use crate::types::{ChunkType, MatchType};

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
            },
        };
        let json = serde_json::to_string(&output).unwrap();
        assert!(json.contains("\"threshold\":0.5"));
        assert!(json.contains("\"budget\":150"));
        assert!(json.contains("\"content_mode\":\"preview\""));
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
            },
        };
        let result = format_context_for_injection(&bundle, 0.0);
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
            },
        };
        let result = format_context_for_injection(&bundle, 0.5);
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
            },
        };
        // With high threshold, chunk content should be filtered out
        let result = format_context_for_injection(&bundle, 0.5);
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
            },
        };
        let result = format_context_for_injection(&bundle, 0.0);
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
            },
        };
        let result = format_context_for_injection(&bundle, 0.0);
        // Score should be 2 decimal places
        assert!(result.contains("score 0.86"), "Expected 2-decimal score in: {}", result);
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
}
