mod benchmark;
mod calibrate;
mod completions;
mod context;
mod deps;
mod grep;
mod history;
mod hook;
mod hotspots;
mod impact;
mod index;
mod log;
mod prime;
mod init;
mod refs;
mod related;
mod review;
mod run;
mod search;
mod serve;
mod similar;
mod status;
mod tour;
mod watch;

use anyhow::Result;
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "bobbin")]
#[command(about = "Local-first code context engine")]
#[command(version)]
pub struct Cli {
    #[command(subcommand)]
    command: Commands,

    /// Output in JSON format
    #[arg(long, global = true)]
    json: bool,

    /// Suppress non-essential output
    #[arg(long, global = true)]
    quiet: bool,

    /// Show detailed progress
    #[arg(long, global = true)]
    verbose: bool,

    /// Use a remote bobbin HTTP server instead of local storage
    #[arg(long, global = true, value_name = "URL")]
    server: Option<String>,

    /// Metrics source identity (also reads BOBBIN_METRICS_SOURCE env var)
    #[arg(long, global = true, env = "BOBBIN_METRICS_SOURCE")]
    metrics_source: Option<String>,

    /// Role for access filtering (also reads BOBBIN_ROLE, GT_ROLE, BD_ACTOR env vars)
    #[arg(long, global = true, env = "BOBBIN_ROLE")]
    role: Option<String>,
}

#[derive(Subcommand)]
enum Commands {
    /// Initialize bobbin in the current repository
    Init(init::InitArgs),

    /// Build or update the search index
    Index(index::IndexArgs),

    /// Calibrate search parameters against git history
    Calibrate(calibrate::CalibrateArgs),

    /// Semantic search for code
    Search(search::SearchArgs),

    /// Assemble task-relevant context from search and git history
    Context(context::ContextArgs),

    /// Show import dependencies for a file
    Deps(deps::DepsArgs),

    /// Keyword/regex search
    Grep(grep::GrepArgs),

    /// Find symbol references and list file symbols
    Refs(refs::RefsArgs),

    /// Find files related to a given file
    Related(related::RelatedArgs),

    /// Show commit history for a file
    History(history::HistoryArgs),

    /// Search git commits semantically (find commits by what they did)
    Log(log::LogArgs),

    /// Identify code hotspots (high churn + high complexity)
    Hotspots(hotspots::HotspotsArgs),

    /// Predict which files are affected by a change
    Impact(impact::ImpactArgs),

    /// Assemble review context from a git diff
    Review(review::ReviewArgs),

    /// Find semantically similar code chunks or scan for duplicates
    Similar(similar::SimilarArgs),

    /// Show index status and statistics
    Status(status::StatusArgs),

    /// Start MCP server for AI agent integration
    Serve(serve::ServeArgs),

    /// Benchmark embedding models for comparison
    Benchmark(benchmark::BenchmarkArgs),

    /// Watch for file changes and re-index continuously
    Watch(watch::WatchArgs),

    /// Generate shell completions
    Completions(completions::CompletionsArgs),

    /// Manage Claude Code hooks for automatic context injection
    Hook(hook::HookArgs),

    /// Interactive guided walkthrough of bobbin features
    Tour(tour::TourArgs),

    /// Show LLM-friendly project overview with live stats
    Prime(prime::PrimeArgs),

    /// Execute or manage user-defined convenience commands
    Run(run::RunArgs),
}

impl Commands {
    fn name(&self) -> &'static str {
        match self {
            Commands::Init(_) => "init",
            Commands::Index(_) => "index",
            Commands::Calibrate(_) => "calibrate",
            Commands::Search(_) => "search",
            Commands::Context(_) => "context",
            Commands::Deps(_) => "deps",
            Commands::Grep(_) => "grep",
            Commands::Refs(_) => "refs",
            Commands::Related(_) => "related",
            Commands::History(_) => "history",
            Commands::Log(_) => "log",
            Commands::Hotspots(_) => "hotspots",
            Commands::Impact(_) => "impact",
            Commands::Review(_) => "review",
            Commands::Similar(_) => "similar",
            Commands::Status(_) => "status",
            Commands::Serve(_) => "serve",
            Commands::Benchmark(_) => "benchmark",
            Commands::Watch(_) => "watch",
            Commands::Completions(_) => "completions",
            Commands::Hook(_) => "hook",
            Commands::Tour(_) => "tour",
            Commands::Prime(_) => "prime",
            Commands::Run(_) => "run",
        }
    }
}

impl Cli {
    pub async fn run(self) -> Result<()> {
        let resolved_role = crate::access::RepoFilter::resolve_role(self.role.as_deref());
        let output = OutputConfig {
            json: self.json,
            quiet: self.quiet,
            verbose: self.verbose,
            server: self.server,
            role: resolved_role,
        };

        let metrics_source = self.metrics_source.clone();
        let start = std::time::Instant::now();

        // Resolve `run` commands: either a management op (done) or a re-dispatch
        let (command, output) = match self.command {
            Commands::Run(args) => match run::resolve(args, &output)? {
                run::RunResult::Done => return Ok(()),
                run::RunResult::Execute(resolved_args) => {
                    let resolved = Cli::try_parse_from(&resolved_args)
                        .map_err(|e| anyhow::anyhow!("{}", e))?;
                    let resolved_output = OutputConfig {
                        json: resolved.json,
                        quiet: resolved.quiet,
                        verbose: resolved.verbose,
                        server: resolved.server,
                        role: crate::access::RepoFilter::resolve_role(resolved.role.as_deref()),
                    };
                    (resolved.command, resolved_output)
                }
            },
            cmd => (cmd, output),
        };

        let command_name = command.name();
        let result = dispatch_command(command, output).await;

        // Best-effort metrics emission (don't skip hooks — they emit their own events)
        if command_name != "hook" {
            if let Some(repo_root) = find_bobbin_root() {
                let source = crate::metrics::resolve_source(
                    metrics_source.as_deref(),
                    None,
                );
                let ev = crate::metrics::event(
                    &source,
                    "command",
                    command_name,
                    start.elapsed().as_millis() as u64,
                    serde_json::json!({
                        "success": result.is_ok(),
                    }),
                );
                crate::metrics::emit(&repo_root, &ev);
            }
        }

        result
    }
}

/// Dispatch a resolved command. This is separated from `Cli::run()` to avoid
/// async recursion when `bobbin run` re-dispatches to the underlying command.
async fn dispatch_command(command: Commands, output: OutputConfig) -> Result<()> {
    match command {
        Commands::Init(args) => init::run(args, output).await,
        Commands::Index(args) => index::run(args, output).await,
        Commands::Calibrate(args) => calibrate::run(args, output).await,
        Commands::Search(args) => search::run(args, output).await,
        Commands::Context(args) => context::run(args, output).await,
        Commands::Deps(args) => deps::run(args, output).await,
        Commands::Grep(args) => grep::run(args, output).await,
        Commands::Refs(args) => refs::run(args, output).await,
        Commands::Related(args) => related::run(args, output).await,
        Commands::History(args) => history::run(args, output).await,
        Commands::Log(args) => log::run(args, output).await,
        Commands::Hotspots(args) => hotspots::run(args, output).await,
        Commands::Impact(args) => impact::run(args, output).await,
        Commands::Review(args) => review::run(args, output).await,
        Commands::Similar(args) => similar::run(args, output).await,
        Commands::Status(args) => status::run(args, output).await,
        Commands::Serve(args) => serve::run(args, output).await,
        Commands::Benchmark(args) => benchmark::run(args, output).await,
        Commands::Watch(args) => watch::run(args, output).await,
        Commands::Completions(args) => {
            completions::run(args);
            Ok(())
        }
        Commands::Hook(args) => hook::run(args, output).await,
        Commands::Tour(args) => tour::run(args, output).await,
        Commands::Prime(args) => prime::run(args, output).await,
        // Run commands are resolved before dispatch, so this is unreachable
        Commands::Run(_) => anyhow::bail!("Nested run commands are not supported"),
    }
}

/// Walk up from cwd to find a directory containing `.bobbin/`.
/// Returns None if not found (bobbin not initialized).
fn find_bobbin_root() -> Option<std::path::PathBuf> {
    let mut current = std::env::current_dir().ok()?;
    loop {
        if current.join(".bobbin").is_dir() {
            return Some(current);
        }
        if !current.pop() {
            return None;
        }
    }
}

/// Output configuration passed to all commands
#[derive(Debug, Clone)]
pub struct OutputConfig {
    pub json: bool,
    pub quiet: bool,
    pub verbose: bool,
    /// Remote server URL for thin-client mode
    pub server: Option<String>,
    /// Resolved role for access filtering
    pub role: String,
}
