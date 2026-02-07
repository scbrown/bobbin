use anyhow::{bail, Context, Result};
use clap::Args;
use colored::Colorize;
use notify::{Config as NotifyConfig, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use super::index::build_context_windows;
use super::OutputConfig;
use crate::config::Config;
use crate::index::{embedder, Embedder, Parser};
use crate::storage::VectorStore;

#[derive(Args)]
pub struct WatchArgs {
    /// Directory containing .bobbin/ config
    #[arg(default_value = ".")]
    path: PathBuf,

    /// Repository name for multi-repo indexing
    #[arg(long)]
    repo: Option<String>,

    /// Source directory to index from (defaults to path)
    #[arg(long)]
    source: Option<PathBuf>,

    /// Debounce interval in milliseconds
    #[arg(long, default_value_t = 500)]
    debounce_ms: u64,

    /// Write PID to this file for daemon management
    #[arg(long)]
    pid_file: Option<PathBuf>,

    /// Print a systemd service unit to stdout and exit
    #[arg(long)]
    generate_systemd: bool,
}

struct ReindexStats {
    files_indexed: usize,
    chunks_created: usize,
}

pub async fn run(args: WatchArgs, output: OutputConfig) -> Result<()> {
    if args.generate_systemd {
        return print_systemd_unit(&args);
    }

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

    let config = Config::load(&config_path)?;
    let repo_name = args.repo.as_deref().unwrap_or("default");

    let source_root = if let Some(ref source) = args.source {
        source
            .canonicalize()
            .with_context(|| format!("Invalid source path: {}", source.display()))?
    } else {
        repo_root.clone()
    };

    // Write PID file
    if let Some(ref pid_path) = args.pid_file {
        std::fs::write(pid_path, std::process::id().to_string())
            .with_context(|| format!("Failed to write PID file: {}", pid_path.display()))?;
    }

    // Ensure embedding model is available
    let model_dir = Config::model_cache_dir()?;
    embedder::ensure_model(&model_dir, &config.embedding.model).await?;

    // Open storage and load models
    let lance_path = Config::lance_path(&repo_root);
    let mut vector_store = VectorStore::open(&lance_path).await?;
    let mut embed = Embedder::load(&model_dir, &config.embedding.model)?;
    let mut parser = Parser::new()?;

    // Precompile include/exclude patterns
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

    // Set up file watcher
    let (notify_tx, notify_rx) = std::sync::mpsc::channel();
    let mut watcher = RecommendedWatcher::new(notify_tx, NotifyConfig::default())
        .context("Failed to create file watcher")?;
    watcher
        .watch(&source_root, RecursiveMode::Recursive)
        .with_context(|| format!("Failed to watch: {}", source_root.display()))?;

    // Bridge std::sync channel to tokio
    let (event_tx, mut event_rx) = tokio::sync::mpsc::unbounded_channel();
    tokio::task::spawn_blocking(move || {
        for event in notify_rx {
            if event_tx.send(event).is_err() {
                break;
            }
        }
    });

    if !output.quiet {
        println!(
            "{} Watching {} for changes (debounce: {}ms)",
            "~".cyan(),
            source_root.display(),
            args.debounce_ms
        );
        println!("  Press Ctrl+C to stop");
    }

    let debounce = Duration::from_millis(args.debounce_ms);
    let mut pending_changes: HashSet<PathBuf> = HashSet::new();
    let mut pending_deletes: HashSet<PathBuf> = HashSet::new();
    let mut last_event_time = Instant::now();
    let mut check_interval = tokio::time::interval(Duration::from_millis(100));

    let mut sigterm =
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;

    loop {
        tokio::select! {
            biased;

            _ = tokio::signal::ctrl_c() => {
                if !output.quiet {
                    println!("\n{} Stopping watch daemon", "~".cyan());
                }
                break;
            }

            _ = sigterm.recv() => {
                if !output.quiet {
                    println!("\n{} Received SIGTERM, stopping", "~".cyan());
                }
                break;
            }

            event = event_rx.recv() => {
                match event {
                    Some(Ok(ev)) => {
                        for path in &ev.paths {
                            if !matches_patterns(path, &source_root, &include_patterns, &exclude_patterns) {
                                continue;
                            }
                            match ev.kind {
                                EventKind::Create(_) | EventKind::Modify(_) => {
                                    pending_deletes.remove(path);
                                    pending_changes.insert(path.clone());
                                    last_event_time = Instant::now();
                                }
                                EventKind::Remove(_) => {
                                    pending_changes.remove(path);
                                    pending_deletes.insert(path.clone());
                                    last_event_time = Instant::now();
                                }
                                _ => {}
                            }
                        }
                    }
                    Some(Err(e)) => {
                        if !output.quiet {
                            eprintln!("  {} Watch error: {}", "!".yellow(), e);
                        }
                    }
                    None => {
                        bail!("File watcher disconnected");
                    }
                }
            }

            _ = check_interval.tick() => {
                let has_work = !pending_changes.is_empty() || !pending_deletes.is_empty();
                if has_work && last_event_time.elapsed() >= debounce {
                    // Process deletions
                    if !pending_deletes.is_empty() {
                        let del_paths: Vec<String> = pending_deletes
                            .drain()
                            .map(|p| {
                                p.strip_prefix(&source_root)
                                    .unwrap_or(&p)
                                    .to_string_lossy()
                                    .to_string()
                            })
                            .collect();

                        match vector_store.delete_by_file(&del_paths).await {
                            Ok(_) => {
                                if !output.quiet {
                                    for p in &del_paths {
                                        println!("  {} Removed {}", "-".red(), p);
                                    }
                                }
                            }
                            Err(e) => {
                                if !output.quiet {
                                    eprintln!("  {} Delete failed: {}", "!".yellow(), e);
                                }
                            }
                        }
                    }

                    // Process changes
                    if !pending_changes.is_empty() {
                        let paths: Vec<PathBuf> = pending_changes.drain().collect();
                        match reindex_files(
                            &paths,
                            &source_root,
                            &config,
                            repo_name,
                            &mut vector_store,
                            &mut embed,
                            &mut parser,
                            &output,
                        )
                        .await
                        {
                            Ok(stats) if stats.files_indexed > 0 => {
                                if !output.quiet {
                                    println!(
                                        "  {} Re-indexed {} file{} ({} chunks)",
                                        "~".cyan(),
                                        stats.files_indexed,
                                        if stats.files_indexed != 1 { "s" } else { "" },
                                        stats.chunks_created,
                                    );
                                }
                            }
                            Ok(_) => {}
                            Err(e) => {
                                if !output.quiet {
                                    eprintln!("  {} Re-index error: {}", "!".yellow(), e);
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    // Cleanup PID file on exit
    if let Some(ref pid_path) = args.pid_file {
        let _ = std::fs::remove_file(pid_path);
    }

    Ok(())
}

/// Check whether a file path matches the configured include/exclude patterns.
fn matches_patterns(
    path: &Path,
    source_root: &Path,
    include: &[glob::Pattern],
    exclude: &[glob::Pattern],
) -> bool {
    if path.is_dir() {
        return false;
    }

    let rel = path
        .strip_prefix(source_root)
        .unwrap_or(path)
        .to_string_lossy();

    // Always skip internal directories
    if rel.starts_with(".git/")
        || rel.starts_with(".git\\")
        || rel.starts_with(".bobbin/")
        || rel.starts_with(".bobbin\\")
    {
        return false;
    }

    if exclude.iter().any(|p| p.matches(&rel)) {
        return false;
    }

    include.iter().any(|p| p.matches(&rel))
}

/// Re-index a set of changed files.
async fn reindex_files(
    paths: &[PathBuf],
    source_root: &Path,
    config: &Config,
    repo_name: &str,
    vector_store: &mut VectorStore,
    embed: &mut Embedder,
    parser: &mut Parser,
    output: &OutputConfig,
) -> Result<ReindexStats> {
    let mut stats = ReindexStats {
        files_indexed: 0,
        chunks_created: 0,
    };
    let now = chrono::Utc::now().timestamp().to_string();

    for path in paths {
        let rel_path = path
            .strip_prefix(source_root)
            .unwrap_or(path)
            .to_string_lossy()
            .to_string();

        let content = match std::fs::read_to_string(path) {
            Ok(c) => c,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                // File deleted between event and processing
                let _ = vector_store.delete_by_file(&[rel_path]).await;
                continue;
            }
            Err(_) => continue,
        };

        if content.trim().is_empty() {
            continue;
        }

        let hash = compute_hash(&content);
        if !vector_store.needs_reindex(&rel_path, &hash).await? {
            continue;
        }

        let chunks = match parser.parse_file(path, &content) {
            Ok(c) if !c.is_empty() => c,
            _ => continue,
        };

        let contexts = build_context_windows(&chunks, &content, &config.embedding.context);

        let embed_texts: Vec<String> = contexts
            .iter()
            .zip(chunks.iter())
            .map(|(ctx, chunk)| {
                ctx.as_ref()
                    .cloned()
                    .unwrap_or_else(|| chunk.content.clone())
            })
            .collect();
        let embed_refs: Vec<&str> = embed_texts.iter().map(|s| s.as_str()).collect();
        let embeddings = embed
            .embed_batch(&embed_refs)
            .context("Failed to generate embeddings")?;

        vector_store.delete_by_file(&[rel_path.clone()]).await?;
        vector_store
            .insert(
                &chunks,
                &embeddings,
                &contexts,
                repo_name,
                &hash,
                &now,
            )
            .await?;

        stats.chunks_created += chunks.len();
        stats.files_indexed += 1;

        if output.verbose {
            println!("  {} {} ({} chunks)", "+".green(), rel_path, chunks.len());
        }
    }

    Ok(stats)
}

/// Compute SHA256 hash of content.
fn compute_hash(content: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(content.as_bytes());
    hex::encode(hasher.finalize())
}

/// Print a systemd service unit file to stdout.
fn print_systemd_unit(args: &WatchArgs) -> Result<()> {
    let bobbin_path = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("bobbin"));
    let work_dir = args
        .path
        .canonicalize()
        .unwrap_or_else(|_| args.path.clone());

    let mut exec_start = format!("{} watch", bobbin_path.display());
    if let Some(ref repo) = args.repo {
        exec_start.push_str(&format!(" --repo {}", repo));
    }
    if let Some(ref source) = args.source {
        exec_start.push_str(&format!(" --source {}", source.display()));
    }
    exec_start.push_str(&format!(" --debounce-ms {}", args.debounce_ms));
    exec_start.push_str(&format!(" {}", work_dir.display()));

    println!(
        "\
[Unit]
Description=Bobbin Watch - Continuous code indexing
After=local-fs.target

[Service]
Type=simple
ExecStart={exec_start}
WorkingDirectory={work_dir}
Restart=on-failure
RestartSec=5
Environment=RUST_LOG=info

[Install]
WantedBy=default.target",
        exec_start = exec_start,
        work_dir = work_dir.display(),
    );

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_matches_patterns_with_tempdir() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        let include = vec![glob::Pattern::new("**/*.rs").unwrap()];
        let exclude = vec![glob::Pattern::new("**/target/**").unwrap()];

        // Create a file that should match
        let rs_file = root.join("main.rs");
        std::fs::write(&rs_file, "fn main() {}").unwrap();
        assert!(matches_patterns(&rs_file, &root, &include, &exclude));

        // Create a file excluded by pattern
        std::fs::create_dir_all(root.join("target/debug")).unwrap();
        let target_file = root.join("target/debug/lib.rs");
        std::fs::write(&target_file, "// artifact").unwrap();
        assert!(!matches_patterns(&target_file, &root, &include, &exclude));

        // Non-matching extension
        let txt_file = root.join("notes.txt");
        std::fs::write(&txt_file, "notes").unwrap();
        assert!(!matches_patterns(&txt_file, &root, &include, &exclude));
    }

    #[test]
    fn test_compute_hash_deterministic() {
        let h1 = compute_hash("hello world");
        let h2 = compute_hash("hello world");
        let h3 = compute_hash("different");

        assert_eq!(h1, h2);
        assert_ne!(h1, h3);
        assert_eq!(h1.len(), 64);
    }

    #[test]
    fn test_matches_patterns_excludes_internal_dirs() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        let include = vec![glob::Pattern::new("**/*").unwrap()];
        let exclude: Vec<glob::Pattern> = vec![];

        // .git/ paths should always be excluded
        std::fs::create_dir_all(root.join(".git/objects")).unwrap();
        let git_file = root.join(".git/objects/abc");
        std::fs::write(&git_file, "git object").unwrap();
        assert!(!matches_patterns(&git_file, &root, &include, &exclude));

        // .bobbin/ paths should always be excluded
        std::fs::create_dir_all(root.join(".bobbin/vectors")).unwrap();
        let bobbin_file = root.join(".bobbin/vectors/data");
        std::fs::write(&bobbin_file, "lance data").unwrap();
        assert!(!matches_patterns(&bobbin_file, &root, &include, &exclude));
    }
}
