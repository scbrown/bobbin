//! Command dispatch for the `bobbin` CLI.
//!
//! Split out of `src/cli/mod.rs` (bobbin-aoz). It is one long `match` over
//! every subcommand and was a third of that file, while changing for a reason
//! unrelated to anything else there — a new subcommand, not a change to how
//! the CLI is parsed.

use anyhow::Result;

use super::*;

/// Dispatch a resolved command. This is separated from `Cli::run()` to avoid
/// async recursion when `bobbin run` re-dispatches to the underlying command.
pub(super) async fn dispatch_command(command: Commands, output: OutputConfig) -> Result<()> {
    match command {
        Commands::Init(args) => init::run(args, output).await,
        Commands::Connect(args) => connect::run(args, output).await,
        Commands::Index(args) => index::run(args, output).await,
        Commands::IndexBead(args) => index_bead::run(args, output).await,
        Commands::Calibrate(args) => calibrate::run(args, output).await,
        Commands::Search(args) => search::run(args, output).await,
        Commands::Context(args) => context::run(args, output).await,
        Commands::Deps(args) => deps::run(args, output).await,
        Commands::Feedback(args) => feedback::run(args, output).await,
        Commands::Bead(args) => bead::run(args, output).await,
        Commands::Reconcile(args) => reconcile::run(args, output).await,
        Commands::Predict(args) => predict::run(args, output).await,
        Commands::Grep(args) => grep::run(args, output).await,
        Commands::Refs(args) => refs::run(args, output).await,
        Commands::Related(args) => related::run(args, output).await,
        Commands::Coverage(args) => coverage::run(args, output).await,
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
pub(super) async fn dispatch_external(args: &[String], output: &OutputConfig) -> Result<()> {
    let name = args
        .first()
        .ok_or_else(|| anyhow::anyhow!("No command specified"))?;

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

            let resolved = Cli::try_parse_from(&full_args).map_err(|e| anyhow::anyhow!("{}", e))?;
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
pub(super) fn resolve_server_url(cli_server: Option<String>) -> Option<String> {
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
