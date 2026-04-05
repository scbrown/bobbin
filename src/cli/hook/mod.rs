use anyhow::Result;
use clap::{Args, Subcommand};
use serde::Serialize;

use super::OutputConfig;

mod format;
mod git_hook;
mod hot_topics;
mod install;
mod ledger;
mod prompt;
mod prompt_remote;
mod session;
mod state;
mod tool;
mod tool_failure;
pub(crate) mod types;
pub(crate) mod util;

#[cfg(test)]
mod tests;

// Re-exports for external consumers (connect.rs)
pub(super) use install::{
    bobbin_hook_entries_with_server, merge_hooks_with, read_settings, resolve_settings_path,
    write_settings,
};

#[derive(Args)]
pub struct HookArgs {
    #[command(subcommand)]
    command: HookCommands,
}

#[derive(Subcommand)]
enum HookCommands {
    /// Install Claude Code hooks into settings.json
    Install(install::InstallArgs),

    /// Remove bobbin hooks from Claude Code settings
    Uninstall(install::UninstallArgs),

    /// Show installed hooks and current config values
    Status(install::StatusArgs),

    /// Handle UserPromptSubmit events (internal, called by Claude Code)
    InjectContext(InjectContextArgs),

    /// Handle SessionStart compact events (internal, called by Claude Code)
    SessionContext(SessionContextArgs),

    /// Output bobbin primer + live stats at SessionStart (internal, called by Claude Code)
    PrimeContext(PrimeContextArgs),

    /// Handle PostToolUse events for file edits (internal, called by Claude Code)
    PostToolUse(PostToolUseArgs),

    /// Handle PostToolUseFailure events (internal, called by Claude Code)
    PostToolUseFailure(PostToolUseFailureArgs),

    /// Install a post-commit git hook for automatic indexing
    InstallGitHook(git_hook::InstallGitHookArgs),

    /// Remove the bobbin post-commit git hook
    UninstallGitHook(git_hook::UninstallGitHookArgs),

    /// Generate hot-topics.md from injection frequency data
    HotTopics(HotTopicsArgs),
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

    /// Injection output format: standard, minimal, verbose, or xml (overrides config)
    #[arg(long)]
    format_mode: Option<String>,
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
struct PostToolUseArgs {
    /// Maximum lines of context (overrides config)
    #[arg(long)]
    budget: Option<usize>,
}

#[derive(Args)]
struct PostToolUseFailureArgs {
    /// Maximum lines of context (overrides config)
    #[arg(long)]
    budget: Option<usize>,
}

#[derive(Args)]
struct HotTopicsArgs {
    /// Regenerate even if injection count hasn't reached threshold
    #[arg(long)]
    force: bool,

    /// Directory to operate on (defaults to current directory)
    #[arg(default_value = ".")]
    path: std::path::PathBuf,
}

#[derive(Serialize)]
pub(super) struct HookStatusOutput {
    pub(super) hooks_installed: bool,
    pub(super) git_hook_installed: bool,
    pub(super) config: HookConfigOutput,
    pub(super) injection_count: u64,
    pub(super) last_injection_time: Option<String>,
    pub(super) last_session_id: Option<String>,
}

#[derive(Serialize)]
pub(super) struct HookConfigOutput {
    pub(super) threshold: f32,
    pub(super) budget: usize,
    pub(super) content_mode: String,
    pub(super) min_prompt_length: usize,
    pub(super) gate_threshold: f32,
    pub(super) dedup_enabled: bool,
}

pub async fn run(args: HookArgs, output: OutputConfig) -> Result<()> {
    match args.command {
        HookCommands::Install(a) => install::run_install(a, output).await,
        HookCommands::Uninstall(a) => install::run_uninstall(a, output).await,
        HookCommands::Status(a) => install::run_status(a, output).await,
        HookCommands::InjectContext(a) => prompt::run_inject_context(a, output).await,
        HookCommands::SessionContext(a) => session::run_session_context(a, output).await,
        HookCommands::PrimeContext(a) => session::run_prime_context(a, output).await,
        HookCommands::PostToolUse(a) => tool::run_post_tool_use(a, output).await,
        HookCommands::PostToolUseFailure(a) => tool_failure::run_post_tool_use_failure(a, output).await,
        HookCommands::InstallGitHook(a) => git_hook::run_install_git_hook(a, output).await,
        HookCommands::UninstallGitHook(a) => git_hook::run_uninstall_git_hook(a, output).await,
        HookCommands::HotTopics(a) => hot_topics::run_hot_topics(a, output).await,
    }
}
