use anyhow::{bail, Context, Result};
use clap::Args;
use colored::Colorize;
use serde::Serialize;
use std::path::{Path, PathBuf};

use super::OutputConfig;
use crate::config::Config;
use crate::storage::{MetadataStore, VectorStore};

#[derive(Args)]
pub struct RelatedArgs {
    /// File to find related files for
    file: PathBuf,

    /// Maximum number of results
    #[arg(long, short = 'n', default_value = "10")]
    limit: usize,

    /// Minimum score threshold
    #[arg(long, default_value = "0.0")]
    threshold: f32,
}

#[derive(Serialize)]
struct RelatedOutput {
    file: String,
    related: Vec<RelatedFile>,
}

#[derive(Serialize)]
struct RelatedFile {
    path: String,
    score: f32,
    co_changes: u32,
}

pub async fn run(args: RelatedArgs, output: OutputConfig) -> Result<()> {
    let file_path = args
        .file
        .canonicalize()
        .with_context(|| format!("File not found: {}", args.file.display()))?;

    let repo_root = find_repo_root(&file_path)?;
    let db_path = Config::db_path(&repo_root);
    let lance_path = Config::lance_path(&repo_root);

    // Use VectorStore to verify file exists in index
    let vector_store = VectorStore::open(&lance_path)
        .await
        .context("Failed to open vector store")?;

    // Use MetadataStore for coupling data
    let store = MetadataStore::open(&db_path).context("Failed to open metadata store")?;

    let rel_path = file_path
        .strip_prefix(&repo_root)
        .context("File is not inside the repository")?
        .to_string_lossy()
        .to_string();

    // Verify file exists in index via LanceDB
    if vector_store.get_file(&rel_path).await?.is_none() {
        if output.json {
            println!(
                "{}",
                serde_json::to_string_pretty(&RelatedOutput {
                    file: rel_path,
                    related: vec![],
                })?
            );
        } else {
            bail!("File not found in index: {}", rel_path);
        }
        return Ok(());
    }

    let couplings = store.get_coupling(&rel_path, args.limit)?;

    let related: Vec<RelatedFile> = couplings
        .into_iter()
        .filter(|c| c.score >= args.threshold)
        .map(|c| {
            let other_path = if c.file_a == rel_path {
                c.file_b
            } else {
                c.file_a
            };

            RelatedFile {
                path: other_path,
                score: c.score,
                co_changes: c.co_changes,
            }
        })
        .collect();

    if output.json {
        let json_output = RelatedOutput {
            file: rel_path,
            related,
        };
        println!("{}", serde_json::to_string_pretty(&json_output)?);
    } else {
        println!("Related to {}:", rel_path.cyan());
        if related.is_empty() {
            println!("  No related files found (no shared commit history)");
        } else {
            for (i, file) in related.into_iter().enumerate() {
                println!(
                    "{}. {} (score: {:.2}) - Co-changed {} times",
                    i + 1,
                    file.path,
                    file.score,
                    file.co_changes
                );
            }
        }
    }

    Ok(())
}

/// Find the repository root by looking for .bobbin directory
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
