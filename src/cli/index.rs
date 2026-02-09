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
use crate::config::{Config, ContextualEmbeddingConfig};
use crate::index::{embedder, resolver, Embedder, Parser};
use crate::storage::{MetadataStore, VectorStore};
use crate::types::{Chunk, ChunkType, ImportDependency, ImportEdge};

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
    imports_total: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    imports_resolved: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    imports_unresolved: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    commits_indexed: Option<usize>,
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
    /// Context-enriched text for each chunk (None = use chunk content directly)
    contexts: Vec<Option<String>>,
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
    embedder::ensure_model_for_config(&model_dir, &config.embedding)
        .await
        .context("Failed to ensure embedding model is available")?;

    // Determine embedding dimension
    let embedding_dim = embedder::resolve_dimension(&config.embedding)?;

    // Open storage
    let metadata_store = MetadataStore::open(&db_path).context("Failed to open metadata store")?;
    let mut vector_store = VectorStore::open_with_dim(&lance_path, embedding_dim as i32)
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
            vector_store = VectorStore::open_with_dim(&lance_path, embedding_dim as i32)
                .await
                .context("Failed to re-open vector store")?;
        }
    }

    metadata_store.set_meta("embedding_model", current_model)?;

    let mut embed = Embedder::from_config(&config.embedding, &model_dir)
        .context("Failed to load embedding model")?;
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
        // Also clear import dependencies for deleted files
        if config.dependencies.enabled {
            for file in &deleted_files {
                metadata_store.clear_file_dependencies(file)?;
            }
        }
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
                imports_total: None,
                imports_resolved: None,
                imports_unresolved: None,
                commits_indexed: None,
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
    let mut all_imports: Vec<ImportEdge> = Vec::new();

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

        // Extract imports from this file (if dependency tracking enabled)
        if config.dependencies.enabled {
            let file_imports = parser.extract_imports(file_path, &content);
            for mut imp in file_imports {
                // Normalize source path to relative
                imp.source_file = rel_path.clone();
                all_imports.push(imp);
            }
        }

        if chunks.is_empty() {
            if let Some(pb) = &progress {
                pb.inc(1);
            }
            continue;
        }

        let hash = compute_hash(&content);

        // Compute contextual embeddings for enabled languages
        let contexts = build_context_windows(&chunks, &content, &config.embedding.context);

        pending_results.push(FileIndexResult {
            path: rel_path,
            hash,
            chunks,
            contexts,
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

    // Analyze and store import dependencies
    let mut dep_count: usize = 0;
    let mut resolved_count: usize = 0;
    if config.dependencies.enabled && !all_imports.is_empty() {
        if output.verbose && !output.quiet && !output.json {
            println!("  Resolving {} import edges...", all_imports.len());
        }

        // Build set of all indexed file paths for resolution
        let all_indexed: HashSet<String> = current_files.clone();
        resolver::resolve_imports(&mut all_imports, &all_indexed, &source_root);

        // Clear old dependencies for re-indexed files and store new ones
        let reindexed_files: Vec<String> = all_imports
            .iter()
            .map(|e| e.source_file.clone())
            .collect::<HashSet<_>>()
            .into_iter()
            .collect();

        metadata_store.begin_transaction()?;
        for file in &reindexed_files {
            metadata_store.clear_file_dependencies(file)?;
        }
        let mut dep_count = 0;
        let mut resolved_count = 0;
        for edge in &all_imports {
            let resolved = edge.resolved_path.is_some();
            let dep = ImportDependency {
                file_a: edge.source_file.clone(),
                file_b: if let Some(ref rp) = edge.resolved_path {
                    rp.clone()
                } else {
                    format!("unresolved:{}", edge.import_specifier)
                },
                dep_type: "import".to_string(),
                import_statement: edge.import_specifier.clone(),
                symbol: None,
                resolved,
            };
            if metadata_store.upsert_dependency(&dep).is_ok() {
                dep_count += 1;
                if resolved {
                    resolved_count += 1;
                }
            }
        }
        metadata_store.commit()?;

        if output.verbose && !output.quiet && !output.json {
            println!(
                "  Stored {} dependency edges ({} resolved)",
                dep_count, resolved_count
            );
        }
    }

    // Index git commits as searchable chunks
    let mut commits_indexed: usize = 0;
    if config.git.commits_enabled {
        if output.verbose && !output.quiet && !output.json {
            println!("  Indexing git commits...");
        }

        match crate::index::git::GitAnalyzer::new(&source_root) {
            Ok(analyzer) => {
                // Check for incremental commit indexing
                let last_commit = metadata_store.get_meta("last_indexed_commit")?;
                let since = if args.force { None } else { last_commit.as_deref() };

                match analyzer.get_commit_log(config.git.commits_depth, since) {
                    Ok(commit_entries) if !commit_entries.is_empty() => {
                        let commit_chunks: Vec<Chunk> = commit_entries
                            .iter()
                            .map(|entry| {
                                // Build rich content: message + metadata + files
                                let files_str = if entry.files.is_empty() {
                                    String::new()
                                } else {
                                    format!("\n\nFiles changed:\n{}", entry.files.join("\n"))
                                };
                                let content = format!(
                                    "{}\n\nAuthor: {}\nDate: {}{}",
                                    entry.message, entry.author, entry.date, files_str
                                );

                                Chunk {
                                    id: format!("commit:{}", entry.hash),
                                    file_path: format!("git:{}", &entry.hash[..7.min(entry.hash.len())]),
                                    chunk_type: ChunkType::Commit,
                                    name: Some(truncate_message(&entry.message, 80)),
                                    start_line: 0,
                                    end_line: 0,
                                    content,
                                    language: "git".to_string(),
                                }
                            })
                            .collect();

                        // Embed commit messages in batches
                        let embed_texts: Vec<String> = commit_chunks
                            .iter()
                            .map(|c| c.content.clone())
                            .collect();
                        let embed_refs: Vec<&str> = embed_texts.iter().map(|s| s.as_str()).collect();

                        match embed.embed_batch(&embed_refs).await {
                            Ok(embeddings) => {
                                let contexts = vec![None; commit_chunks.len()];
                                let now = chrono::Utc::now().timestamp().to_string();

                                // Delete old commit chunks if force re-indexing
                                if args.force {
                                    let old_ids: Vec<String> = commit_chunks
                                        .iter()
                                        .map(|c| c.id.clone())
                                        .collect();
                                    vector_store.delete(&old_ids).await.ok();
                                }

                                if let Err(e) = vector_store
                                    .insert(
                                        &commit_chunks,
                                        &embeddings,
                                        &contexts,
                                        repo_name,
                                        "git-commits",
                                        &now,
                                    )
                                    .await
                                {
                                    if !output.quiet && !output.json {
                                        println!(
                                            "{} Failed to store commit chunks: {}",
                                            "!".yellow(),
                                            e
                                        );
                                    }
                                } else {
                                    commits_indexed = commit_chunks.len();

                                    // Track the latest commit for incremental indexing
                                    if let Some(latest) = commit_entries.first() {
                                        metadata_store
                                            .set_meta("last_indexed_commit", &latest.hash)?;
                                    }

                                    if output.verbose && !output.quiet && !output.json {
                                        println!("  Indexed {} commits", commits_indexed);
                                    }
                                }
                            }
                            Err(e) => {
                                if !output.quiet && !output.json {
                                    println!(
                                        "{} Failed to embed commit messages: {}",
                                        "!".yellow(),
                                        e
                                    );
                                }
                            }
                        }
                    }
                    Ok(_) => {
                        if output.verbose && !output.quiet && !output.json {
                            println!("  No new commits to index");
                        }
                    }
                    Err(e) => {
                        if !output.quiet && !output.json {
                            println!("{} Failed to get commit log: {}", "!".yellow(), e);
                        }
                    }
                }
            }
            Err(_) => {}
        }
    }

    // Compact fragmented lance data after indexing — each file insert creates a
    // new fragment, and compaction merges them for better read performance.
    // Stats queries on heavily fragmented tables return incomplete results.
    if let Err(e) = vector_store.compact().await {
        eprintln!("warning: lance compaction failed: {e:#}");
    }

    let elapsed = start_time.elapsed();

    // Build import stats for output (only if deps were processed)
    let (imports_total, imports_resolved, imports_unresolved) = if dep_count > 0 {
        (
            Some(dep_count),
            Some(resolved_count),
            Some(dep_count - resolved_count),
        )
    } else {
        (None, None, None)
    };

    if output.json {
        let stats = vector_store.get_stats(Some(repo_name)).await?;
        let json_output = IndexOutput {
            status: "indexed".to_string(),
            files_indexed: indexed_files,
            chunks_created: total_chunks,
            deleted_files: deleted_files.len(),
            total_files: Some(stats.total_files),
            total_chunks: Some(stats.total_chunks),
            imports_total,
            imports_resolved,
            imports_unresolved,
            commits_indexed: if commits_indexed > 0 {
                Some(commits_indexed)
            } else {
                None
            },
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

        if commits_indexed > 0 {
            println!("  Commits: {} indexed for semantic search", commits_indexed);
        }

        if dep_count > 0 {
            println!(
                "  Imports: {} total, {} resolved, {} unresolved",
                dep_count,
                resolved_count,
                dep_count - resolved_count
            );
        }

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
    let mut chunks_count = 0;

    for result in results.drain(..) {
        // Build text to embed: use full_context when available, fall back to content
        let embed_texts: Vec<String> = result
            .contexts
            .iter()
            .zip(result.chunks.iter())
            .map(|(ctx, chunk)| {
                ctx.as_ref().cloned().unwrap_or_else(|| chunk.content.clone())
            })
            .collect();
        let embed_refs: Vec<&str> = embed_texts.iter().map(|s| s.as_str()).collect();

        let embeddings = embed
            .embed_batch(&embed_refs)
            .await
            .context("Failed to generate embeddings")?;

        // Delete existing chunks for this file, then insert new ones
        vector_store
            .delete_by_file(&[result.path.clone()])
            .await?;

        vector_store
            .insert(
                &result.chunks,
                &embeddings,
                &result.contexts,
                repo,
                &result.hash,
                &now,
            )
            .await
            .context("Failed to store chunks")?;

        chunks_count += result.chunks.len();
        indexed_count += 1;
    }

    Ok((indexed_count, chunks_count))
}

/// Build context windows for chunks based on contextual embedding config.
///
/// For chunks in enabled languages, extracts N lines before and after the chunk
/// from the file content to create enriched embedding text. Returns `None` for
/// chunks where contextual embedding is disabled (they'll be embedded with their
/// content directly).
pub(crate) fn build_context_windows(
    chunks: &[Chunk],
    file_content: &str,
    config: &ContextualEmbeddingConfig,
) -> Vec<Option<String>> {
    if config.context_lines == 0 || config.enabled_languages.is_empty() {
        return vec![None; chunks.len()];
    }

    let file_lines: Vec<&str> = file_content.lines().collect();
    let n = config.context_lines;

    chunks
        .iter()
        .map(|chunk| {
            if !config.enabled_languages.contains(&chunk.language) {
                return None;
            }

            // start_line and end_line are 1-based
            let start = chunk.start_line as usize;
            let end = chunk.end_line as usize;

            let ctx_start = start.saturating_sub(n).max(1);
            let ctx_end = (end + n).min(file_lines.len());

            // Extract context lines (converting from 1-based to 0-based index)
            let prefix_lines = &file_lines[(ctx_start - 1)..(start - 1).min(file_lines.len())];
            let suffix_lines = if end < file_lines.len() {
                &file_lines[end..ctx_end]
            } else {
                &[]
            };

            // Only produce full_context if it actually adds surrounding lines
            if prefix_lines.is_empty() && suffix_lines.is_empty() {
                return None;
            }

            let mut parts = Vec::new();
            if !prefix_lines.is_empty() {
                parts.push(prefix_lines.join("\n"));
            }
            parts.push(chunk.content.clone());
            if !suffix_lines.is_empty() {
                parts.push(suffix_lines.join("\n"));
            }

            Some(parts.join("\n"))
        })
        .collect()
}

/// Truncate a commit message to max_len, appending "..." if truncated
fn truncate_message(msg: &str, max_len: usize) -> String {
    // Take only the first line (subject line)
    let first_line = msg.lines().next().unwrap_or(msg);
    if first_line.len() <= max_len {
        first_line.to_string()
    } else {
        let truncated: String = first_line.chars().take(max_len - 3).collect();
        format!("{}...", truncated.trim_end())
    }
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
    use crate::types::ChunkType;
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
    fn test_truncate_message_short() {
        assert_eq!(truncate_message("short msg", 80), "short msg");
    }

    #[test]
    fn test_truncate_message_long() {
        let long_msg = "a".repeat(100);
        let result = truncate_message(&long_msg, 20);
        assert_eq!(result.len(), 20); // 17 chars + "..."
        assert!(result.ends_with("..."));
    }

    #[test]
    fn test_truncate_message_multiline() {
        let msg = "First line subject\n\nLong body with details";
        assert_eq!(truncate_message(msg, 80), "First line subject");
    }

    #[test]
    fn test_build_context_windows_enabled_language() {
        let file_content = "line1\nline2\nline3\nline4\nline5\nline6\nline7\nline8\nline9\nline10";
        let chunks = vec![Chunk {
            id: "c1".to_string(),
            file_path: "doc.md".to_string(),
            chunk_type: ChunkType::Section,
            name: Some("Section".to_string()),
            start_line: 4,
            end_line: 6,
            content: "line4\nline5\nline6".to_string(),
            language: "markdown".to_string(),
        }];
        let config = ContextualEmbeddingConfig {
            context_lines: 2,
            enabled_languages: vec!["markdown".to_string()],
        };

        let contexts = build_context_windows(&chunks, file_content, &config);
        assert_eq!(contexts.len(), 1);
        let ctx = contexts[0].as_ref().unwrap();
        // Should include lines 2-3 (prefix), 4-6 (content), 7-8 (suffix)
        assert!(ctx.contains("line2"));
        assert!(ctx.contains("line3"));
        assert!(ctx.contains("line4"));
        assert!(ctx.contains("line5"));
        assert!(ctx.contains("line6"));
        assert!(ctx.contains("line7"));
        assert!(ctx.contains("line8"));
        assert!(!ctx.contains("line1"));
        assert!(!ctx.contains("line9"));
    }

    #[test]
    fn test_build_context_windows_disabled_language() {
        let file_content = "line1\nline2\nline3\nline4\nline5";
        let chunks = vec![Chunk {
            id: "c1".to_string(),
            file_path: "main.rs".to_string(),
            chunk_type: ChunkType::Function,
            name: Some("main".to_string()),
            start_line: 2,
            end_line: 4,
            content: "line2\nline3\nline4".to_string(),
            language: "rust".to_string(),
        }];
        let config = ContextualEmbeddingConfig {
            context_lines: 2,
            enabled_languages: vec!["markdown".to_string()],
        };

        let contexts = build_context_windows(&chunks, file_content, &config);
        assert_eq!(contexts.len(), 1);
        assert!(contexts[0].is_none()); // Rust not enabled
    }

    #[test]
    fn test_build_context_windows_at_file_boundaries() {
        let file_content = "line1\nline2\nline3";
        let chunks = vec![Chunk {
            id: "c1".to_string(),
            file_path: "doc.md".to_string(),
            chunk_type: ChunkType::Section,
            name: Some("All".to_string()),
            start_line: 1,
            end_line: 3,
            content: "line1\nline2\nline3".to_string(),
            language: "markdown".to_string(),
        }];
        let config = ContextualEmbeddingConfig {
            context_lines: 5,
            enabled_languages: vec!["markdown".to_string()],
        };

        let contexts = build_context_windows(&chunks, file_content, &config);
        // No surrounding lines available, should return None
        assert!(contexts[0].is_none());
    }

    #[test]
    fn test_build_context_windows_zero_lines() {
        let file_content = "line1\nline2\nline3";
        let chunks = vec![Chunk {
            id: "c1".to_string(),
            file_path: "doc.md".to_string(),
            chunk_type: ChunkType::Section,
            name: Some("S".to_string()),
            start_line: 2,
            end_line: 2,
            content: "line2".to_string(),
            language: "markdown".to_string(),
        }];
        let config = ContextualEmbeddingConfig {
            context_lines: 0,
            enabled_languages: vec!["markdown".to_string()],
        };

        let contexts = build_context_windows(&chunks, file_content, &config);
        assert!(contexts[0].is_none()); // context_lines=0 disables
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
