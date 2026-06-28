use anyhow::{bail, Context, Result};
use clap::Args;
use colored::Colorize;
use serde::Serialize;
use std::path::{Path, PathBuf};

use super::OutputConfig;
use crate::access::RepoFilter;
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
    /// Repo the file lives in — set only for cross-repo coupled files (bo-oqny);
    /// omitted for same-repo results.
    #[serde(skip_serializing_if = "Option::is_none")]
    repo: Option<String>,
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

    let mut related: Vec<RelatedFile> = couplings
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
                repo: None,
            }
        })
        .collect();

    // Cross-repo coupled files (bo-oqny), access-filtered. In a single-repo store
    // resolve the seed's repo from the lone `repo_source:` entry; with several
    // repos the seed repo is ambiguous, so match on path alone (results are still
    // access-filtered). Cross-repo edges only exist when this store indexes a
    // multi-repo group, so this is usually a no-op locally.
    let config = Config::load(&Config::config_path(&repo_root)).unwrap_or_default();
    let filter = RepoFilter::from_config(&config.access, &output.role);
    let seed_repo = single_repo_name(&store);
    let cross = crate::index::cross_repo::related_cross_repo(
        &store,
        seed_repo.as_deref(),
        &rel_path,
        args.limit,
        args.threshold,
        &filter,
    )?;
    related.extend(cross.into_iter().map(|c| RelatedFile {
        path: c.path,
        score: c.score,
        co_changes: c.co_changes,
        repo: Some(c.repo),
    }));
    related.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
    related.truncate(args.limit);

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
                let path_label = match &file.repo {
                    Some(repo) => format!("{} [{}]", file.path, repo.magenta()),
                    None => file.path.clone(),
                };
                println!(
                    "{}. {} (score: {:.2}) - Co-changed {} times",
                    i + 1,
                    path_label,
                    file.score,
                    file.co_changes
                );
            }
        }
    }

    Ok(())
}

/// Resolve the seed file's repo name when the store indexes exactly one repo.
/// Returns `None` for multi-repo stores (ambiguous) or if the registry is empty.
fn single_repo_name(store: &MetadataStore) -> Option<String> {
    let entries = store.get_meta_by_prefix("repo_source:").ok()?;
    if entries.len() == 1 {
        entries[0]
            .0
            .strip_prefix("repo_source:")
            .map(|s| s.to_string())
    } else {
        None
    }
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
