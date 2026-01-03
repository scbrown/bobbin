use anyhow::{bail, Context, Result};
use clap::Args;
use colored::Colorize;
use serde::Serialize;
use std::path::PathBuf;

use super::OutputConfig;
use crate::config::Config;
use crate::index::Embedder;
use crate::search::SemanticSearch;
use crate::storage::{MetadataStore, VectorStore};
use crate::types::{ChunkType, SearchResult};

#[derive(Args)]
pub struct SearchArgs {
    /// The search query
    query: String,

    /// Filter by chunk type (function, method, class, struct, enum, interface, module, impl, trait)
    #[arg(long, short = 't')]
    r#type: Option<String>,

    /// Maximum number of results
    #[arg(long, short = 'n', default_value = "10")]
    limit: usize,

    /// Directory to search in (defaults to current directory)
    #[arg(default_value = ".")]
    path: PathBuf,
}

/// JSON output format for search results
#[derive(Serialize)]
struct SearchOutput {
    query: String,
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
    language: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    content_preview: Option<String>,
}

pub async fn run(args: SearchArgs, output: OutputConfig) -> Result<()> {
    // Resolve the repository root
    let repo_root = args
        .path
        .canonicalize()
        .with_context(|| format!("Invalid path: {}", args.path.display()))?;

    // Check if bobbin is initialized
    let config_path = Config::config_path(&repo_root);
    if !config_path.exists() {
        bail!(
            "Bobbin not initialized in {}. Run `bobbin init` first.",
            repo_root.display()
        );
    }
    
    // Load configuration
    let config = Config::load(&config_path)
        .with_context(|| "Failed to load configuration")?;

    // Parse the type filter if provided
    let type_filter = args.r#type.as_ref().map(|t| parse_chunk_type(t)).transpose()?;

    let lance_path = Config::lance_path(&repo_root);
    let db_path = Config::db_path(&repo_root);
    let model_dir = Config::model_cache_dir()?;

    // Open vector store
    let vector_store = VectorStore::open(&lance_path)
        .await
        .context("Failed to open vector store")?;
    
    // Check if index exists
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
    
    // Check model consistency
    let metadata_store = MetadataStore::open(&db_path)
        .context("Failed to open metadata store")?;
        
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

    // Load the embedder
    let embedder = Embedder::load(&model_dir, current_model).context("Failed to load embedding model")?;

    // Create semantic search engine
    let mut search = SemanticSearch::new(embedder, vector_store);

    // Perform search (request more results if filtering by type)
    let search_limit = if type_filter.is_some() {
        args.limit * 3 // Request more to account for filtering
    } else {
        args.limit
    };

    let results = search
        .search(&args.query, search_limit)
        .await
        .context("Search failed")?;

    // Filter by chunk type if specified
    let filtered_results: Vec<SearchResult> = if let Some(ref chunk_type) = type_filter {
        results
            .into_iter()
            .filter(|r| &r.chunk.chunk_type == chunk_type)
            .take(args.limit)
            .collect()
    } else {
        results.into_iter().take(args.limit).collect()
    };

    // Output results
    if output.json {
        print_json_output(&args.query, &args.r#type, args.limit, &filtered_results)?;
    } else if !output.quiet {
        print_human_output(&args.query, &filtered_results, output.verbose);
    }

    Ok(())
}

/// Parse a chunk type string into ChunkType enum
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
        "other" => Ok(ChunkType::Other),
        _ => bail!(
            "Unknown chunk type '{}'. Valid types: function, method, class, struct, enum, interface, module, impl, trait, doc, other",
            s
        ),
    }
}

/// Print results in JSON format
fn print_json_output(
    query: &str,
    type_filter: &Option<String>,
    limit: usize,
    results: &[SearchResult],
) -> Result<()> {
    let output = SearchOutput {
        query: query.to_string(),
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
                language: r.chunk.language.clone(),
                content_preview: Some(truncate_content(&r.chunk.content, 200)),
            })
            .collect(),
    };

    println!("{}", serde_json::to_string_pretty(&output)?);
    Ok(())
}

/// Print results in human-readable format
fn print_human_output(query: &str, results: &[SearchResult], verbose: bool) {
    if results.is_empty() {
        println!("{} No results found for: {}", "!".yellow(), query.cyan());
        return;
    }

    println!(
        "{} Found {} results for: {}",
        "✓".green(),
        results.len(),
        query.cyan()
    );
    println!();

    for (i, result) in results.iter().enumerate() {
        let chunk = &result.chunk;
        let score_pct = (result.score * 100.0) as u32;

        // Header line with file path and location
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

        // Details line
        println!(
            "   {} {} · lines {}-{} · {}% match",
            chunk.chunk_type.to_string().magenta(),
            chunk.language.dimmed(),
            chunk.start_line,
            chunk.end_line,
            score_pct
        );

        // Content preview (if verbose or for short content)
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

/// Truncate content to a maximum length, adding ellipsis if needed
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
        assert!(parse_chunk_type("functon").is_err()); // typo
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
        // Unicode characters should be handled correctly
        let content = "こんにちは世界"; // "Hello World" in Japanese
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
        }];

        let output = SearchOutput {
            query: "test query".to_string(),
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
                    language: r.chunk.language.clone(),
                    content_preview: Some(truncate_content(&r.chunk.content, 200)),
                })
                .collect(),
        };

        let json = serde_json::to_string(&output).unwrap();
        assert!(json.contains("\"query\":\"test query\""));
        assert!(json.contains("\"file_path\":\"src/main.rs\""));
        assert!(json.contains("\"chunk_type\":\"function\""));
        assert!(json.contains("\"score\":0.95"));
    }
}
