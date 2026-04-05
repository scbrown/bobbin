mod benchmark;
mod bundle;
mod calibrate;
mod completions;
mod connect;
mod context;
mod deps;
mod feedback;
mod grep;
mod history;
mod hook;
mod hotspots;
mod impact;
mod index;
mod log;
mod migrate_bundles;
mod ontology;
mod prime;
mod purge;
mod init;
mod refs;
mod related;
mod review;
mod run;
mod search;
mod serve;
mod similar;
mod status;
mod tag;
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
    #[arg(long, global = true, value_name = "URL", env = "BOBBIN_SERVER")]
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

    /// Connect to a remote bobbin server (configure URL + install hooks)
    Connect(connect::ConnectArgs),

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

    /// Submit, list, and manage feedback on bobbin context injections
    Feedback(feedback::FeedbackArgs),

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

    /// Remove all indexed data for a named repository
    Purge(purge::PurgeArgs),

    /// Show LLM-friendly project overview with live stats
    Prime(prime::PrimeArgs),

    /// Manage chunk tags (list, add rules, remove rules)
    Tag(tag::TagArgs),

    /// Explore context bundles (named, hierarchical knowledge anchors)
    Bundle(bundle::BundleArgs),

    /// Navigate the tag ontology: hierarchy, relationships, and domain concepts
    Ontology(ontology::OntologyArgs),

    /// Migrate [[bundles]] from tags.toml to Quipu knowledge graph
    MigrateBundles(migrate_bundles::MigrateBundlesArgs),

    /// Execute or manage user-defined convenience commands
    Run(run::RunArgs),

    /// Catch-all for dynamic commands (from commands.toml or HTTP /cmd)
    #[command(external_subcommand)]
    External(Vec<String>),
}

impl Commands {
    fn name(&self) -> &'static str {
        match self {
            Commands::Init(_) => "init",
            Commands::Connect(_) => "connect",
            Commands::Index(_) => "index",
            Commands::Calibrate(_) => "calibrate",
            Commands::Search(_) => "search",
            Commands::Context(_) => "context",
            Commands::Deps(_) => "deps",
            Commands::Feedback(_) => "feedback",
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
            Commands::Purge(_) => "purge",
            Commands::Prime(_) => "prime",
            Commands::Tag(_) => "tag",
            Commands::Bundle(_) => "bundle",
            Commands::Ontology(_) => "ontology",
            Commands::MigrateBundles(_) => "migrate-bundles",
            Commands::Run(_) => "run",
            Commands::External(ref args) => {
                // Leak a string so we can return &'static str
                // (only called once per invocation, acceptable)
                if let Some(name) = args.first() {
                    Box::leak(name.clone().into_boxed_str())
                } else {
                    "external"
                }
            }
        }
    }
}

impl Cli {
    pub async fn run(self) -> Result<()> {
        let resolved_role = crate::access::RepoFilter::resolve_role(self.role.as_deref());
        // Resolve server URL: --server flag / BOBBIN_SERVER env > repo config > global config
        let resolved_server = resolve_server_url(self.server);
        let output = OutputConfig {
            json: self.json,
            quiet: self.quiet,
            verbose: self.verbose,
            server: resolved_server,
            role: resolved_role,
        };

        let metrics_source = self.metrics_source.clone();
        let start = std::time::Instant::now();

        // Resolve `run` commands: either a management op (done) or a re-dispatch
        // Resolve `external` commands: try local commands.toml, then HTTP /cmd
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
            Commands::External(ref args) => {
                return dispatch_external(args, &output).await;
            }
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
        Commands::Connect(args) => connect::run(args, output).await,
        Commands::Index(args) => index::run(args, output).await,
        Commands::Calibrate(args) => calibrate::run(args, output).await,
        Commands::Search(args) => search::run(args, output).await,
        Commands::Context(args) => context::run(args, output).await,
        Commands::Deps(args) => deps::run(args, output).await,
        Commands::Feedback(args) => feedback::run(args, output).await,
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
        Commands::Purge(args) => purge::run(args, output).await,
        Commands::Prime(args) => prime::run(args, output).await,
        Commands::Tag(args) => tag::run(args, output).await,
        Commands::Bundle(args) => bundle::run(args, output).await,
        Commands::Ontology(args) => ontology::run(args, output).await,
        Commands::MigrateBundles(args) => migrate_bundles::run(args, output).await,
        // Run commands are resolved before dispatch, so this is unreachable
        Commands::Run(_) => anyhow::bail!("Nested run commands are not supported"),
        // External commands are resolved before dispatch, so this is unreachable
        Commands::External(_) => anyhow::bail!("External command dispatch failed"),
    }
}

/// Dispatch an external (dynamic) subcommand.
///
/// Resolution order:
/// 1. Check local commands.toml (same as `bobbin run <name>`)
/// 2. Check HTTP commands on the server (`/cmd/<name>`)
/// 3. Error with helpful message
async fn dispatch_external(args: &[String], output: &OutputConfig) -> Result<()> {
    let name = args.first().ok_or_else(|| anyhow::anyhow!("No command specified"))?;

    // Parse remaining args as key=value params for HTTP commands
    let kv_params: Vec<(&str, String)> = args[1..]
        .iter()
        .filter_map(|arg| {
            let (k, v) = arg.split_once('=')?;
            Some((k, v.to_string()))
        })
        .collect();

    // 1. Try local commands.toml
    if let Some(repo_root) = find_bobbin_root() {
        let commands = crate::commands::load_commands(&repo_root).unwrap_or_default();
        if let Some(def) = commands.get(name.as_str()) {
            // Re-dispatch through normal clap path (same as `bobbin run`)
            let mut full_args = vec!["bobbin".to_string()];
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
            full_args.push(def.command.clone());
            full_args.extend(def.args.iter().cloned());
            // Pass through user args, translating key=value to --key value
            for arg in &args[1..] {
                if let Some((key, value)) = arg.split_once('=') {
                    // "q=term" is the common shorthand for the positional query
                    if key == "q" {
                        full_args.push(value.to_string());
                    } else {
                        full_args.push(format!("--{}", key));
                        full_args.push(value.to_string());
                    }
                } else {
                    full_args.push(arg.clone());
                }
            }

            let resolved = Cli::try_parse_from(&full_args)
                .map_err(|e| anyhow::anyhow!("{}", e))?;
            let resolved_output = OutputConfig {
                json: resolved.json,
                quiet: resolved.quiet,
                verbose: resolved.verbose,
                server: resolved.server,
                role: crate::access::RepoFilter::resolve_role(resolved.role.as_deref()),
            };
            return dispatch_command(resolved.command, resolved_output).await;
        }
    }

    // 2. Try HTTP command on server
    let server_url = output.server.as_ref().ok_or_else(|| {
        anyhow::anyhow!(
            "Unknown command '{}'. Not found in local commands.toml and no server configured.\n\
             Hint: set BOBBIN_SERVER or use --server to enable HTTP commands.",
            name
        )
    })?;

    let client = crate::http::client::Client::new(server_url);

    // Collect key=value params (already parsed above, but we need owned refs)
    let params: Vec<(&str, String)> = kv_params;

    let result = client.invoke_command(name, &params).await?;

    // Pretty-print the JSON response
    if output.json {
        println!("{}", serde_json::to_string(&result)?);
    } else {
        println!("{}", serde_json::to_string_pretty(&result)?);
    }

    Ok(())
}

/// Resolve the effective server URL from multiple sources.
///
/// Priority: cli_server (--server flag / BOBBIN_SERVER env) > repo config > global config.
/// An empty string means "no server" (disables remote, useful for evals/local-only).
fn resolve_server_url(cli_server: Option<String>) -> Option<String> {
    use crate::config::Config;

    // 1. CLI flag or BOBBIN_SERVER env (already resolved by clap)
    if let Some(ref url) = cli_server {
        // Empty string = explicit "no server" override
        if url.is_empty() {
            return None;
        }
        return cli_server;
    }

    // 2. Repo-level config [server].url
    if let Some(repo_root) = find_bobbin_root() {
        let config_path = Config::config_path(&repo_root);
        if let Ok(config) = Config::load(&config_path) {
            if let Some(ref url) = config.server.url {
                if url.is_empty() {
                    return None;
                }
                return config.server.url;
            }
        }
    }

    // 3. Global config [server].url
    let global = Config::load_global();
    global.server.url.filter(|u| !u.is_empty())
}

/// Walk up from cwd to find a directory containing `.bobbin/`.
/// Returns None if not found (bobbin not initialized).
pub fn find_bobbin_root() -> Option<std::path::PathBuf> {
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

/// Generate a helpful "not initialized" error message that suggests BOBBIN_SERVER
/// when running in a multi-agent/multi-repo setup.
pub fn not_initialized_error(dir: &std::path::Path) -> String {
    let mut msg = format!(
        "Bobbin not initialized in {}. Run `bobbin init` first.",
        dir.display()
    );
    if std::env::var("BOBBIN_SERVER").is_err() {
        msg.push_str(
            "\n\nHint: If a bobbin server is running elsewhere, set BOBBIN_SERVER=<url> \
             or use --server <url> to connect without local initialization."
        );
    }
    msg
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
