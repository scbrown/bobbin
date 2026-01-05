use anyhow::{Context, Result};
use clap::Args;
use colored::Colorize;
use serde::Serialize;
use std::path::PathBuf;

use super::OutputConfig;
use crate::config::Config;
use crate::storage::MetadataStore;
use crate::types::IndexStats;

#[derive(Args)]
pub struct StatusArgs {
    /// Show detailed statistics
    #[arg(long)]
    detailed: bool,

    /// Directory to check status in (defaults to current directory)
    #[arg(default_value = ".")]
    path: PathBuf,
}

#[derive(Serialize)]
struct StatusOutput {
    status: String,
    path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    stats: Option<IndexStats>,
}

pub async fn run(args: StatusArgs, output: OutputConfig) -> Result<()> {
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

    // Load metadata store to get stats
    let db_path = Config::db_path(&repo_root);
    let metadata_store = MetadataStore::open(&db_path).context("Failed to open metadata store")?;

    let stats = metadata_store.get_stats()?;

    if output.json {
        let json_output = StatusOutput {
            status: "ready".to_string(),
            path: data_dir.display().to_string(),
            stats: Some(stats),
        };
        println!("{}", serde_json::to_string_pretty(&json_output)?);
    } else if !output.quiet {
        println!("{} Bobbin status for {}", "✓".green(), repo_root.display());
        println!();
        println!("  Status:       {}", "Ready".green());
        println!("  Total files:  {}", stats.total_files.to_string().cyan());
        println!("  Total chunks: {}", stats.total_chunks.to_string().cyan());

        if let Some(ts) = stats.last_indexed {
            let dt = chrono::DateTime::from_timestamp(ts, 0)
                .map(|t| t.to_rfc3339())
                .unwrap_or_else(|| "Unknown".to_string());
            println!("  Last indexed: {}", dt);
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
