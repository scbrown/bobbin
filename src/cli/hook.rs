use anyhow::{Context, Result};
use clap::{Args, Subcommand};
use colored::Colorize;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use super::OutputConfig;
use crate::config::Config;

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

async fn run_install(_args: InstallArgs, output: OutputConfig) -> Result<()> {
    if !output.quiet {
        eprintln!("bobbin hook install: not yet implemented");
    }
    Ok(())
}

async fn run_uninstall(_args: UninstallArgs, output: OutputConfig) -> Result<()> {
    if !output.quiet {
        eprintln!("bobbin hook uninstall: not yet implemented");
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

    if output.json {
        let status = HookStatusOutput {
            hooks_installed: false, // TODO: check .claude/settings.json
            git_hook_installed: false, // TODO: check .git/hooks/post-commit
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
        println!("  Claude Code hooks: {}", "not installed".yellow());
        println!("  Git post-commit:   {}", "not installed".yellow());
    }

    Ok(())
}

async fn run_inject_context(args: InjectContextArgs, _output: OutputConfig) -> Result<()> {
    // Never block user prompts — any error exits silently
    match inject_context_inner(args).await {
        Ok(()) => Ok(()),
        Err(_) => Ok(()),
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

/// Format a context bundle into a compact text block for Claude.
fn format_context_for_injection(
    bundle: &crate::search::context::ContextBundle,
    threshold: f32,
) -> String {
    use std::fmt::Write;

    let mut out = String::new();
    let _ = writeln!(
        out,
        "Bobbin found {} relevant files ({} chunks, {}/{} budget lines):",
        bundle.summary.total_files,
        bundle.summary.total_chunks,
        bundle.budget.used_lines,
        bundle.budget.max_lines,
    );

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
            let _ = writeln!(
                out,
                "\n--- {}:{}-{}{} ({}, score {:.3}) ---",
                file.path, chunk.start_line, chunk.end_line, name, chunk.chunk_type, chunk.score,
            );
            if let Some(ref content) = chunk.content {
                let _ = write!(out, "{}", content);
                if !content.ends_with('\n') {
                    let _ = writeln!(out);
                }
            }
        }
    }

    out
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

async fn run_session_context(_args: SessionContextArgs, _output: OutputConfig) -> Result<()> {
    // Stub: future implementation reads stdin JSON and recovers context
    // For now, exit silently (never block session start)
    Ok(())
}

async fn run_install_git_hook(_args: InstallGitHookArgs, output: OutputConfig) -> Result<()> {
    if !output.quiet {
        eprintln!("bobbin hook install-git-hook: not yet implemented");
    }
    Ok(())
}

async fn run_uninstall_git_hook(_args: UninstallGitHookArgs, output: OutputConfig) -> Result<()> {
    if !output.quiet {
        eprintln!("bobbin hook uninstall-git-hook: not yet implemented");
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
        assert!(result.contains("score 0.850"));
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
}
