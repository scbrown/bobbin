use anyhow::{Context, Result};
use clap::Args;
use colored::Colorize;
use serde::Serialize;
use std::path::PathBuf;

use super::OutputConfig;
use crate::config::Config;
use crate::storage::VectorStore;
use crate::tags::TagsConfig;
use crate::types::IndexStats;

/// Embedded primer documentation
const PRIMER: &str = include_str!("../../docs/primer.md");

/// Known section headings in the primer (lowercase for matching)
const SECTIONS: &[&str] = &[
    "what bobbin does",
    "architecture",
    "supported languages",
    "key commands",
    "mcp tools",
    "quick start",
    "configuration",
];

#[derive(Args)]
pub struct PrimeArgs {
    /// Show brief (compact) overview only
    #[arg(long)]
    brief: bool,

    /// Show a specific section (e.g. "architecture", "commands", "mcp tools")
    #[arg(long, value_name = "NAME")]
    section: Option<String>,

    /// Directory to check (defaults to current directory)
    #[arg(default_value = ".")]
    path: PathBuf,
}

#[derive(Serialize)]
struct PrimeOutput {
    primer: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    section: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    stats: Option<IndexStats>,
    initialized: bool,
}

pub async fn run(args: PrimeArgs, output: OutputConfig) -> Result<()> {
    let repo_root = args
        .path
        .canonicalize()
        .with_context(|| format!("Invalid path: {}", args.path.display()))?;

    let config_path = Config::config_path(&repo_root);
    let initialized = config_path.exists();

    // Gather live stats if initialized
    let lance_path = Config::lance_path(&repo_root);
    let vector_store = if initialized {
        VectorStore::open(&lance_path).await.ok()
    } else {
        None
    };
    let stats = if let Some(ref store) = vector_store {
        store.get_stats(None).await.ok()
    } else {
        None
    };

    // Select primer content
    let primer_text = if let Some(ref section_query) = args.section {
        extract_section(PRIMER, section_query)
    } else if args.brief {
        extract_brief(PRIMER)
    } else {
        PRIMER.to_string()
    };

    if output.json {
        let json_output = PrimeOutput {
            primer: primer_text,
            section: args.section,
            stats,
            initialized,
        };
        println!("{}", serde_json::to_string_pretty(&json_output)?);
        return Ok(());
    }

    if output.quiet {
        print!("{}", primer_text);
        return Ok(());
    }

    // Human-readable output
    println!("{}", primer_text);

    // Append live stats
    println!("\n{}", "## Live Status".bold());
    println!();

    if !initialized {
        println!(
            "  {} Bobbin not initialized in {}",
            "!".yellow(),
            repo_root.display()
        );
        println!("  Run `bobbin init` to get started.");
    } else if let Some(ref stats) = stats {
        println!("  Status:       {}", "Ready".green());
        println!("  Total files:  {}", stats.total_files.to_string().cyan());
        println!("  Total chunks: {}", stats.total_chunks.to_string().cyan());

        if let Some(ts) = stats.last_indexed {
            let dt = chrono::DateTime::from_timestamp(ts, 0)
                .map(|t| t.to_rfc3339())
                .unwrap_or_else(|| "Unknown".to_string());
            println!("  Last indexed: {}", dt);
        }

        if !stats.languages.is_empty() {
            println!("  Languages:    {}", stats.languages.iter()
                .map(|l| format!("{} ({} files)", l.language, l.file_count))
                .collect::<Vec<_>>()
                .join(", "));
        }

        // Show dependency stats
        if let Some(ref vs) = vector_store {
            if let Ok((total_deps, resolved_deps)) = vs.get_dependency_stats().await {
                if total_deps > 0 {
                    println!(
                        "  Dependencies: {} ({} resolved)",
                        total_deps.to_string().cyan(),
                        resolved_deps
                    );
                }
            }
        }
    } else {
        println!(
            "  {} Initialized but unable to read index stats",
            "!".yellow()
        );
    }

    // Show available bundles
    let tags_config = load_tags_with_bundles(&repo_root);
    if !tags_config.bundles.is_empty() {
        println!("\n{}", "## Context Bundles".bold());
        println!();
        for b in &tags_config.bundles {
            let files = b.member_files().len();
            let child_hint = if tags_config
                .bundles
                .iter()
                .any(|c| c.parent_name() == Some(&b.name))
            {
                " (+children)"
            } else {
                ""
            };
            // Skip children in the top-level list
            if b.parent_name().is_some() {
                continue;
            }
            println!(
                "  {} — {} ({} files{})",
                b.name, b.description, files, child_hint
            );
        }
        println!();
        println!(
            "  Use `bobbin bundle list` for tree view, `bobbin bundle show <name>` for details"
        );
    }

    Ok(())
}

/// Load tags config with bundle definitions, checking local .bobbin/ first, then global config.
fn load_tags_with_bundles(repo_root: &std::path::Path) -> TagsConfig {
    let local_path = TagsConfig::tags_path(repo_root);
    let mut config = TagsConfig::load_or_default(&local_path);

    if config.bundles.is_empty() {
        if let Some(global_dir) = Config::global_config_dir() {
            let global_tags_path = global_dir.join("tags.toml");
            if global_tags_path.exists() {
                let global_config = TagsConfig::load_or_default(&global_tags_path);
                if !global_config.bundles.is_empty() {
                    config.bundles = global_config.bundles;
                }
            }
        }
    }

    config
}

/// Extract only the first two sections (title + "What Bobbin Does") for --brief.
fn extract_brief(primer: &str) -> String {
    let mut result = String::new();
    let mut heading_count = 0;

    for line in primer.lines() {
        if line.starts_with("## ") {
            heading_count += 1;
            if heading_count > 1 {
                break;
            }
        }
        result.push_str(line);
        result.push('\n');
    }

    result.trim_end().to_string()
}

/// Extract a named section from the primer. Matches by substring (case-insensitive).
fn extract_section(primer: &str, query: &str) -> String {
    let query_lower = query.to_lowercase();

    // Find the best matching section heading
    let binding = query_lower.as_str();
    let target = SECTIONS
        .iter()
        .find(|s| s.contains(&query_lower) || query_lower.contains(*s))
        .unwrap_or(&binding);

    let mut result = String::new();
    let mut capturing = false;

    for line in primer.lines() {
        if line.starts_with("## ") {
            if capturing {
                break; // End of target section
            }
            let heading = line.trim_start_matches('#').trim().to_lowercase();
            if heading.contains(target) || target.contains(&heading.as_str()) {
                capturing = true;
            }
        }

        if capturing {
            result.push_str(line);
            result.push('\n');
        }
    }

    if result.is_empty() {
        format!("Section '{}' not found. Available sections: {}", query,
            SECTIONS.join(", "))
    } else {
        result.trim_end().to_string()
    }
}
