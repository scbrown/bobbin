use anyhow::{bail, Context, Result};
use clap::Args;
use colored::Colorize;
use serde::Serialize;
use std::path::PathBuf;

use super::OutputConfig;
use crate::analysis::impact::{ImpactAnalyzer, ImpactConfig, ImpactMode, ImpactSignal};
use crate::config::Config;
use crate::index::Embedder;
use crate::storage::{MetadataStore, VectorStore};

#[derive(Args)]
pub struct ImpactArgs {
    /// File path or file:function target to analyze
    target: String,

    /// Directory to analyze (defaults to current directory)
    #[arg(long, default_value = ".")]
    path: PathBuf,

    /// Transitive impact depth (1-3)
    #[arg(long, short = 'd', default_value = "1")]
    depth: u32,

    /// Signal mode: combined, coupling, semantic, deps
    #[arg(long, short = 'm', default_value = "combined")]
    mode: String,

    /// Maximum number of results
    #[arg(long, short = 'n', default_value = "15")]
    limit: usize,

    /// Minimum impact score threshold (0.0-1.0)
    #[arg(long, short = 't', default_value = "0.1")]
    threshold: f32,

    /// Filter to a specific repository
    #[arg(long, short = 'r')]
    repo: Option<String>,
}

#[derive(Serialize)]
struct ImpactOutput {
    target: String,
    mode: String,
    depth: u32,
    count: usize,
    results: Vec<ImpactEntry>,
}

#[derive(Serialize)]
struct ImpactEntry {
    file: String,
    signal: String,
    score: f32,
    reason: String,
}

fn parse_mode(s: &str) -> Result<ImpactMode> {
    match s.to_lowercase().as_str() {
        "combined" => Ok(ImpactMode::Combined),
        "coupling" => Ok(ImpactMode::Coupling),
        "semantic" => Ok(ImpactMode::Semantic),
        "deps" => Ok(ImpactMode::Deps),
        _ => bail!(
            "Unknown mode '{}'. Use: combined, coupling, semantic, deps",
            s
        ),
    }
}

fn signal_name(signal: &ImpactSignal) -> &'static str {
    match signal {
        ImpactSignal::Coupling { .. } => "coupling",
        ImpactSignal::Semantic { .. } => "semantic",
        ImpactSignal::Dependency => "deps",
        ImpactSignal::Combined => "combined",
    }
}

pub async fn run(args: ImpactArgs, output: OutputConfig) -> Result<()> {
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
    let mode = parse_mode(&args.mode)?;

    let impact_config = ImpactConfig {
        mode,
        threshold: args.threshold,
        limit: args.limit,
    };

    let metadata_store = MetadataStore::open(&Config::db_path(&repo_root))
        .context("Failed to open metadata store")?;
    let vector_store = VectorStore::open(&Config::lance_path(&repo_root))
        .await
        .context("Failed to open vector store")?;
    let model_dir = Config::model_cache_dir()?;
    let embedder = Embedder::from_config(&config.embedding, &model_dir)?;

    let mut analyzer = ImpactAnalyzer::new(metadata_store, vector_store, embedder);
    let results = analyzer
        .analyze(&args.target, &impact_config, args.depth, args.repo.as_deref())
        .await?;

    if output.json {
        let entries: Vec<ImpactEntry> = results
            .iter()
            .map(|r| ImpactEntry {
                file: r.path.clone(),
                signal: signal_name(&r.signal).to_string(),
                score: r.score,
                reason: r.reason.clone(),
            })
            .collect();
        let json_output = ImpactOutput {
            target: args.target,
            mode: args.mode,
            depth: args.depth,
            count: entries.len(),
            results: entries,
        };
        println!("{}", serde_json::to_string_pretty(&json_output)?);
    } else if !output.quiet {
        if results.is_empty() {
            println!(
                "{} No impact results for \"{}\" above threshold {:.2}.",
                "!".yellow(),
                args.target,
                args.threshold,
            );
            return Ok(());
        }

        println!(
            "Impact analysis for {}:\n",
            args.target.cyan(),
        );

        // Header
        println!(
            "  {:<4} {:<40} {:<10} {:<6} {}",
            "#".bold(),
            "File".bold(),
            "Signal".bold(),
            "Score".bold(),
            "Reason".bold(),
        );

        for (i, r) in results.iter().enumerate() {
            let sig = signal_name(&r.signal);
            println!(
                "  {:<4} {:<40} {:<10} {:<6.3} {}",
                format!("{}.", i + 1),
                r.path.cyan(),
                sig,
                r.score,
                r.reason.dimmed(),
            );
        }

        if output.verbose {
            println!("\n{}", "Legend:".bold());
            println!("  score = impact likelihood (0.0-1.0)");
            println!("  combined = max(coupling, semantic, deps)");
            println!("  depth {} transitive expansion", args.depth);
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_mode() {
        assert_eq!(parse_mode("combined").unwrap(), ImpactMode::Combined);
        assert_eq!(parse_mode("coupling").unwrap(), ImpactMode::Coupling);
        assert_eq!(parse_mode("semantic").unwrap(), ImpactMode::Semantic);
        assert_eq!(parse_mode("deps").unwrap(), ImpactMode::Deps);
        assert_eq!(parse_mode("COMBINED").unwrap(), ImpactMode::Combined);
        assert!(parse_mode("invalid").is_err());
    }

    #[test]
    fn test_signal_name() {
        assert_eq!(
            signal_name(&ImpactSignal::Coupling { co_changes: 5 }),
            "coupling"
        );
        assert_eq!(
            signal_name(&ImpactSignal::Semantic { similarity: 0.9 }),
            "semantic"
        );
        assert_eq!(signal_name(&ImpactSignal::Dependency), "deps");
        assert_eq!(signal_name(&ImpactSignal::Combined), "combined");
    }

    #[test]
    fn test_impact_output_serialization() {
        let output = ImpactOutput {
            target: "src/auth.rs".to_string(),
            mode: "combined".to_string(),
            depth: 1,
            count: 1,
            results: vec![ImpactEntry {
                file: "src/session.rs".to_string(),
                signal: "coupling".to_string(),
                score: 0.82,
                reason: "Co-changed 47 times".to_string(),
            }],
        };
        let json = serde_json::to_string(&output).unwrap();
        assert!(json.contains("src/auth.rs"));
        assert!(json.contains("src/session.rs"));
        assert!(json.contains("0.82"));
    }
}
