use anyhow::{bail, Context, Result};
use clap::Args;
use serde::Serialize;
use std::path::PathBuf;

use super::OutputConfig;
use crate::config::Config;
use crate::storage::{MetadataStore, VectorStore};

#[derive(Args)]
pub struct PurgeArgs {
    /// Repository name to purge from the index
    #[arg(long)]
    repo: String,

    /// Directory containing .bobbin/ config (defaults to current directory)
    #[arg(default_value = ".")]
    path: PathBuf,

    /// Skip confirmation prompt
    #[arg(long, short = 'y')]
    yes: bool,
}

#[derive(Serialize)]
struct PurgeOutput {
    status: String,
    repo: String,
    chunks_before: u64,
    chunks_after: u64,
}

pub async fn run(args: PurgeArgs, output: OutputConfig) -> Result<()> {
    let repo_root = args
        .path
        .canonicalize()
        .with_context(|| format!("Invalid path: {}", args.path.display()))?;

    let config_path = Config::config_path(&repo_root);
    if !config_path.exists() {
        bail!("{}", super::not_initialized_error(&repo_root));
    }

    let lance_path = Config::lance_path(&repo_root);
    let vector_store = VectorStore::open(&lance_path).await?;

    // Check current chunk count for this repo
    let stats_before = vector_store.get_stats(Some(&args.repo)).await?;
    if stats_before.total_chunks == 0 {
        if output.json {
            let out = PurgeOutput {
                status: "noop".into(),
                repo: args.repo.clone(),
                chunks_before: 0,
                chunks_after: 0,
            };
            println!("{}", serde_json::to_string_pretty(&out)?);
        } else if !output.quiet {
            println!("No chunks found for repo '{}'. Nothing to purge.", args.repo);
        }
        return Ok(());
    }

    if !args.yes && !output.json {
        println!(
            "About to purge {} chunks for repo '{}'.",
            stats_before.total_chunks, args.repo
        );
        print!("Continue? [y/N] ");
        use std::io::Write;
        std::io::stdout().flush()?;
        let mut input = String::new();
        std::io::stdin().read_line(&mut input)?;
        if !input.trim().eq_ignore_ascii_case("y") {
            println!("Aborted.");
            return Ok(());
        }
    }

    if !output.quiet && !output.json {
        println!(
            "Purging {} chunks for repo '{}'...",
            stats_before.total_chunks, args.repo
        );
    }

    vector_store.delete_by_repo(&args.repo).await?;

    // Reset this repo's commit + coupling watermarks. Purge wipes
    // the repo's chunks but does NOT reindex, so a surviving watermark would
    // make the NEXT `bobbin index` (the production incremental path, non-force)
    // ask `git log <watermark>..HEAD` and re-add only NEW commits — leaving the
    // purged commit-history corpus gone while reporting success. Clearing the
    // watermarks forces that next index to rebuild the full history
    // (`since=None` → `git log` → all commits). Best-effort: a purge that
    // succeeded on the chunks must not fail because the metadata db is absent.
    if let Ok(metadata_store) = MetadataStore::open(&Config::db_path(&repo_root)) {
        let _ = metadata_store.delete_meta(&format!("last_indexed_commit:{}", args.repo));
        let _ = metadata_store.delete_meta(&format!("last_coupling_commit:{}", args.repo));
    }

    let stats_after = vector_store.get_stats(Some(&args.repo)).await?;

    if output.json {
        let out = PurgeOutput {
            status: "ok".into(),
            repo: args.repo.clone(),
            chunks_before: stats_before.total_chunks,
            chunks_after: stats_after.total_chunks,
        };
        println!("{}", serde_json::to_string_pretty(&out)?);
    } else if !output.quiet {
        println!(
            "Purged repo '{}': {} → {} chunks",
            args.repo, stats_before.total_chunks, stats_after.total_chunks
        );
    }

    Ok(())
}
