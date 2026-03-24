use anyhow::{bail, Context, Result};
use clap::Args;
use colored::Colorize;
use serde::Serialize;

use super::OutputConfig;
use crate::config::Config;
use crate::http::client::Client;

#[derive(Args)]
pub struct ConnectArgs {
    /// Bobbin server URL (e.g. http://search.svc)
    url: String,

    /// Save to global config (~/.config/bobbin/config.toml) instead of repo-local
    #[arg(long, short)]
    global: bool,

    /// Install Claude Code hooks after connecting
    #[arg(long, default_value_t = true)]
    hooks: bool,

    /// Skip hooks installation
    #[arg(long)]
    no_hooks: bool,

    /// Install hooks globally (into ~/.claude/settings.json)
    #[arg(long)]
    global_hooks: bool,

    /// Skip server connectivity check
    #[arg(long)]
    no_verify: bool,
}

#[derive(Serialize)]
struct ConnectOutput {
    status: String,
    server_url: String,
    config_path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    server_status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    repos: Option<Vec<RepoInfo>>,
    hooks_installed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    hooks_path: Option<String>,
}

#[derive(Serialize)]
struct RepoInfo {
    name: String,
    chunks: u64,
    files: u64,
}

pub async fn run(args: ConnectArgs, output: OutputConfig) -> Result<()> {
    let url = args.url.trim_end_matches('/').to_string();
    let install_hooks = args.hooks && !args.no_hooks;

    // 1. Verify server connectivity
    let mut server_status = None;
    let mut repos: Option<Vec<RepoInfo>> = None;

    if !args.no_verify {
        if !output.quiet && !output.json {
            eprint!("  Checking server... ");
        }

        let client = Client::new(&url);
        match client.status().await {
            Ok(status) => {
                server_status = Some(status.status.clone());
                // Build repo info from language stats (status gives us aggregate)
                let repo_list: Vec<RepoInfo> = status
                    .index
                    .languages
                    .iter()
                    .map(|l| RepoInfo {
                        name: l.language.clone(),
                        chunks: l.chunk_count,
                        files: l.file_count,
                    })
                    .collect();
                if !repo_list.is_empty() {
                    repos = Some(repo_list);
                }

                if !output.quiet && !output.json {
                    eprintln!(
                        "{} ({} files, {} chunks)",
                        "ok".green(),
                        status.index.total_files,
                        status.index.total_chunks
                    );
                }
            }
            Err(e) => {
                if !output.quiet && !output.json {
                    eprintln!("{}", "failed".red());
                }
                bail!(
                    "Cannot connect to bobbin server at {}: {}\n\
                     Hint: verify the URL is correct and the server is running.",
                    url,
                    e
                );
            }
        }
    }

    // 2. Save server URL to config
    let config_path = if args.global {
        let global_path = Config::global_config_path()
            .ok_or_else(|| anyhow::anyhow!("Cannot determine global config directory"))?;
        let mut config = Config::load_global();
        config.server.url = Some(url.clone());
        // Ensure parent directory exists
        if let Some(parent) = global_path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create config directory: {}", parent.display()))?;
        }
        config.save(&global_path)?;
        global_path
    } else {
        // Repo-local config
        let repo_root = super::find_bobbin_root().ok_or_else(|| {
            anyhow::anyhow!(
                "Not in a bobbin-initialized directory. Use --global to save to global config,\n\
                 or run `bobbin init` first."
            )
        })?;
        let config_path = Config::config_path(&repo_root);
        let mut config = if config_path.exists() {
            Config::load(&config_path).unwrap_or_default()
        } else {
            Config::default()
        };
        config.server.url = Some(url.clone());
        config.save(&config_path)?;
        config_path
    };

    // 3. Install hooks (if requested)
    let mut hooks_installed = false;
    let mut hooks_path = None;

    if install_hooks {
        let hook_global = args.global_hooks || args.global;
        let settings_path = super::hook::resolve_settings_path(hook_global)?;

        let mut settings = super::hook::read_settings(&settings_path)?;
        let bobbin = super::hook::bobbin_hook_entries_with_server(Some(&url));
        super::hook::merge_hooks_with(&mut settings, &bobbin);
        super::hook::write_settings(&settings_path, &settings)?;

        hooks_installed = true;
        hooks_path = Some(settings_path.display().to_string());
    }

    // 4. Output
    if output.json {
        let json_output = ConnectOutput {
            status: "connected".to_string(),
            server_url: url.clone(),
            config_path: config_path.display().to_string(),
            server_status,
            repos,
            hooks_installed,
            hooks_path,
        };
        println!("{}", serde_json::to_string_pretty(&json_output)?);
    } else if !output.quiet {
        println!(
            "{} Connected to bobbin server",
            "✓".green(),
        );
        println!("  Server:  {}", url.cyan());
        println!("  Config:  {}", config_path.display().to_string().dimmed());
        if hooks_installed {
            if let Some(ref path) = hooks_path {
                println!("  Hooks:   {} ({})", "installed".green(), path.dimmed());
            }
        }

        if !install_hooks {
            println!(
                "\n  To install hooks: {}",
                "bobbin hook install".cyan()
            );
        }

        println!(
            "\n  Test it: {}",
            "bobbin search \"hello world\"".cyan()
        );
    }

    Ok(())
}
