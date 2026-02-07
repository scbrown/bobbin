use anyhow::{bail, Context, Result};
use clap::Args;
use colored::Colorize;
use serde::Serialize;
use std::path::PathBuf;

use super::OutputConfig;
use crate::analysis::complexity::ComplexityAnalyzer;
use crate::config::Config;
use crate::index::GitAnalyzer;

#[derive(Args)]
pub struct HotspotsArgs {
    /// Directory to analyze (defaults to current directory)
    #[arg(long, default_value = ".")]
    path: PathBuf,

    /// Time window for churn analysis (e.g. "6 months ago", "1 year ago")
    #[arg(long, default_value = "1 year ago")]
    since: String,

    /// Maximum number of hotspots to show
    #[arg(long, short = 'n', default_value = "20")]
    limit: usize,

    /// Minimum hotspot score threshold (0.0-1.0)
    #[arg(long, default_value = "0.0")]
    threshold: f32,
}

#[derive(Serialize)]
struct HotspotsOutput {
    count: usize,
    since: String,
    hotspots: Vec<HotspotEntry>,
}

#[derive(Serialize)]
struct HotspotEntry {
    file: String,
    score: f32,
    churn: u32,
    complexity: f32,
    language: String,
}

pub async fn run(args: HotspotsArgs, output: OutputConfig) -> Result<()> {
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

    // Get churn data from git
    let git = GitAnalyzer::new(&repo_root)?;
    let churn_map = git.get_file_churn(Some(&args.since))?;

    if churn_map.is_empty() {
        if output.json {
            println!(
                "{}",
                serde_json::to_string_pretty(&HotspotsOutput {
                    count: 0,
                    since: args.since,
                    hotspots: vec![],
                })?
            );
        } else if !output.quiet {
            println!(
                "{} No file changes found since \"{}\".",
                "!".yellow(),
                args.since
            );
        }
        return Ok(());
    }

    // Compute complexity for files that have churn
    let mut analyzer = ComplexityAnalyzer::new()?;
    let mut hotspots: Vec<HotspotEntry> = Vec::new();

    let max_churn = churn_map.values().copied().max().unwrap_or(1) as f32;

    for (file_path, churn) in &churn_map {
        let language = detect_language(file_path);
        if language == "unknown" || language == "markdown" || language == "json"
            || language == "yaml" || language == "toml"
        {
            continue;
        }

        let abs_path = repo_root.join(file_path);
        let content = match std::fs::read_to_string(&abs_path) {
            Ok(c) => c,
            Err(_) => continue, // file may have been deleted
        };

        let complexity = match analyzer.analyze_file(file_path, &content, &language) {
            Ok(fc) => fc.complexity,
            Err(_) => continue, // skip files that fail to parse
        };

        // Combined score: geometric mean of normalized churn and complexity
        let churn_norm = (*churn as f32) / max_churn;
        let score = (churn_norm * complexity).sqrt();

        if score >= args.threshold {
            hotspots.push(HotspotEntry {
                file: file_path.clone(),
                score,
                churn: *churn,
                complexity,
                language,
            });
        }
    }

    // Sort by score descending
    hotspots.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
    hotspots.truncate(args.limit);

    if output.json {
        let json_output = HotspotsOutput {
            count: hotspots.len(),
            since: args.since,
            hotspots,
        };
        println!("{}", serde_json::to_string_pretty(&json_output)?);
    } else if !output.quiet {
        if hotspots.is_empty() {
            println!(
                "{} No hotspots found above threshold {:.2}.",
                "!".yellow(),
                args.threshold
            );
            return Ok(());
        }

        println!(
            "{} {} hotspot{} (since \"{}\"):\n",
            "🔥".red(),
            hotspots.len(),
            if hotspots.len() == 1 { "" } else { "s" },
            args.since,
        );

        for (i, h) in hotspots.iter().enumerate() {
            let bar = render_bar(h.score, 20);
            println!(
                "{:>3}. {} {:.3}  {} (churn: {}, complexity: {:.2})",
                i + 1,
                bar,
                h.score,
                h.file.cyan(),
                h.churn.to_string().yellow(),
                h.complexity,
            );
        }

        if output.verbose {
            println!("\n{}", "Legend:".bold());
            println!("  score = sqrt(churn_norm * complexity)");
            println!("  churn = commits touching file since \"{}\"", args.since);
            println!("  complexity = weighted AST complexity [0, 1]");
        }
    }

    Ok(())
}

fn render_bar(score: f32, width: usize) -> String {
    let filled = (score * width as f32).round() as usize;
    let empty = width.saturating_sub(filled);
    format!(
        "{}{}",
        "█".repeat(filled).red(),
        "░".repeat(empty).dimmed()
    )
}

fn detect_language(file: &str) -> String {
    let ext = file.rsplit('.').next().unwrap_or("");
    match ext {
        "rs" => "rust",
        "ts" | "tsx" => "typescript",
        "js" | "jsx" => "javascript",
        "py" => "python",
        "go" => "go",
        "java" => "java",
        "cpp" | "cc" | "cxx" | "hpp" | "h" => "cpp",
        "c" => "c",
        "md" => "markdown",
        "json" => "json",
        "yaml" | "yml" => "yaml",
        "toml" => "toml",
        _ => "unknown",
    }
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_language() {
        assert_eq!(detect_language("src/main.rs"), "rust");
        assert_eq!(detect_language("app.ts"), "typescript");
        assert_eq!(detect_language("component.tsx"), "typescript");
        assert_eq!(detect_language("index.js"), "javascript");
        assert_eq!(detect_language("script.py"), "python");
        assert_eq!(detect_language("main.go"), "go");
        assert_eq!(detect_language("App.java"), "java");
        assert_eq!(detect_language("lib.cpp"), "cpp");
        assert_eq!(detect_language("README.md"), "markdown");
        assert_eq!(detect_language("config.json"), "json");
        assert_eq!(detect_language("config.yaml"), "yaml");
        assert_eq!(detect_language("Cargo.toml"), "toml");
        assert_eq!(detect_language("Makefile"), "unknown");
    }

    #[test]
    fn test_render_bar() {
        let bar = render_bar(1.0, 10);
        // Should contain filled characters
        assert!(bar.contains('█'));

        let bar = render_bar(0.0, 10);
        // Should contain empty characters
        assert!(bar.contains('░'));

        let bar = render_bar(0.5, 10);
        // Should contain both
        assert!(bar.contains('█'));
        assert!(bar.contains('░'));
    }

    #[test]
    fn test_hotspots_output_serialization() {
        let output = HotspotsOutput {
            count: 1,
            since: "1 year ago".to_string(),
            hotspots: vec![HotspotEntry {
                file: "src/main.rs".to_string(),
                score: 0.75,
                churn: 42,
                complexity: 0.6,
                language: "rust".to_string(),
            }],
        };
        let json = serde_json::to_string(&output).unwrap();
        assert!(json.contains("src/main.rs"));
        assert!(json.contains("0.75"));
        assert!(json.contains("42"));
    }

    #[test]
    fn test_score_calculation() {
        // Score = sqrt(churn_norm * complexity)
        // With max_churn=10, churn=10, complexity=1.0 -> sqrt(1.0 * 1.0) = 1.0
        let churn_norm = 10.0 / 10.0;
        let complexity = 1.0f32;
        let score = (churn_norm * complexity).sqrt();
        assert!((score - 1.0).abs() < 0.001);

        // With max_churn=10, churn=5, complexity=0.5 -> sqrt(0.5 * 0.5) = 0.5
        let churn_norm = 5.0 / 10.0;
        let complexity = 0.5f32;
        let score = (churn_norm * complexity).sqrt();
        assert!((score - 0.5).abs() < 0.001);

        // With churn=0, score should be 0
        let churn_norm = 0.0f32;
        let complexity = 0.8f32;
        let score = (churn_norm * complexity).sqrt();
        assert!((score - 0.0).abs() < 0.001);
    }
}
