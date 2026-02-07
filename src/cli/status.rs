use anyhow::Context;
use anyhow::Result;
use clap::Args;
use colored::Colorize;
use serde::Serialize;
use std::path::PathBuf;

use super::OutputConfig;
use crate::config::Config;
use crate::storage::{MetadataStore, VectorStore};
use crate::types::IndexStats;

#[derive(Args)]
pub struct StatusArgs {
    /// Show detailed statistics
    #[arg(long)]
    detailed: bool,

    /// Show stats for a specific repository only
    #[arg(long, short = 'r')]
    repo: Option<String>,

    /// Directory to check status in (defaults to current directory)
    #[arg(default_value = ".")]
    path: PathBuf,
}

#[derive(Serialize)]
struct StatusOutput {
    status: String,
    path: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    repos: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    stats: Option<IndexStats>,
}

pub async fn run(args: StatusArgs, output: OutputConfig) -> Result<()> {
    // Thin-client mode: proxy through remote server
    if let Some(ref server_url) = output.server {
        return run_remote(args, output.clone(), server_url).await;
    }

    let repo_root = args
        .path
        .canonicalize()
        .with_context(|| format!("Invalid path: {}", args.path.display()))?;

    let config_path = Config::config_path(&repo_root);
    let data_dir = Config::data_dir(&repo_root);

    if !config_path.exists() {
        if output.json {
            let json_output = StatusOutput {
                status: "not_initialized".to_string(),
                path: repo_root.display().to_string(),
                repos: vec![],
                stats: None,
            };
            println!("{}", serde_json::to_string_pretty(&json_output)?);
        } else if !output.quiet {
            println!(
                "{} Bobbin not initialized in {}",
                "!".yellow(),
                repo_root.display()
            );
            println!("Run `bobbin init` to initialize.");
        }
        return Ok(());
    }

    // Get stats from LanceDB (primary storage)
    let lance_path = Config::lance_path(&repo_root);
    let vector_store = VectorStore::open(&lance_path)
        .await
        .context("Failed to open vector store")?;

    let repos = vector_store.get_all_repos().await?;
    let stats = vector_store.get_stats(args.repo.as_deref()).await?;

    if output.json {
        let json_output = StatusOutput {
            status: "ready".to_string(),
            path: data_dir.display().to_string(),
            repos,
            stats: Some(stats),
        };
        println!("{}", serde_json::to_string_pretty(&json_output)?);
    } else if !output.quiet {
        println!("{} Bobbin status for {}", "✓".green(), repo_root.display());
        println!();
        println!("  Status:       {}", "Ready".green());

        if repos.len() > 1 || (repos.len() == 1 && repos[0] != "default") {
            println!("  Repositories: {}", repos.join(", ").cyan());
        }
        if let Some(ref repo) = args.repo {
            println!("  Showing:      {}", repo.cyan());
        }

        println!("  Total files:  {}", stats.total_files.to_string().cyan());
        println!("  Total chunks: {}", stats.total_chunks.to_string().cyan());

        if let Some(ts) = stats.last_indexed {
            let dt = chrono::DateTime::from_timestamp(ts, 0)
                .map(|t| t.to_rfc3339())
                .unwrap_or_else(|| "Unknown".to_string());
            println!("  Last indexed: {}", dt);
        }

        // Show dependency stats
        let db_path = Config::db_path(&repo_root);
        if let Ok(meta_store) = MetadataStore::open(&db_path) {
            if let Ok((total_deps, resolved_deps)) = meta_store.get_dependency_stats() {
                if total_deps > 0 {
                    println!(
                        "  Dependencies: {} ({} resolved)",
                        total_deps.to_string().cyan(),
                        resolved_deps
                    );
                }
            }
        }

        if args.detailed {
            println!("\n  Languages:");
            for lang in &stats.languages {
                println!(
                    "    {}: {} files, {} chunks",
                    lang.language.blue(),
                    lang.file_count,
                    lang.chunk_count
                );
            }
        }
    }

    Ok(())
}

/// Run status via remote HTTP server (thin-client mode).
async fn run_remote(args: StatusArgs, output: OutputConfig, server_url: &str) -> Result<()> {
    use crate::http::client::Client;

    let client = Client::new(server_url);
    let resp = client.status().await?;

    if output.json {
        let json_output = StatusOutput {
            status: resp.status,
            path: server_url.to_string(),
            repos: vec![],
            stats: Some(IndexStats {
                total_files: resp.index.total_files,
                total_chunks: resp.index.total_chunks,
                total_embeddings: resp.index.total_embeddings,
                languages: resp
                    .index
                    .languages
                    .iter()
                    .map(|l| crate::types::LanguageStats {
                        language: l.language.clone(),
                        file_count: l.file_count,
                        chunk_count: l.chunk_count,
                    })
                    .collect(),
                last_indexed: resp.index.last_indexed,
                index_size_bytes: resp.index.index_size_bytes,
            }),
        };
        println!("{}", serde_json::to_string_pretty(&json_output)?);
    } else if !output.quiet {
        println!(
            "{} Bobbin status via {}",
            "✓".green(),
            server_url
        );
        println!();
        println!("  Status:       {}", resp.status.green());
        println!(
            "  Total files:  {}",
            resp.index.total_files.to_string().cyan()
        );
        println!(
            "  Total chunks: {}",
            resp.index.total_chunks.to_string().cyan()
        );

        if let Some(ts) = resp.index.last_indexed {
            let dt = chrono::DateTime::from_timestamp(ts, 0)
                .map(|t| t.to_rfc3339())
                .unwrap_or_else(|| "Unknown".to_string());
            println!("  Last indexed: {}", dt);
        }

        if args.detailed {
            println!("\n  Languages:");
            for lang in &resp.index.languages {
                println!(
                    "    {}: {} files, {} chunks",
                    lang.language.blue(),
                    lang.file_count,
                    lang.chunk_count
                );
            }
        }
    }

    Ok(())
}
