use anyhow::{bail, Context, Result};
use clap::Args;
use colored::Colorize;
use serde::Serialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use super::OutputConfig;
use crate::config::Config;
use crate::index::GitAnalyzer;

#[derive(Args)]
pub struct HistoryArgs {
    /// File to show history for
    file: PathBuf,

    /// Maximum number of entries to show
    #[arg(long, short = 'n', default_value = "20")]
    limit: usize,
}

#[derive(Serialize)]
struct HistoryOutput {
    file: String,
    entries: Vec<HistoryEntry>,
    stats: HistoryStats,
}

#[derive(Serialize)]
struct HistoryEntry {
    date: String,
    author: String,
    message: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    issues: Vec<String>,
}

#[derive(Serialize)]
struct HistoryStats {
    total_commits: usize,
    authors: Vec<AuthorStat>,
    churn_rate: f32,
}

#[derive(Serialize)]
struct AuthorStat {
    name: String,
    commits: usize,
}

pub async fn run(args: HistoryArgs, output: OutputConfig) -> Result<()> {
    // 1. Resolve path and repo root
    let file_path = args
        .file
        .canonicalize()
        .with_context(|| format!("File not found: {}", args.file.display()))?;

    let repo_root = find_repo_root(&file_path)?;

    // 2. Get relative path
    let rel_path = file_path
        .strip_prefix(&repo_root)
        .context("File is not inside the repository")?
        .to_string_lossy()
        .to_string();

    // 3. Create git analyzer and get file history
    let analyzer = GitAnalyzer::new(&repo_root)?;
    let history = analyzer.get_file_history(&rel_path, args.limit)?;

    if history.is_empty() {
        if output.json {
            println!(
                "{}",
                serde_json::to_string_pretty(&HistoryOutput {
                    file: rel_path,
                    entries: vec![],
                    stats: HistoryStats {
                        total_commits: 0,
                        authors: vec![],
                        churn_rate: 0.0,
                    },
                })?
            );
        } else {
            bail!("No history found for: {}", rel_path);
        }
        return Ok(());
    }

    // 4. Calculate statistics
    let mut author_counts: HashMap<String, usize> = HashMap::new();
    for entry in &history {
        *author_counts.entry(entry.author.clone()).or_insert(0) += 1;
    }

    let mut authors: Vec<AuthorStat> = author_counts
        .into_iter()
        .map(|(name, commits)| AuthorStat { name, commits })
        .collect();
    authors.sort_by(|a, b| b.commits.cmp(&a.commits));

    // Calculate churn rate (commits per month based on history span)
    let churn_rate = if history.len() >= 2 {
        let first_ts = history.last().map(|e| e.timestamp).unwrap_or(0);
        let last_ts = history.first().map(|e| e.timestamp).unwrap_or(0);
        let days = ((last_ts - first_ts) as f32 / 86400.0).max(1.0);
        (history.len() as f32 / days) * 30.0 // commits per month
    } else {
        0.0
    };

    let stats = HistoryStats {
        total_commits: history.len(),
        authors,
        churn_rate,
    };

    // 5. Convert to output format
    let entries: Vec<HistoryEntry> = history
        .into_iter()
        .map(|h| HistoryEntry {
            date: h.date,
            author: h.author,
            message: h.message,
            issues: h.issues,
        })
        .collect();

    // 6. Output results
    if output.json {
        let json_output = HistoryOutput {
            file: rel_path,
            entries,
            stats,
        };
        println!("{}", serde_json::to_string_pretty(&json_output)?);
    } else {
        println!("History for {}:", rel_path.cyan());
        println!();

        for entry in &entries {
            let issues_str = if entry.issues.is_empty() {
                String::new()
            } else {
                format!(" ({})", entry.issues.join(", ").yellow())
            };

            println!(
                "- {} ({}): {}{}",
                entry.date.dimmed(),
                entry.author.green(),
                entry.message,
                issues_str
            );
        }

        println!();
        println!("{}", "Statistics:".bold());
        println!("  Total commits: {}", stats.total_commits);
        println!("  Churn rate: {:.1} commits/month", stats.churn_rate);
        println!("  Authors:");
        for author in &stats.authors {
            println!("    - {}: {} commits", author.name, author.commits);
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
