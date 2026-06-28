use anyhow::{bail, Context, Result};
use clap::Args;
use colored::Colorize;
use serde::Serialize;
use std::path::{Path, PathBuf};

use super::OutputConfig;
use crate::config::Config;
use crate::index::coverage::{derive_coverage, CoverageDirection};
use crate::storage::{MetadataStore, VectorStore};

#[derive(Args)]
pub struct CoverageArgs {
    /// File to map test↔source coverage for (a source file lists its tests; a
    /// test file lists the sources it covers)
    file: PathBuf,

    /// Maximum number of results
    #[arg(long, short = 'n', default_value = "10")]
    limit: usize,

    /// Minimum coupling score threshold
    #[arg(long, default_value = "0.0")]
    threshold: f32,
}

#[derive(Serialize)]
struct CoverageOutput {
    file: String,
    /// "test" when `file` is source (links are tests), "source" when `file` is a test.
    link_kind: String,
    links: Vec<CoverageEntry>,
}

#[derive(Serialize)]
struct CoverageEntry {
    path: String,
    score: f32,
    co_changes: u32,
}

pub async fn run(args: CoverageArgs, output: OutputConfig) -> Result<()> {
    let file_path = args
        .file
        .canonicalize()
        .with_context(|| format!("File not found: {}", args.file.display()))?;

    let repo_root = find_repo_root(&file_path)?;
    let db_path = Config::db_path(&repo_root);
    let lance_path = Config::lance_path(&repo_root);

    // Verify the file is indexed (mirrors `bobbin related`).
    let vector_store = VectorStore::open(&lance_path)
        .await
        .context("Failed to open vector store")?;

    let store = MetadataStore::open(&db_path).context("Failed to open metadata store")?;

    let rel_path = file_path
        .strip_prefix(&repo_root)
        .context("File is not inside the repository")?
        .to_string_lossy()
        .to_string();

    if vector_store.get_file(&rel_path).await?.is_none() {
        if output.json {
            println!(
                "{}",
                serde_json::to_string_pretty(&CoverageOutput {
                    file: rel_path,
                    link_kind: "test".to_string(),
                    links: vec![],
                })?
            );
            return Ok(());
        }
        bail!("File not found in index: {}", rel_path);
    }

    let couplings = store.get_coupling(&rel_path, args.limit)?;
    let (direction, links) = derive_coverage(&rel_path, couplings);

    let entries: Vec<CoverageEntry> = links
        .into_iter()
        .filter(|l| l.score >= args.threshold)
        .map(|l| CoverageEntry {
            path: l.path,
            score: l.score,
            co_changes: l.co_changes,
        })
        .collect();

    if output.json {
        let json_output = CoverageOutput {
            file: rel_path,
            link_kind: direction.link_kind().to_string(),
            links: entries,
        };
        println!("{}", serde_json::to_string_pretty(&json_output)?);
    } else {
        let header = match direction {
            CoverageDirection::TestsForSource => format!("Tests covering {}:", rel_path.cyan()),
            CoverageDirection::SourcesForTest => format!("Sources covered by {}:", rel_path.cyan()),
        };
        println!("{header}");
        if entries.is_empty() {
            let kind = direction.link_kind();
            println!("  No {kind} files found (no shared commit history)");
        } else {
            for (i, entry) in entries.into_iter().enumerate() {
                println!(
                    "{}. {} (score: {:.2}) - Co-changed {} times",
                    i + 1,
                    entry.path,
                    entry.score,
                    entry.co_changes
                );
            }
        }
    }

    Ok(())
}

/// Find the repository root by looking for the bobbin config.
fn find_repo_root(start_path: &Path) -> Result<PathBuf> {
    let mut current = start_path;
    if current.is_file() {
        if let Some(p) = current.parent() {
            current = p;
        }
    }

    loop {
        if Config::config_path(current).exists() {
            return Ok(current.to_path_buf());
        }
        match current.parent() {
            Some(p) => current = p,
            None => break,
        }
    }
    bail!("Bobbin not initialized. Run `bobbin init` first.")
}
