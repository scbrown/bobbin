use anyhow::{bail, Context, Result};
use clap::Args;
use colored::Colorize;
use serde::Serialize;
use std::path::PathBuf;

use super::OutputConfig;
use crate::config::Config;
use crate::index::Embedder;
use crate::search::{HybridSearch, SemanticSearch};
use crate::storage::{MetadataStore, VectorStore};
use crate::types::{ChunkType, MatchType, SearchResult};

#[derive(Args)]
pub struct LogArgs {
    /// Natural language search query (e.g. "refactored error handling", "added auth")
    query: Option<String>,

    /// Filter by commit author name (substring match)
    #[arg(long)]
    author: Option<String>,

    /// Filter to commits touching a specific file path (substring match)
    #[arg(long)]
    file: Option<String>,

    /// Maximum number of results
    #[arg(long, short = 'n', default_value = "10")]
    limit: usize,

    /// Search mode: hybrid (default), semantic, or keyword
    #[arg(long, short = 'm', default_value = "hybrid")]
    mode: LogSearchMode,

    /// Directory to search in (defaults to current directory)
    #[arg(default_value = ".")]
    path: PathBuf,
}

/// Search mode for commit queries
#[derive(Debug, Clone, Copy, Default, clap::ValueEnum)]
pub enum LogSearchMode {
    /// Combine semantic and keyword search using RRF
    #[default]
    Hybrid,
    /// Vector similarity search only
    Semantic,
    /// Full-text keyword search only
    Keyword,
}

#[derive(Serialize)]
struct LogOutput {
    query: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    author_filter: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    file_filter: Option<String>,
    count: usize,
    results: Vec<LogEntry>,
}

#[derive(Serialize)]
struct LogEntry {
    hash: String,
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    author: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    date: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    files: Vec<String>,
    score: f32,
    #[serde(skip_serializing_if = "Option::is_none")]
    match_type: Option<String>,
}

pub async fn run(args: LogArgs, output: OutputConfig) -> Result<()> {
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

    let config = Config::load(&config_path).context("Failed to load configuration")?;
    let lance_path = Config::lance_path(&repo_root);
    let db_path = Config::db_path(&repo_root);
    let model_dir = Config::model_cache_dir()?;

    let mut vector_store = VectorStore::open(&lance_path)
        .await
        .context("Failed to open vector store")?;

    let count = vector_store.count().await?;
    if count == 0 {
        if output.json {
            println!(
                r#"{{"error": "empty_index", "message": "No indexed content. Run `bobbin index` first."}}"#
            );
        } else if !output.quiet {
            println!(
                "{} No indexed content. Run `bobbin index` first.",
                "!".yellow()
            );
        }
        return Ok(());
    }

    let query_str = args.query.as_deref().unwrap_or("*");

    // Fetch more results than needed so we can post-filter by author/file
    let fetch_limit = if args.author.is_some() || args.file.is_some() {
        args.limit * 5
    } else {
        args.limit
    };
    // Commit search: always search 3x to ensure enough commit-type results
    let search_limit = fetch_limit * 3;

    let results = match (args.query.as_ref(), args.mode) {
        (None, _) | (Some(_), LogSearchMode::Keyword) => {
            // No query or keyword mode: use FTS
            vector_store
                .search_fts(query_str, search_limit, None)
                .await
                .context("Keyword search failed")?
        }
        (Some(_), LogSearchMode::Semantic) | (Some(_), LogSearchMode::Hybrid) => {
            let metadata_store =
                MetadataStore::open(&db_path).context("Failed to open metadata store")?;
            let current_model = config.embedding.model.as_str();
            let stored_model = metadata_store.get_meta("embedding_model")?;

            if let Some(stored) = stored_model {
                if stored != current_model {
                    bail!(
                        "Configured embedding model ({}) differs from indexed model ({}). Run `bobbin index` to re-index.",
                        current_model,
                        stored
                    );
                }
            }

            let embedder = Embedder::from_config(&config.embedding, &model_dir)
                .context("Failed to load embedding model")?;

            match args.mode {
                LogSearchMode::Semantic => {
                    let mut search = SemanticSearch::new(embedder, vector_store);
                    search
                        .search(query_str, search_limit, None)
                        .await
                        .context("Semantic search failed")?
                }
                LogSearchMode::Hybrid => {
                    let mut search = HybridSearch::new(
                        embedder,
                        vector_store,
                        config.search.semantic_weight,
                    );
                    search
                        .search(query_str, search_limit, None)
                        .await
                        .context("Hybrid search failed")?
                }
                LogSearchMode::Keyword => unreachable!(),
            }
        }
    };

    // Filter to commit chunks only, then apply author/file filters
    let filtered: Vec<SearchResult> = results
        .into_iter()
        .filter(|r| r.chunk.chunk_type == ChunkType::Commit)
        .filter(|r| {
            if let Some(ref author_filter) = args.author {
                let lower = author_filter.to_lowercase();
                r.chunk
                    .content
                    .to_lowercase()
                    .contains(&format!("author: {}", lower))
                    || r.chunk.content.to_lowercase().contains(&lower)
            } else {
                true
            }
        })
        .filter(|r| {
            if let Some(ref file_filter) = args.file {
                r.chunk.content.to_lowercase().contains(&file_filter.to_lowercase())
            } else {
                true
            }
        })
        .take(args.limit)
        .collect();

    // Parse commit content into structured entries
    let entries: Vec<LogEntry> = filtered
        .iter()
        .map(parse_commit_result)
        .collect();

    if output.json {
        let json_output = LogOutput {
            query: args.query.clone(),
            author_filter: args.author.clone(),
            file_filter: args.file.clone(),
            count: entries.len(),
            results: entries,
        };
        println!("{}", serde_json::to_string_pretty(&json_output)?);
    } else if !output.quiet {
        if entries.is_empty() {
            let query_display = args.query.as_deref().unwrap_or("(all commits)");
            println!(
                "{} No commit results for: {}",
                "!".yellow(),
                query_display.cyan()
            );
            if !config.git.commits_enabled {
                println!(
                    "  {} Commit indexing is disabled. Enable with git.commits_enabled = true",
                    "hint:".dimmed()
                );
            } else {
                println!(
                    "  {} Run `bobbin index` to index commits.",
                    "hint:".dimmed()
                );
            }
            return Ok(());
        }

        let query_display = args.query.as_deref().unwrap_or("(recent commits)");
        println!(
            "{} Found {} commits for: {}",
            "✓".green(),
            entries.len(),
            query_display.cyan()
        );
        println!();

        for (i, entry) in entries.iter().enumerate() {
            let author_display = entry
                .author
                .as_deref()
                .unwrap_or("unknown")
                .green()
                .to_string();
            let date_display = entry
                .date
                .as_deref()
                .unwrap_or("")
                .dimmed()
                .to_string();

            println!(
                "{}. {} {}",
                (i + 1).to_string().bold(),
                entry.hash[..7.min(entry.hash.len())].yellow(),
                entry.message.trim(),
            );
            println!(
                "   {} · {} · score {:.4}",
                author_display, date_display, entry.score
            );

            if !entry.files.is_empty() && output.verbose {
                let max_files = 5;
                for f in entry.files.iter().take(max_files) {
                    println!("   {}", f.dimmed());
                }
                if entry.files.len() > max_files {
                    println!(
                        "   {}",
                        format!("... and {} more files", entry.files.len() - max_files).dimmed()
                    );
                }
            }

            println!();
        }
    }

    Ok(())
}

/// Parse a commit SearchResult's content into a structured LogEntry
fn parse_commit_result(result: &SearchResult) -> LogEntry {
    let chunk = &result.chunk;

    // Extract hash from id (format: "commit:<hash>")
    let hash = chunk
        .id
        .strip_prefix("commit:")
        .unwrap_or(&chunk.id)
        .to_string();

    // Parse structured content: message\n\nAuthor: ...\nDate: ...\n\nFiles changed:\n...
    let content = &chunk.content;
    let mut message = String::new();
    let mut author = None;
    let mut date = None;
    let mut files = Vec::new();
    let mut in_files = false;

    for line in content.lines() {
        if line.starts_with("Author: ") {
            author = Some(line.strip_prefix("Author: ").unwrap().to_string());
        } else if line.starts_with("Date: ") {
            date = Some(line.strip_prefix("Date: ").unwrap().to_string());
        } else if line == "Files changed:" {
            in_files = true;
        } else if in_files && !line.is_empty() {
            files.push(line.to_string());
        } else if !in_files && author.is_none() && !line.is_empty() {
            if !message.is_empty() {
                message.push(' ');
            }
            message.push_str(line);
        }
    }

    // Fallback: use the name field (truncated message)
    if message.is_empty() {
        message = chunk.name.clone().unwrap_or_default();
    }

    let match_type = result.match_type.map(|mt| match mt {
        MatchType::Semantic => "semantic".to_string(),
        MatchType::Keyword => "keyword".to_string(),
        MatchType::Hybrid => "hybrid".to_string(),
    });

    LogEntry {
        hash,
        message,
        author,
        date,
        files,
        score: result.score,
        match_type,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Chunk;

    fn make_commit_result(id: &str, content: &str, score: f32) -> SearchResult {
        SearchResult {
            chunk: Chunk {
                id: id.to_string(),
                file_path: format!("git:{}", &id[7..14.min(id.len())]),
                chunk_type: ChunkType::Commit,
                name: Some("test commit".to_string()),
                start_line: 0,
                end_line: 0,
                content: content.to_string(),
                language: "git".to_string(),
            },
            score,
            match_type: Some(MatchType::Semantic),
        }
    }

    #[test]
    fn test_parse_commit_result_full() {
        let content = "feat: add user auth\n\nAuthor: Alice\nDate: 2026-01-15\n\nFiles changed:\nsrc/auth.rs\nsrc/main.rs";
        let result = make_commit_result("commit:abc1234", content, 0.85);
        let entry = parse_commit_result(&result);

        assert_eq!(entry.hash, "abc1234");
        assert_eq!(entry.message, "feat: add user auth");
        assert_eq!(entry.author.as_deref(), Some("Alice"));
        assert_eq!(entry.date.as_deref(), Some("2026-01-15"));
        assert_eq!(entry.files, vec!["src/auth.rs", "src/main.rs"]);
        assert_eq!(entry.score, 0.85);
    }

    #[test]
    fn test_parse_commit_result_no_files() {
        let content = "fix: typo\n\nAuthor: Bob\nDate: 2026-02-01";
        let result = make_commit_result("commit:def5678", content, 0.72);
        let entry = parse_commit_result(&result);

        assert_eq!(entry.hash, "def5678");
        assert_eq!(entry.message, "fix: typo");
        assert_eq!(entry.author.as_deref(), Some("Bob"));
        assert!(entry.files.is_empty());
    }

    #[test]
    fn test_parse_commit_result_minimal() {
        let content = "Initial commit";
        let result = make_commit_result("commit:000aaaa", content, 0.5);
        let entry = parse_commit_result(&result);

        assert_eq!(entry.hash, "000aaaa");
        assert_eq!(entry.message, "Initial commit");
        assert!(entry.author.is_none());
        assert!(entry.files.is_empty());
    }
}
