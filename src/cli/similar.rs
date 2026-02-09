use anyhow::{bail, Context, Result};
use clap::Args;
use colored::Colorize;
use serde::Serialize;
use std::path::PathBuf;

use super::OutputConfig;
use crate::analysis::similar::{DuplicateCluster, SimilarResult, SimilarTarget, SimilarityAnalyzer};
use crate::config::Config;
use crate::index::Embedder;
use crate::storage::VectorStore;

#[derive(Args)]
pub struct SimilarArgs {
    /// Target to find similar code for (file:name chunk ref or free text).
    /// Required unless --scan is used.
    target: Option<String>,

    /// Scan entire codebase for near-duplicate clusters
    #[arg(long)]
    scan: bool,

    /// Minimum cosine similarity threshold
    #[arg(long, short = 't', default_value = "0.85")]
    threshold: f32,

    /// Maximum number of results or clusters
    #[arg(long, short = 'n', default_value = "10")]
    limit: usize,

    /// Filter to a specific repository
    #[arg(long, short = 'r')]
    repo: Option<String>,

    /// In scan mode, compare chunks across different repos
    #[arg(long)]
    cross_repo: bool,

    /// Directory to search in (defaults to current directory)
    #[arg(long, short = 'C', default_value = ".")]
    path: PathBuf,
}

#[derive(Serialize)]
struct SimilarOutput {
    mode: String,
    threshold: f32,
    #[serde(skip_serializing_if = "Option::is_none")]
    target: Option<String>,
    count: usize,
    results: Vec<SimilarResultOutput>,
    clusters: Vec<ClusterOutput>,
}

#[derive(Serialize)]
struct SimilarResultOutput {
    file_path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    chunk_type: String,
    start_line: u32,
    end_line: u32,
    similarity: f32,
    language: String,
    explanation: String,
}

#[derive(Serialize)]
struct ClusterOutput {
    representative: ChunkRef,
    avg_similarity: f32,
    member_count: usize,
    members: Vec<SimilarResultOutput>,
}

#[derive(Serialize)]
struct ChunkRef {
    file_path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    chunk_type: String,
    start_line: u32,
    end_line: u32,
    language: String,
}

pub async fn run(args: SimilarArgs, output: OutputConfig) -> Result<()> {
    if !args.scan && args.target.is_none() {
        bail!("Either provide a target or use --scan. Run `bobbin similar --help` for usage.");
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

    let config = Config::load(&config_path).context("Failed to load configuration")?;

    let lance_path = Config::lance_path(&repo_root);
    let model_dir = Config::model_cache_dir()?;

    let vector_store = VectorStore::open(&lance_path)
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

    let embedder = Embedder::from_config(&config.embedding, &model_dir)
        .context("Failed to load embedding model")?;

    let mut analyzer = SimilarityAnalyzer::new(embedder, vector_store);
    let repo_filter = args.repo.as_deref();

    if args.scan {
        let clusters = analyzer
            .scan_duplicates(args.threshold, args.limit, repo_filter, args.cross_repo)
            .await
            .context("Scan failed")?;

        if output.json {
            print_scan_json(&clusters, args.threshold)?;
        } else if !output.quiet {
            print_scan_human(&clusters, args.threshold, output.verbose);
        }
    } else {
        let target_str = args.target.as_deref().unwrap();
        let target = parse_target(target_str);

        let results = analyzer
            .find_similar(&target, args.threshold, args.limit, repo_filter)
            .await
            .context("Similarity search failed")?;

        if output.json {
            print_similar_json(target_str, &results, args.threshold)?;
        } else if !output.quiet {
            print_similar_human(target_str, &results, args.threshold, output.verbose);
        }
    }

    Ok(())
}

/// Parse a target string into a SimilarTarget.
/// If it contains a colon with a file-like prefix, treat as chunk ref; otherwise text.
fn parse_target(s: &str) -> SimilarTarget {
    // Heuristic: if it looks like "file.ext:name", it's a chunk ref
    if let Some(colon_pos) = s.find(':') {
        let before = &s[..colon_pos];
        if before.contains('.') || before.contains('/') {
            return SimilarTarget::ChunkRef(s.to_string());
        }
    }
    SimilarTarget::Text(s.to_string())
}

fn to_result_output(r: &SimilarResult) -> SimilarResultOutput {
    SimilarResultOutput {
        file_path: r.chunk.file_path.clone(),
        name: r.chunk.name.clone(),
        chunk_type: r.chunk.chunk_type.to_string(),
        start_line: r.chunk.start_line,
        end_line: r.chunk.end_line,
        similarity: r.similarity,
        language: r.chunk.language.clone(),
        explanation: r.explanation.clone(),
    }
}

fn print_similar_json(target: &str, results: &[SimilarResult], threshold: f32) -> Result<()> {
    let output = SimilarOutput {
        mode: "single".to_string(),
        threshold,
        target: Some(target.to_string()),
        count: results.len(),
        results: results.iter().map(to_result_output).collect(),
        clusters: vec![],
    };
    println!("{}", serde_json::to_string_pretty(&output)?);
    Ok(())
}

fn print_similar_human(target: &str, results: &[SimilarResult], threshold: f32, verbose: bool) {
    if results.is_empty() {
        println!(
            "{} No similar chunks found for {} (threshold: {:.2})",
            "!".yellow(),
            target.cyan(),
            threshold
        );
        return;
    }

    println!(
        "Similar to {} (threshold: {:.2}):",
        target.cyan(),
        threshold
    );
    println!();

    for (i, result) in results.iter().enumerate() {
        let name_display = result
            .chunk
            .name
            .as_ref()
            .map(|n| format!(" ({})", n.cyan()))
            .unwrap_or_default();

        println!(
            "  {}. {}:{}-{}{}",
            (i + 1).to_string().bold(),
            result.chunk.file_path.blue(),
            result.chunk.start_line,
            result.chunk.end_line,
            name_display,
        );

        println!(
            "     {} {} [{:.2} similarity]",
            result.chunk.chunk_type.to_string().magenta(),
            result.chunk.language.dimmed(),
            result.similarity,
        );

        if verbose {
            println!("     {}", result.explanation.dimmed());
        }

        println!();
    }
}

fn print_scan_json(clusters: &[DuplicateCluster], threshold: f32) -> Result<()> {
    let output = SimilarOutput {
        mode: "scan".to_string(),
        threshold,
        target: None,
        count: clusters.len(),
        results: vec![],
        clusters: clusters
            .iter()
            .map(|c| ClusterOutput {
                representative: ChunkRef {
                    file_path: c.representative.file_path.clone(),
                    name: c.representative.name.clone(),
                    chunk_type: c.representative.chunk_type.to_string(),
                    start_line: c.representative.start_line,
                    end_line: c.representative.end_line,
                    language: c.representative.language.clone(),
                },
                avg_similarity: c.avg_similarity,
                member_count: c.members.len(),
                members: c.members.iter().map(to_result_output).collect(),
            })
            .collect(),
    };
    println!("{}", serde_json::to_string_pretty(&output)?);
    Ok(())
}

fn print_scan_human(clusters: &[DuplicateCluster], threshold: f32, verbose: bool) {
    if clusters.is_empty() {
        println!(
            "{} No duplicate clusters found (threshold: {:.2})",
            "!".yellow(),
            threshold
        );
        return;
    }

    println!(
        "Duplicate clusters (threshold: {:.2}):",
        threshold
    );
    println!();

    for (i, cluster) in clusters.iter().enumerate() {
        let rep_name = cluster
            .representative
            .name
            .as_deref()
            .unwrap_or("unnamed");

        println!(
            "  Cluster {} ({} chunks, avg similarity: {:.2}):",
            (i + 1).to_string().bold(),
            cluster.members.len() + 1,
            cluster.avg_similarity,
        );

        // Representative
        println!(
            "    {} {} ({}:{}-{})",
            "*".green(),
            rep_name.cyan(),
            cluster.representative.file_path.blue(),
            cluster.representative.start_line,
            cluster.representative.end_line,
        );

        // Members
        for member in &cluster.members {
            let member_name = member.chunk.name.as_deref().unwrap_or("unnamed");
            println!(
                "    {} {} ({}:{}-{})     [{:.2}]",
                "-".dimmed(),
                member_name.cyan(),
                member.chunk.file_path.blue(),
                member.chunk.start_line,
                member.chunk.end_line,
                member.similarity,
            );

            if verbose {
                println!("      {}", member.explanation.dimmed());
            }
        }

        println!();
    }
}
