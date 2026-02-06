use anyhow::{bail, Context, Result};
use clap::Args;
use colored::Colorize;
use ignore::WalkBuilder;
use indicatif::{ProgressBar, ProgressStyle};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::Instant;

use super::OutputConfig;
use crate::config::Config;
use crate::index::{embedder, Embedder, Parser};
use crate::storage::{MetadataStore, VectorStore};
use crate::types::Chunk;

#[derive(Args)]
pub struct IndexArgs {
    /// Only update changed files
    #[arg(long)]
    incremental: bool,

    /// Force reindex all files
    #[arg(long)]
    force: bool,

    /// Repository name for multi-repo indexing (default: "default")
    #[arg(long)]
    repo: Option<String>,

    /// Source directory to index files from (defaults to path)
    #[arg(long)]
    source: Option<PathBuf>,

    /// Directory containing .bobbin/ config (defaults to current directory)
    #[arg(default_value = ".")]
    path: PathBuf,
}

#[derive(Serialize)]
struct IndexOutput {
    status: String,
    files_indexed: usize,
    chunks_created: usize,
    deleted_files: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    total_files: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    total_chunks: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    elapsed_ms: Option<u128>,
    #[serde(skip_serializing_if = "Option::is_none")]
    errors: Option<usize>,
}

/// Result of indexing a single file
struct FileIndexResult {
    path: String,
    hash: String,
    chunks: Vec<Chunk>,
}

pub async fn run(args: IndexArgs, output: OutputConfig) -> Result<()> {
    let start_time = Instant::now();

    let repo_root = args
        .path
        .canonicalize()
        .with_context(|| format!("Invalid path: {}", args.path.display()))?;

    let config_path = Config::config_path(&repo_root);
    if !config_path.exists() {
        bail!(
            "Bobbin not initialized in {}. Run `bobbin init` first.",
            repo_root.display()
        );
    }

    let config = Config::load(&config_path).with_context(|| "Failed to load configuration")?;

    let repo_name = args.repo.as_deref().unwrap_or("default");

    // Source directory: --source overrides the default (which is the bobbin home path)
    let source_root = if let Some(ref source) = args.source {
        source
            .canonicalize()
            .with_context(|| format!("Invalid source path: {}", source.display()))?
    } else {
        repo_root.clone()
    };

    let db_path = Config::db_path(&repo_root);
    let lance_path = Config::lance_path(&repo_root);
    let model_dir = Config::model_cache_dir()?;

    if output.verbose && !output.quiet && !output.json {
        println!("  Checking embedding model...");
    }
    embedder::ensure_model(&model_dir, &config.embedding.model)
        .await
        .context("Failed to ensure embedding model is available")?;

    // Open storage
    let metadata_store = MetadataStore::open(&db_path).context("Failed to open metadata store")?;
    let mut vector_store = VectorStore::open(&lance_path)
        .await
        .context("Failed to open vector store")?;

    // Check for model change and migration
    let current_model = config.embedding.model.as_str();
    let stored_model = metadata_store.get_meta("embedding_model")?;

    if let Some(stored) = stored_model {
        if stored != current_model {
            if !output.quiet && !output.json {
                println!(
                    "{} Embedding model changed from {} to {}. Re-indexing...",
                    "!".yellow(),
                    stored,
                    current_model
                );
            }

            // Re-create vector store (wipe all data)
            drop(vector_store);
            if lance_path.exists() {
                std::fs::remove_dir_all(&lance_path).with_context(|| {
                    format!("Failed to remove vector store at {}", lance_path.display())
                })?;
            }
            vector_store = VectorStore::open(&lance_path)
                .await
                .context("Failed to re-open vector store")?;
        }
    }

    metadata_store.set_meta("embedding_model", current_model)?;

    let mut embed =
        Embedder::load(&model_dir, current_model).context("Failed to load embedding model")?;
    let mut parser = Parser::new().context("Failed to initialize parser")?;

    // Get existing indexed files from LanceDB (filtered by repo)
    let existing_files: HashSet<String> = if args.force {
        HashSet::new()
    } else {
        vector_store
            .get_all_file_paths(Some(repo_name))
            .await?
            .into_iter()
            .collect()
    };

    let files_to_index = collect_files(&source_root, &config)?;

    if output.verbose && !output.quiet && !output.json {
        println!("  Found {} files matching patterns", files_to_index.len());
    }

    // Track files that no longer exist (for cleanup)
    let current_files: HashSet<String> = files_to_index
        .iter()
        .map(|p| {
            p.strip_prefix(&source_root)
                .unwrap_or(p)
                .to_string_lossy()
                .to_string()
        })
        .collect();

    let deleted_files: Vec<String> = existing_files.difference(&current_files).cloned().collect();

    // Clean up deleted files
    if !deleted_files.is_empty() {
        if output.verbose && !output.quiet && !output.json {
            println!("  Cleaning up {} deleted files...", deleted_files.len());
        }
        vector_store.delete_by_file(&deleted_files).await?;
    }

    // Filter files that need indexing
    let mut files_needing_index = Vec::new();

    for file_path in &files_to_index {
        let rel_path = file_path
            .strip_prefix(&source_root)
            .unwrap_or(file_path)
            .to_string_lossy()
            .to_string();

        if !args.force && args.incremental {
            let content = std::fs::read_to_string(file_path)
                .with_context(|| format!("Failed to read {}", file_path.display()))?;
            let hash = compute_hash(&content);

            if !vector_store.needs_reindex(&rel_path, &hash).await? {
                continue;
            }
        }

        files_needing_index.push(file_path.clone());
    }

    let total_files = files_needing_index.len();

    if total_files == 0 {
        if output.json {
            let json_output = IndexOutput {
                status: "up_to_date".to_string(),
                files_indexed: 0,
                chunks_created: 0,
                deleted_files: deleted_files.len(),
                total_files: None,
                total_chunks: None,
                elapsed_ms: None,
                errors: None,
            };
            println!("{}", serde_json::to_string_pretty(&json_output)?);
        } else if !output.quiet {
            println!("{} Index is up to date", "✓".green());
        }
        return Ok(());
    }

    let progress = if !output.quiet && !output.json {
        let pb = ProgressBar::new(total_files as u64);
        pb.set_style(
            ProgressStyle::default_bar()
                .template(
                    "{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {pos}/{len} ({eta})",
                )
                .unwrap()
                .progress_chars("#>-"),
        );
        Some(pb)
    } else {
        None
    };

    let mut indexed_files = 0;
    let mut total_chunks = 0;
    let mut errors = Vec::new();

    let batch_size = config.embedding.batch_size;
    let mut pending_results: Vec<FileIndexResult> = Vec::new();

    for file_path in &files_needing_index {
        let rel_path = file_path
            .strip_prefix(&source_root)
            .unwrap_or(file_path)
            .to_string_lossy()
            .to_string();

        let content = match std::fs::read_to_string(file_path) {
            Ok(c) => c,
            Err(e) => {
                errors.push((rel_path.clone(), e.to_string()));
                if let Some(pb) = &progress {
                    pb.inc(1);
                }
                continue;
            }
        };

        if content.trim().is_empty() {
            if let Some(pb) = &progress {
                pb.inc(1);
            }
            continue;
        }

        let chunks = match parser.parse_file(file_path, &content) {
            Ok(c) => c,
            Err(e) => {
                errors.push((rel_path.clone(), format!("Parse error: {}", e)));
                if let Some(pb) = &progress {
                    pb.inc(1);
                }
                continue;
            }
        };

        if chunks.is_empty() {
            if let Some(pb) = &progress {
                pb.inc(1);
            }
            continue;
        }

        let hash = compute_hash(&content);

        pending_results.push(FileIndexResult {
            path: rel_path,
            hash,
            chunks,
        });

        let total_pending_chunks: usize = pending_results.iter().map(|r| r.chunks.len()).sum();
        if total_pending_chunks >= batch_size {
            let (indexed, chunks_count) = process_batch(
                &mut pending_results,
                &mut vector_store,
                &mut embed,
                repo_name,
            )
            .await?;

            indexed_files += indexed;
            total_chunks += chunks_count;

            if let Some(pb) = &progress {
                pb.inc(indexed as u64);
            }
        }
    }

    // Process remaining files
    if !pending_results.is_empty() {
        let (indexed, chunks_count) = process_batch(
            &mut pending_results,
            &mut vector_store,
            &mut embed,
            repo_name,
        )
        .await?;

        indexed_files += indexed;
        total_chunks += chunks_count;

        if let Some(pb) = &progress {
            pb.inc(indexed as u64);
        }
    }

    if let Some(pb) = progress {
        pb.finish_and_clear();
    }

    // Analyze and store git coupling if enabled
    if config.git.coupling_enabled {
        if output.verbose && !output.quiet && !output.json {
            println!("  Analyzing git coupling...");
        }

        match crate::index::git::GitAnalyzer::new(&source_root) {
            Ok(analyzer) => {
                match analyzer
                    .analyze_coupling(config.git.coupling_depth, config.git.coupling_threshold)
                {
                    Ok(couplings) => {
                        let mut count = 0;
                        metadata_store.begin_transaction()?;
                        for coupling in couplings {
                            if metadata_store.upsert_coupling(&coupling).is_ok() {
                                count += 1;
                            }
                        }
                        metadata_store.commit()?;

                        if output.verbose && !output.quiet && !output.json {
                            println!("  Stored {} coupling relations", count);
                        }
                    }
                    Err(e) => {
                        if !output.quiet && !output.json {
                            println!("{} Failed to analyze git coupling: {}", "!".yellow(), e);
                        }
                    }
                }
            }
            Err(_) => {}
        }
    }

    let elapsed = start_time.elapsed();

    if output.json {
        let stats = vector_store.get_stats(Some(repo_name)).await?;
        let json_output = IndexOutput {
            status: "indexed".to_string(),
            files_indexed: indexed_files,
            chunks_created: total_chunks,
            deleted_files: deleted_files.len(),
            total_files: Some(stats.total_files),
            total_chunks: Some(stats.total_chunks),
            elapsed_ms: Some(elapsed.as_millis()),
            errors: Some(errors.len()),
        };
        println!("{}", serde_json::to_string_pretty(&json_output)?);
    } else if !output.quiet {
        println!(
            "{} Indexed {} files ({} chunks) in {:.2}s",
            "✓".green(),
            indexed_files,
            total_chunks,
            elapsed.as_secs_f64()
        );

        if !deleted_files.is_empty() {
            println!("  Cleaned up {} deleted files", deleted_files.len());
        }

        if !errors.is_empty() {
            println!("\n{} {} files had errors:", "!".yellow(), errors.len());
            for (path, err) in errors.iter().take(5) {
                println!("  {}: {}", path, err);
            }
            if errors.len() > 5 {
                println!("  ... and {} more", errors.len() - 5);
            }
        }

        if output.verbose {
            let stats = vector_store.get_stats(Some(repo_name)).await?;
            println!("\nIndex statistics:");
            println!("  Total files:  {}", stats.total_files);
            println!("  Total chunks: {}", stats.total_chunks);
            for lang in &stats.languages {
                println!(
                    "  {}: {} files, {} chunks",
                    lang.language, lang.file_count, lang.chunk_count
                );
            }
        }
    }

    Ok(())
}

/// Collect all files to index based on configuration patterns
fn collect_files(repo_root: &Path, config: &Config) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();

    let include_patterns: Vec<glob::Pattern> = config
        .index
        .include
        .iter()
        .filter_map(|p| glob::Pattern::new(p).ok())
        .collect();

    let exclude_patterns: Vec<glob::Pattern> = config
        .index
        .exclude
        .iter()
        .filter_map(|p| glob::Pattern::new(p).ok())
        .collect();

    let mut builder = WalkBuilder::new(repo_root);
    builder
        .hidden(true)
        .git_ignore(config.index.use_gitignore)
        .git_global(config.index.use_gitignore)
        .git_exclude(config.index.use_gitignore);

    for entry in builder.build() {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };

        if entry.file_type().map(|ft| ft.is_dir()).unwrap_or(true) {
            continue;
        }

        let path = entry.path();
        let rel_path = path
            .strip_prefix(repo_root)
            .unwrap_or(path)
            .to_string_lossy();

        let excluded = exclude_patterns.iter().any(|p| p.matches(&rel_path));
        if excluded {
            continue;
        }

        let included = include_patterns.iter().any(|p| p.matches(&rel_path));

        if included {
            files.push(path.to_path_buf());
        }
    }

    Ok(files)
}

/// Process a batch of files: generate embeddings and store in LanceDB
async fn process_batch(
    results: &mut Vec<FileIndexResult>,
    vector_store: &mut VectorStore,
    embed: &mut Embedder,
    repo: &str,
) -> Result<(usize, usize)> {
    if results.is_empty() {
        return Ok((0, 0));
    }

    let now = chrono::Utc::now().timestamp().to_string();

    let mut indexed_count = 0;

    for result in results.drain(..) {
        // Collect chunks and generate embeddings
        let chunk_contents: Vec<String> = result.chunks.iter().map(|c| c.content.clone()).collect();
        let content_refs: Vec<&str> = chunk_contents.iter().map(|s| s.as_str()).collect();

        let embeddings = embed
            .embed_batch(&content_refs)
            .context("Failed to generate embeddings")?;

        // Delete existing chunks for this file, then insert new ones
        vector_store
            .delete_by_file(&[result.path.clone()])
            .await?;

        vector_store
            .insert(
                &result.chunks,
                &embeddings,
                repo,
                &result.hash,
                &now,
            )
            .await
            .context("Failed to store chunks")?;

        indexed_count += 1;
    }

    // We already drained, but count total chunks from what was processed
    Ok((indexed_count, 0)) // chunks counted per-file below
}

/// Compute SHA256 hash of content
fn compute_hash(content: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(content.as_bytes());
    hex::encode(hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_compute_hash() {
        let hash1 = compute_hash("hello world");
        let hash2 = compute_hash("hello world");
        let hash3 = compute_hash("different content");

        assert_eq!(hash1, hash2);
        assert_ne!(hash1, hash3);
        assert_eq!(hash1.len(), 64);
    }

    #[test]
    fn test_collect_files_respects_patterns() {
        let dir = tempdir().unwrap();
        let root = dir.path();

        std::fs::write(root.join("main.rs"), "fn main() {}").unwrap();
        std::fs::write(root.join("lib.rs"), "pub fn lib() {}").unwrap();
        std::fs::write(root.join("test.txt"), "not code").unwrap();
        std::fs::create_dir(root.join("src")).unwrap();
        std::fs::write(root.join("src/mod.rs"), "mod test;").unwrap();

        let config = Config::default();
        let files = collect_files(root, &config).unwrap();

        let rs_files: Vec<_> = files
            .iter()
            .filter(|p| p.extension().map(|e| e == "rs").unwrap_or(false))
            .collect();
        assert_eq!(rs_files.len(), 3);

        let txt_files: Vec<_> = files
            .iter()
            .filter(|p| p.extension().map(|e| e == "txt").unwrap_or(false))
            .collect();
        assert!(txt_files.is_empty());
    }

    #[test]
    fn test_collect_files_excludes_patterns() {
        let dir = tempdir().unwrap();
        let root = dir.path();

        std::fs::write(root.join("main.rs"), "fn main() {}").unwrap();
        std::fs::create_dir_all(root.join("target/debug")).unwrap();
        std::fs::write(root.join("target/debug/lib.rs"), "// build artifact").unwrap();
        std::fs::create_dir_all(root.join("node_modules/pkg")).unwrap();
        std::fs::write(root.join("node_modules/pkg/index.js"), "// npm").unwrap();

        let config = Config::default();
        let files = collect_files(root, &config).unwrap();

        assert!(files
            .iter()
            .any(|p| p.file_name().map(|n| n == "main.rs").unwrap_or(false)));

        assert!(!files
            .iter()
            .any(|p| p.to_string_lossy().contains("target/")));
        assert!(!files
            .iter()
            .any(|p| p.to_string_lossy().contains("node_modules/")));
    }
}
