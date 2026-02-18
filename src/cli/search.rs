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
pub struct SearchArgs {
    /// The search query
    query: String,

    /// Filter by chunk type (function, method, class, struct, enum, interface, module, impl, trait, commit)
    #[arg(long, short = 't')]
    r#type: Option<String>,

    /// Maximum number of results
    #[arg(long, short = 'n', default_value = "10")]
    limit: usize,

    /// Search mode: hybrid (default), semantic, or keyword
    #[arg(long, short = 'm', default_value = "hybrid")]
    mode: SearchMode,

    /// Filter results to a specific repository
    #[arg(long, short = 'r')]
    repo: Option<String>,

    /// Directory to search in (defaults to current directory)
    #[arg(default_value = ".")]
    path: PathBuf,
}

/// Search mode for the query
#[derive(Debug, Clone, Copy, Default, clap::ValueEnum)]
pub enum SearchMode {
    /// Combine semantic and keyword search using RRF
    #[default]
    Hybrid,
    /// Vector similarity search only
    Semantic,
    /// Full-text keyword search only
    Keyword,
}

/// JSON output format for search results
#[derive(Serialize)]
struct SearchOutput {
    query: String,
    mode: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    r#type: Option<String>,
    limit: usize,
    count: usize,
    results: Vec<SearchResultOutput>,
}

#[derive(Serialize)]
struct SearchResultOutput {
    file_path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    chunk_type: String,
    start_line: u32,
    end_line: u32,
    score: f32,
    #[serde(skip_serializing_if = "Option::is_none")]
    match_type: Option<String>,
    language: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    content_preview: Option<String>,
}

pub async fn run(args: SearchArgs, output: OutputConfig) -> Result<()> {
    // Thin-client mode: proxy through remote server
    if let Some(ref server_url) = output.server {
        return run_remote(args, output.clone(), server_url).await;
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

    let config = Config::load(&config_path).with_context(|| "Failed to load configuration")?;

    let type_filter = args
        .r#type
        .as_ref()
        .map(|t| parse_chunk_type(t))
        .transpose()?;

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

    // MetadataStore only used for model check
    let metadata_store = MetadataStore::open(&db_path).context("Failed to open metadata store")?;

    let search_limit = if type_filter.is_some() {
        args.limit * 3
    } else {
        args.limit
    };

    let repo_filter = args.repo.as_deref();

    let results = match args.mode {
        SearchMode::Keyword => {
            vector_store
                .search_fts(&args.query, search_limit, repo_filter)
                .await
                .context("Keyword search failed")?
        }
        SearchMode::Semantic | SearchMode::Hybrid => {
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
                SearchMode::Semantic => {
                    let mut search = SemanticSearch::new(embedder, vector_store);
                    search
                        .search(&args.query, search_limit, repo_filter)
                        .await
                        .context("Semantic search failed")?
                }
                SearchMode::Hybrid => {
                    let mut search = HybridSearch::new(
                        embedder,
                        vector_store,
                        config.search.semantic_weight,
                    );
                    search
                        .search(&args.query, search_limit, repo_filter)
                        .await
                        .context("Hybrid search failed")?
                }
                SearchMode::Keyword => unreachable!(),
            }
        }
    };

    let filtered_results: Vec<SearchResult> = if let Some(ref chunk_type) = type_filter {
        results
            .into_iter()
            .filter(|r| &r.chunk.chunk_type == chunk_type)
            .take(args.limit)
            .collect()
    } else {
        results.into_iter().take(args.limit).collect()
    };

    if output.json {
        print_json_output(&args.query, args.mode, &args.r#type, args.limit, &filtered_results)?;
    } else if !output.quiet {
        print_human_output(&args.query, args.mode, &filtered_results, output.verbose);
    }

    Ok(())
}

/// Run search via remote HTTP server (thin-client mode).
async fn run_remote(args: SearchArgs, output: OutputConfig, server_url: &str) -> Result<()> {
    use crate::http::client::Client;

    let client = Client::new(server_url);

    let mode_str = match args.mode {
        SearchMode::Hybrid => "hybrid",
        SearchMode::Semantic => "semantic",
        SearchMode::Keyword => "keyword",
    };

    let resp = client
        .search(
            &args.query,
            mode_str,
            args.r#type.as_deref(),
            args.limit,
            args.repo.as_deref(),
        )
        .await?;

    if output.json {
        let json_output = SearchOutput {
            query: resp.query,
            mode: resp.mode,
            r#type: args.r#type,
            limit: args.limit,
            count: resp.count,
            results: resp
                .results
                .iter()
                .map(|r| SearchResultOutput {
                    file_path: r.file_path.clone(),
                    name: r.name.clone(),
                    chunk_type: r.chunk_type.clone(),
                    start_line: r.start_line,
                    end_line: r.end_line,
                    score: r.score,
                    match_type: r.match_type.clone(),
                    language: r.language.clone(),
                    content_preview: Some(r.content_preview.clone()),
                })
                .collect(),
        };
        println!("{}", serde_json::to_string_pretty(&json_output)?);
    } else if !output.quiet {
        if resp.results.is_empty() {
            println!(
                "{} No results found for: {} (via {})",
                "!".yellow(),
                args.query.cyan(),
                server_url.dimmed()
            );
            return Ok(());
        }

        println!(
            "{} Found {} results for: {} ({}, via {})",
            "✓".green(),
            resp.results.len(),
            args.query.cyan(),
            resp.mode.dimmed(),
            server_url.dimmed()
        );
        println!();

        for (i, result) in resp.results.iter().enumerate() {
            let name_display = result
                .name
                .as_ref()
                .map(|n| format!(" ({})", n.cyan()))
                .unwrap_or_default();

            println!(
                "{}. {}:{}{}",
                (i + 1).to_string().bold(),
                result.file_path.blue(),
                result.start_line,
                name_display
            );

            let match_info = result
                .match_type
                .as_ref()
                .map(|mt| format!(" [{}]", mt).dimmed().to_string())
                .unwrap_or_default();

            println!(
                "   {} {} · lines {}-{} · score {:.4}{}",
                result.chunk_type.magenta(),
                result.language.dimmed(),
                result.start_line,
                result.end_line,
                result.score,
                match_info
            );

            if output.verbose {
                let preview = truncate_content(&result.content_preview, 300);
                for line in preview.lines().take(5) {
                    println!("   {}", line.dimmed());
                }
                if result.content_preview.lines().count() > 5 {
                    println!("   {}", "...".dimmed());
                }
            }

            println!();
        }
    }

    Ok(())
}

fn parse_chunk_type(s: &str) -> Result<ChunkType> {
    match s.to_lowercase().as_str() {
        "function" | "func" | "fn" => Ok(ChunkType::Function),
        "method" => Ok(ChunkType::Method),
        "class" => Ok(ChunkType::Class),
        "struct" => Ok(ChunkType::Struct),
        "enum" => Ok(ChunkType::Enum),
        "interface" => Ok(ChunkType::Interface),
        "module" | "mod" => Ok(ChunkType::Module),
        "impl" => Ok(ChunkType::Impl),
        "trait" => Ok(ChunkType::Trait),
        "doc" | "documentation" => Ok(ChunkType::Doc),
        "section" => Ok(ChunkType::Section),
        "table" => Ok(ChunkType::Table),
        "code_block" | "codeblock" => Ok(ChunkType::CodeBlock),
        "commit" => Ok(ChunkType::Commit),
        "issue" | "bead" => Ok(ChunkType::Issue),
        "other" => Ok(ChunkType::Other),
        _ => bail!(
            "Unknown chunk type '{}'. Valid types: function, method, class, struct, enum, interface, module, impl, trait, doc, section, table, code_block, commit, issue, other",
            s
        ),
    }
}

fn print_json_output(
    query: &str,
    mode: SearchMode,
    type_filter: &Option<String>,
    limit: usize,
    results: &[SearchResult],
) -> Result<()> {
    let mode_str = match mode {
        SearchMode::Hybrid => "hybrid",
        SearchMode::Semantic => "semantic",
        SearchMode::Keyword => "keyword",
    };

    let output = SearchOutput {
        query: query.to_string(),
        mode: mode_str.to_string(),
        r#type: type_filter.clone(),
        limit,
        count: results.len(),
        results: results
            .iter()
            .map(|r| SearchResultOutput {
                file_path: r.chunk.file_path.clone(),
                name: r.chunk.name.clone(),
                chunk_type: r.chunk.chunk_type.to_string(),
                start_line: r.chunk.start_line,
                end_line: r.chunk.end_line,
                score: r.score,
                match_type: r.match_type.map(|mt| match mt {
                    MatchType::Semantic => "semantic".to_string(),
                    MatchType::Keyword => "keyword".to_string(),
                    MatchType::Hybrid => "hybrid".to_string(),
                }),
                language: r.chunk.language.clone(),
                content_preview: Some(truncate_content(&r.chunk.content, 200)),
            })
            .collect(),
    };

    println!("{}", serde_json::to_string_pretty(&output)?);
    Ok(())
}

fn print_human_output(query: &str, mode: SearchMode, results: &[SearchResult], verbose: bool) {
    if results.is_empty() {
        println!("{} No results found for: {}", "!".yellow(), query.cyan());
        return;
    }

    let mode_str = match mode {
        SearchMode::Hybrid => "hybrid",
        SearchMode::Semantic => "semantic",
        SearchMode::Keyword => "keyword",
    };

    println!(
        "{} Found {} results for: {} ({})",
        "✓".green(),
        results.len(),
        query.cyan(),
        mode_str.dimmed()
    );
    println!();

    for (i, result) in results.iter().enumerate() {
        let chunk = &result.chunk;

        let match_info = match (mode, result.match_type) {
            (SearchMode::Hybrid, Some(MatchType::Hybrid)) => " [hybrid]".yellow().to_string(),
            (SearchMode::Hybrid, Some(MatchType::Semantic)) => " [semantic]".dimmed().to_string(),
            (SearchMode::Hybrid, Some(MatchType::Keyword)) => " [keyword]".dimmed().to_string(),
            _ => String::new(),
        };

        if chunk.chunk_type == ChunkType::Commit {
            // Commit-specific display
            let name_display = chunk
                .name
                .as_ref()
                .map(|n| n.cyan().to_string())
                .unwrap_or_default();

            println!(
                "{}. {} {}",
                (i + 1).to_string().bold(),
                chunk.file_path.blue(),
                name_display,
            );

            println!(
                "   {} · score {:.4}{}",
                "commit".magenta(),
                result.score,
                match_info
            );
        } else {
            // Standard code chunk display
            let name_display = chunk
                .name
                .as_ref()
                .map(|n| format!(" ({})", n.cyan()))
                .unwrap_or_default();

            println!(
                "{}. {}:{}{}",
                (i + 1).to_string().bold(),
                chunk.file_path.blue(),
                chunk.start_line,
                name_display
            );

            println!(
                "   {} {} · lines {}-{} · score {:.4}{}",
                chunk.chunk_type.to_string().magenta(),
                chunk.language.dimmed(),
                chunk.start_line,
                chunk.end_line,
                result.score,
                match_info
            );
        }

        if verbose {
            let preview = truncate_content(&chunk.content, 300);
            for line in preview.lines().take(5) {
                println!("   {}", line.dimmed());
            }
            if chunk.content.lines().count() > 5 {
                println!("   {}", "...".dimmed());
            }
        }

        println!();
    }
}

fn truncate_content(content: &str, max_len: usize) -> String {
    if content.len() <= max_len {
        content.to_string()
    } else {
        let truncated: String = content.chars().take(max_len).collect();
        format!("{}...", truncated.trim_end())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_chunk_type_valid() {
        assert_eq!(parse_chunk_type("function").unwrap(), ChunkType::Function);
        assert_eq!(parse_chunk_type("func").unwrap(), ChunkType::Function);
        assert_eq!(parse_chunk_type("fn").unwrap(), ChunkType::Function);
        assert_eq!(parse_chunk_type("method").unwrap(), ChunkType::Method);
        assert_eq!(parse_chunk_type("class").unwrap(), ChunkType::Class);
        assert_eq!(parse_chunk_type("struct").unwrap(), ChunkType::Struct);
        assert_eq!(parse_chunk_type("enum").unwrap(), ChunkType::Enum);
        assert_eq!(parse_chunk_type("interface").unwrap(), ChunkType::Interface);
        assert_eq!(parse_chunk_type("module").unwrap(), ChunkType::Module);
        assert_eq!(parse_chunk_type("mod").unwrap(), ChunkType::Module);
        assert_eq!(parse_chunk_type("impl").unwrap(), ChunkType::Impl);
        assert_eq!(parse_chunk_type("trait").unwrap(), ChunkType::Trait);
        assert_eq!(parse_chunk_type("commit").unwrap(), ChunkType::Commit);
        assert_eq!(parse_chunk_type("other").unwrap(), ChunkType::Other);
    }

    #[test]
    fn test_parse_chunk_type_case_insensitive() {
        assert_eq!(parse_chunk_type("FUNCTION").unwrap(), ChunkType::Function);
        assert_eq!(parse_chunk_type("Function").unwrap(), ChunkType::Function);
        assert_eq!(parse_chunk_type("STRUCT").unwrap(), ChunkType::Struct);
        assert_eq!(parse_chunk_type("Trait").unwrap(), ChunkType::Trait);
    }

    #[test]
    fn test_parse_chunk_type_invalid() {
        assert!(parse_chunk_type("invalid").is_err());
        assert!(parse_chunk_type("").is_err());
        assert!(parse_chunk_type("functon").is_err());
    }

    #[test]
    fn test_truncate_content_short() {
        let content = "short content";
        let result = truncate_content(content, 100);
        assert_eq!(result, "short content");
    }

    #[test]
    fn test_truncate_content_long() {
        let content = "This is a very long piece of content that should be truncated";
        let result = truncate_content(content, 20);
        assert_eq!(result, "This is a very long...");
    }

    #[test]
    fn test_truncate_content_exact() {
        let content = "exact";
        let result = truncate_content(content, 5);
        assert_eq!(result, "exact");
    }

    #[test]
    fn test_truncate_content_unicode() {
        let content = "こんにちは世界";
        let result = truncate_content(content, 3);
        assert_eq!(result, "こんに...");
    }

    #[test]
    fn test_search_output_serialization() {
        use crate::types::MatchType;

        let results = vec![SearchResult {
            chunk: crate::types::Chunk {
                id: "test-id".to_string(),
                file_path: "src/main.rs".to_string(),
                chunk_type: ChunkType::Function,
                name: Some("test_fn".to_string()),
                start_line: 1,
                end_line: 10,
                content: "fn test_fn() {}".to_string(),
                language: "rust".to_string(),
            },
            score: 0.95,
            match_type: Some(MatchType::Semantic),
            indexed_at: None,
        }];

        let output = SearchOutput {
            query: "test query".to_string(),
            mode: "hybrid".to_string(),
            r#type: Some("function".to_string()),
            limit: 10,
            count: 1,
            results: results
                .iter()
                .map(|r| SearchResultOutput {
                    file_path: r.chunk.file_path.clone(),
                    name: r.chunk.name.clone(),
                    chunk_type: r.chunk.chunk_type.to_string(),
                    start_line: r.chunk.start_line,
                    end_line: r.chunk.end_line,
                    score: r.score,
                    match_type: r.match_type.map(|mt| match mt {
                        MatchType::Semantic => "semantic".to_string(),
                        MatchType::Keyword => "keyword".to_string(),
                        MatchType::Hybrid => "hybrid".to_string(),
                    }),
                    language: r.chunk.language.clone(),
                    content_preview: Some(truncate_content(&r.chunk.content, 200)),
                })
                .collect(),
        };

        let json = serde_json::to_string(&output).unwrap();
        assert!(json.contains("\"query\":\"test query\""));
        assert!(json.contains("\"mode\":\"hybrid\""));
        assert!(json.contains("\"file_path\":\"src/main.rs\""));
        assert!(json.contains("\"chunk_type\":\"function\""));
        assert!(json.contains("\"score\":0.95"));
        assert!(json.contains("\"match_type\":\"semantic\""));
    }
}
