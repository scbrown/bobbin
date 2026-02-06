use anyhow::{bail, Context, Result};
use clap::Args;
use colored::Colorize;
use regex::Regex;
use serde::Serialize;
use std::path::PathBuf;

use super::OutputConfig;
use crate::config::Config;
use crate::storage::VectorStore;
use crate::types::{ChunkType, SearchResult};

#[derive(Args)]
pub struct GrepArgs {
    /// Pattern to search for (supports FTS queries and regex with -E)
    pattern: String,

    /// Case insensitive search
    #[arg(long, short = 'i')]
    ignore_case: bool,

    /// Use extended regex matching (post-filter FTS results)
    #[arg(long, short = 'E')]
    regex: bool,

    /// Filter by chunk type (function, method, class, struct, enum, interface, module, impl, trait)
    #[arg(long, short = 't')]
    r#type: Option<String>,

    /// Maximum number of results
    #[arg(long, short = 'n', default_value = "10")]
    limit: usize,

    /// Number of context lines to show around matches
    #[arg(long, short = 'C', default_value = "0")]
    context: usize,

    /// Directory to search in (defaults to current directory)
    #[arg(default_value = ".")]
    path: PathBuf,
}

/// JSON output format for grep results
#[derive(Serialize)]
struct GrepOutput {
    pattern: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    r#type: Option<String>,
    ignore_case: bool,
    regex: bool,
    limit: usize,
    count: usize,
    results: Vec<GrepResultOutput>,
}

#[derive(Serialize)]
struct GrepResultOutput {
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
    #[serde(skip_serializing_if = "Vec::is_empty")]
    matching_lines: Vec<MatchingLine>,
}

#[derive(Serialize)]
struct MatchingLine {
    line_number: u32,
    content: String,
}

pub async fn run(args: GrepArgs, output: OutputConfig) -> Result<()> {
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

    // Parse the type filter if provided
    let type_filter = args
        .r#type
        .as_ref()
        .map(|t| parse_chunk_type(t))
        .transpose()?;

    // Build the regex pattern if regex mode is enabled
    let regex_pattern = if args.regex {
        let pattern = if args.ignore_case {
            format!("(?i){}", args.pattern)
        } else {
            args.pattern.clone()
        };
        Some(
            Regex::new(&pattern)
                .with_context(|| format!("Invalid regex pattern: {}", args.pattern))?,
        )
    } else {
        None
    };

    // Open vector store
    let lance_path = Config::lance_path(&repo_root);
    let mut vector_store = VectorStore::open(&lance_path)
        .await
        .context("Failed to open vector store")?;

    // Check if index exists
    let stats = vector_store.get_stats().await?;
    if stats.total_chunks == 0 {
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

    // Build FTS query - handle case sensitivity for FTS
    // LanceDB FTS (tantivy) is case-insensitive by default, so for case-sensitive we post-filter
    let fts_query = if args.regex {
        // For regex mode, use the pattern as a simple FTS query to get candidates
        // We'll filter by regex afterward
        extract_fts_terms(&args.pattern)
    } else {
        args.pattern.clone()
    };

    // Request more results if filtering by type or regex (to account for filtering)
    let search_limit = if type_filter.is_some() || args.regex || !args.ignore_case {
        args.limit * 5
    } else {
        args.limit
    };

    // Perform FTS search via LanceDB
    let results = vector_store
        .search_fts(&fts_query, search_limit)
        .await
        .context("FTS search failed")?;

    // Apply filters
    let filtered_results: Vec<SearchResult> = results
        .into_iter()
        // Filter by chunk type
        .filter(|r| {
            if let Some(ref chunk_type) = type_filter {
                &r.chunk.chunk_type == chunk_type
            } else {
                true
            }
        })
        // Filter by regex if enabled
        .filter(|r| {
            if let Some(ref re) = regex_pattern {
                re.is_match(&r.chunk.content)
                    || r.chunk.name.as_ref().is_some_and(|n| re.is_match(n))
            } else {
                true
            }
        })
        // For case-sensitive mode without regex, post-filter
        .filter(|r| {
            if !args.ignore_case && regex_pattern.is_none() {
                r.chunk.content.contains(&args.pattern)
                    || r.chunk
                        .name
                        .as_ref()
                        .is_some_and(|n| n.contains(&args.pattern))
            } else {
                true
            }
        })
        .take(args.limit)
        .collect();

    // Output results
    if output.json {
        print_json_output(&args, &filtered_results, regex_pattern.as_ref())?;
    } else if !output.quiet {
        print_human_output(
            &args,
            &filtered_results,
            regex_pattern.as_ref(),
            output.verbose,
        );
    }

    Ok(())
}

/// Extract FTS search terms from a regex pattern
/// This is a best-effort extraction for getting initial candidates
fn extract_fts_terms(pattern: &str) -> String {
    // Remove common regex metacharacters and extract word-like terms
    let cleaned: String = pattern
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '_' || c == ' ' {
                c
            } else {
                ' '
            }
        })
        .collect();

    // Get unique words that are at least 2 chars
    let words: Vec<&str> = cleaned
        .split_whitespace()
        .filter(|w| w.len() >= 2)
        .collect();

    if words.is_empty() {
        // If no extractable terms, use the original pattern
        pattern.to_string()
    } else {
        // Join with OR for FTS
        words.join(" OR ")
    }
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
        "other" => Ok(ChunkType::Other),
        _ => bail!(
            "Unknown chunk type '{}'. Valid types: function, method, class, struct, enum, interface, module, impl, trait, other",
            s
        ),
    }
}

/// Find lines matching the pattern with context
fn find_matching_lines(
    content: &str,
    pattern: &str,
    regex: Option<&Regex>,
    ignore_case: bool,
    context: usize,
    start_line: u32,
) -> Vec<MatchingLine> {
    let lines: Vec<&str> = content.lines().collect();
    let mut matching_indices = Vec::new();

    for (idx, line) in lines.iter().enumerate() {
        let matches = if let Some(re) = regex {
            re.is_match(line)
        } else if ignore_case {
            line.to_lowercase().contains(&pattern.to_lowercase())
        } else {
            line.contains(pattern)
        };

        if matches {
            matching_indices.push(idx);
        }
    }

    // Collect lines with context, avoiding duplicates
    let mut included: std::collections::HashSet<usize> = std::collections::HashSet::new();
    for &idx in &matching_indices {
        let start = idx.saturating_sub(context);
        let end = (idx + context + 1).min(lines.len());
        for i in start..end {
            included.insert(i);
        }
    }

    let mut result: Vec<MatchingLine> = included
        .into_iter()
        .map(|idx| MatchingLine {
            line_number: start_line + idx as u32,
            content: lines[idx].to_string(),
        })
        .collect();

    result.sort_by_key(|m| m.line_number);
    result
}

/// Print results in JSON format
fn print_json_output(
    args: &GrepArgs,
    results: &[SearchResult],
    regex: Option<&Regex>,
) -> Result<()> {
    let output = GrepOutput {
        pattern: args.pattern.clone(),
        r#type: args.r#type.clone(),
        ignore_case: args.ignore_case,
        regex: args.regex,
        limit: args.limit,
        count: results.len(),
        results: results
            .iter()
            .map(|r| {
                let matching_lines = if args.context > 0 || args.regex {
                    find_matching_lines(
                        &r.chunk.content,
                        &args.pattern,
                        regex,
                        args.ignore_case,
                        args.context,
                        r.chunk.start_line,
                    )
                } else {
                    Vec::new()
                };

                GrepResultOutput {
                    file_path: r.chunk.file_path.clone(),
                    name: r.chunk.name.clone(),
                    chunk_type: r.chunk.chunk_type.to_string(),
                    start_line: r.chunk.start_line,
                    end_line: r.chunk.end_line,
                    score: normalize_bm25_score(r.score),
                    language: r.chunk.language.clone(),
                    content_preview: Some(truncate_content(&r.chunk.content, 200)),
                    matching_lines,
                }
            })
            .collect(),
    };

    println!("{}", serde_json::to_string_pretty(&output)?);
    Ok(())
}

/// Print results in human-readable format
fn print_human_output(
    args: &GrepArgs,
    results: &[SearchResult],
    regex: Option<&Regex>,
    verbose: bool,
) {
    if results.is_empty() {
        println!(
            "{} No results found for: {}",
            "!".yellow(),
            args.pattern.cyan()
        );
        return;
    }

    println!(
        "{} Found {} results for: {}",
        "✓".green(),
        results.len(),
        args.pattern.cyan()
    );
    println!();

    for (i, result) in results.iter().enumerate() {
        let chunk = &result.chunk;
        let score = normalize_bm25_score(result.score);
        let score_pct = (score * 100.0) as u32;

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
            "   {} {} · lines {}-{} · {}% relevance",
            chunk.chunk_type.to_string().magenta(),
            chunk.language.dimmed(),
            chunk.start_line,
            chunk.end_line,
            score_pct
        );

        // Show matching lines with context if requested
        if args.context > 0 || verbose {
            let matching_lines = find_matching_lines(
                &chunk.content,
                &args.pattern,
                regex,
                args.ignore_case,
                if args.context > 0 { args.context } else { 2 },
                chunk.start_line,
            );

            if !matching_lines.is_empty() {
                for ml in matching_lines.iter().take(10) {
                    // Highlight the match in the line
                    let highlighted =
                        highlight_match(&ml.content, &args.pattern, regex, args.ignore_case);
                    println!(
                        "   {}: {}",
                        ml.line_number.to_string().dimmed(),
                        highlighted
                    );
                }
                if matching_lines.len() > 10 {
                    println!(
                        "   {}",
                        format!("... {} more lines", matching_lines.len() - 10).dimmed()
                    );
                }
            }
        }

        println!();
    }
}

/// Highlight matches in a line
fn highlight_match(line: &str, pattern: &str, regex: Option<&Regex>, ignore_case: bool) -> String {
    if let Some(re) = regex {
        // Use regex to find and highlight matches
        re.replace_all(line, |caps: &regex::Captures| {
            format!("{}", caps[0].to_string().red().bold())
        })
        .to_string()
    } else if ignore_case {
        // Case-insensitive highlighting
        let lower_line = line.to_lowercase();
        let lower_pattern = pattern.to_lowercase();
        let mut result = String::new();
        let mut last_end = 0;

        for (start, _) in lower_line.match_indices(&lower_pattern) {
            result.push_str(&line[last_end..start]);
            result.push_str(&format!(
                "{}",
                line[start..start + pattern.len()].red().bold()
            ));
            last_end = start + pattern.len();
        }
        result.push_str(&line[last_end..]);
        result
    } else {
        // Case-sensitive highlighting
        line.replace(pattern, &format!("{}", pattern.red().bold()))
    }
}

/// Normalize BM25 score to 0-1 range
/// LanceDB/tantivy BM25 returns positive scores where higher = better match
fn normalize_bm25_score(bm25_score: f32) -> f32 {
    // BM25 from LanceDB/tantivy returns positive scores
    // Higher = better match (e.g., 20 is better than 5)
    // Convert to 0-1 scale where higher = better
    //
    // Typical scores range from 0 to around 30
    if bm25_score < 0.001 {
        0.0
    } else {
        // sigmoid-like mapping: higher input → higher output
        // 30 → ~0.86, 10 → ~0.67, 5 → ~0.50, 1 → ~0.17
        1.0 - (1.0 / (1.0 + bm25_score / 5.0))
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
    fn test_extract_fts_terms() {
        assert_eq!(extract_fts_terms("hello"), "hello");
        assert_eq!(extract_fts_terms("hello.*world"), "hello OR world");
        assert_eq!(extract_fts_terms("foo_bar"), "foo_bar");
        // Single character terms get filtered out (less than 2 chars)
        // "fn\\s+\\w+" becomes "fn" after cleaning, which is exactly 2 chars
        assert_eq!(extract_fts_terms("fn\\s+\\w+"), "fn");
    }

    #[test]
    fn test_normalize_bm25_score() {
        // LanceDB/tantivy BM25 scores are positive where higher = better match
        // Our normalization converts to 0-1 where higher = better
        let score1 = normalize_bm25_score(20.0);
        let score2 = normalize_bm25_score(5.0);
        // Both should be in valid range
        assert!(score1 > 0.0 && score1 <= 1.0);
        assert!(score2 > 0.0 && score2 <= 1.0);
        // Higher BM25 = better match = higher normalized score
        assert!(
            score1 > score2,
            "score1={} should be greater than score2={} because 20 > 5",
            score1,
            score2
        );

        // Zero/near-zero score should be 0.0
        assert!(normalize_bm25_score(0.0) < 0.01);
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
    fn test_find_matching_lines_basic() {
        let content = "line 1\nline 2 with pattern\nline 3";
        let result = find_matching_lines(content, "pattern", None, false, 0, 1);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].line_number, 2);
        assert!(result[0].content.contains("pattern"));
    }

    #[test]
    fn test_find_matching_lines_with_context() {
        let content = "line 1\nline 2\nline 3 with pattern\nline 4\nline 5";
        let result = find_matching_lines(content, "pattern", None, false, 1, 1);
        assert_eq!(result.len(), 3);
        assert_eq!(result[0].line_number, 2);
        assert_eq!(result[1].line_number, 3);
        assert_eq!(result[2].line_number, 4);
    }

    #[test]
    fn test_find_matching_lines_case_insensitive() {
        let content = "line 1\nline 2 with PATTERN\nline 3";
        let result = find_matching_lines(content, "pattern", None, true, 0, 1);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].line_number, 2);
    }

    #[test]
    fn test_find_matching_lines_with_regex() {
        let content = "fn foo()\nfn bar()\nlet x = 1";
        let re = Regex::new(r"fn\s+\w+").unwrap();
        let result = find_matching_lines(content, "", Some(&re), false, 0, 1);
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn test_highlight_match_simple() {
        let line = "hello world";
        let result = highlight_match(line, "world", None, false);
        // Should contain the match with ANSI color codes
        assert!(result.contains("world"));
    }

    #[test]
    fn test_highlight_match_case_insensitive() {
        let line = "Hello WORLD";
        let result = highlight_match(line, "world", None, true);
        // The original case should be preserved in the highlight
        assert!(result.contains("WORLD"));
    }
}
