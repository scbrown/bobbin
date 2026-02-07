use anyhow::{Context, Result};
use clap::Args;
use colored::Colorize;
use serde::Serialize;
use std::path::PathBuf;
use std::time::Instant;

use super::OutputConfig;
use crate::config::Config;
use crate::index::embedder;
use crate::index::Embedder;

#[derive(Args)]
pub struct BenchmarkArgs {
    /// Queries to benchmark (can specify multiple)
    #[arg(long, short = 'q', required = true)]
    query: Vec<String>,

    /// Models to compare (default: all built-in models)
    #[arg(long, short = 'm')]
    model: Vec<String>,

    /// Number of iterations per query (default: 5)
    #[arg(long, default_value = "5")]
    iterations: usize,

    /// Batch size for embedding (default: 32)
    #[arg(long, default_value = "32")]
    batch_size: usize,

    /// Directory containing .bobbin/ config (defaults to current directory)
    #[arg(default_value = ".")]
    path: PathBuf,
}

#[derive(Serialize)]
struct BenchmarkOutput {
    models: Vec<ModelBenchmark>,
    queries: Vec<String>,
    iterations: usize,
}

#[derive(Serialize)]
struct ModelBenchmark {
    model: String,
    dimension: usize,
    load_time_ms: f64,
    embed_single: EmbedStats,
    embed_batch: EmbedStats,
}

#[derive(Serialize)]
struct EmbedStats {
    mean_ms: f64,
    min_ms: f64,
    max_ms: f64,
    p50_ms: f64,
    p95_ms: f64,
}

pub async fn run(args: BenchmarkArgs, output: OutputConfig) -> Result<()> {
    let models = if args.model.is_empty() {
        vec![
            "all-MiniLM-L6-v2".to_string(),
            "bge-small-en-v1.5".to_string(),
            "gte-small".to_string(),
        ]
    } else {
        args.model.clone()
    };

    let model_dir = Config::model_cache_dir()?;

    if !output.quiet && !output.json {
        println!(
            "{} Embedding model benchmark",
            "▶".cyan()
        );
        println!(
            "  Models:     {}",
            models.join(", ")
        );
        println!("  Queries:    {}", args.query.len());
        println!("  Iterations: {}", args.iterations);
        println!();
    }

    let mut results: Vec<ModelBenchmark> = Vec::new();

    for model_name in &models {
        if !output.quiet && !output.json {
            print!("  {} {}... ", "⏱".dimmed(), model_name.bold());
        }

        // Ensure model is downloaded
        match embedder::ensure_model(&model_dir, model_name).await {
            Ok(_) => {}
            Err(e) => {
                if !output.quiet && !output.json {
                    println!("{}", format!("skip ({})", e).red());
                }
                continue;
            }
        }

        // Measure load time
        let load_start = Instant::now();
        let mut embed = match Embedder::load(&model_dir, model_name) {
            Ok(e) => e,
            Err(e) => {
                if !output.quiet && !output.json {
                    println!("{}", format!("skip ({})", e).red());
                }
                continue;
            }
        };
        let load_time_ms = load_start.elapsed().as_secs_f64() * 1000.0;

        let dimension = embed.dimension();

        // Benchmark single embeddings
        let mut single_times: Vec<f64> = Vec::new();
        for query in &args.query {
            for _ in 0..args.iterations {
                let start = Instant::now();
                embed
                    .embed_batch(&[query.as_str()])
                    .await
                    .context("Embedding failed")?;
                single_times.push(start.elapsed().as_secs_f64() * 1000.0);
            }
        }

        // Benchmark batch embeddings
        let query_refs: Vec<&str> = args.query.iter().map(|q| q.as_str()).collect();
        let mut batch_times: Vec<f64> = Vec::new();
        for _ in 0..args.iterations {
            let start = Instant::now();
            embed
                .embed_batch(&query_refs)
                .await
                .context("Batch embedding failed")?;
            batch_times.push(start.elapsed().as_secs_f64() * 1000.0);
        }

        let single_stats = compute_stats(&single_times);
        let batch_stats = compute_stats(&batch_times);

        if !output.quiet && !output.json {
            println!(
                "{}",
                format!(
                    "dim={} load={:.0}ms single={:.1}ms batch={:.1}ms",
                    dimension, load_time_ms, single_stats.mean_ms, batch_stats.mean_ms
                )
                .green()
            );
        }

        results.push(ModelBenchmark {
            model: model_name.clone(),
            dimension,
            load_time_ms,
            embed_single: single_stats,
            embed_batch: batch_stats,
        });
    }

    if output.json {
        let bench_output = BenchmarkOutput {
            models: results,
            queries: args.query.clone(),
            iterations: args.iterations,
        };
        println!("{}", serde_json::to_string_pretty(&bench_output)?);
    } else if !output.quiet {
        println!();
        println!("{}", "Results:".bold());
        println!(
            "  {:<25} {:>5} {:>10} {:>12} {:>12}",
            "Model", "Dim", "Load (ms)", "Single (ms)", "Batch (ms)"
        );
        println!("  {}", "-".repeat(70));

        for result in &results {
            println!(
                "  {:<25} {:>5} {:>10.1} {:>12.2} {:>12.2}",
                result.model,
                result.dimension,
                result.load_time_ms,
                result.embed_single.mean_ms,
                result.embed_batch.mean_ms,
            );
        }

        if results.len() > 1 {
            println!();
            let fastest = results
                .iter()
                .min_by(|a, b| {
                    a.embed_single
                        .mean_ms
                        .partial_cmp(&b.embed_single.mean_ms)
                        .unwrap()
                })
                .unwrap();
            println!(
                "  {} Fastest single embed: {} ({:.2}ms)",
                "★".yellow(),
                fastest.model.green(),
                fastest.embed_single.mean_ms,
            );
        }
    }

    Ok(())
}

fn compute_stats(times: &[f64]) -> EmbedStats {
    if times.is_empty() {
        return EmbedStats {
            mean_ms: 0.0,
            min_ms: 0.0,
            max_ms: 0.0,
            p50_ms: 0.0,
            p95_ms: 0.0,
        };
    }

    let mut sorted = times.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());

    let sum: f64 = sorted.iter().sum();
    let mean = sum / sorted.len() as f64;
    let min = sorted[0];
    let max = sorted[sorted.len() - 1];
    let p50 = percentile(&sorted, 50.0);
    let p95 = percentile(&sorted, 95.0);

    EmbedStats {
        mean_ms: mean,
        min_ms: min,
        max_ms: max,
        p50_ms: p50,
        p95_ms: p95,
    }
}

fn percentile(sorted: &[f64], pct: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let idx = (pct / 100.0 * (sorted.len() - 1) as f64).round() as usize;
    sorted[idx.min(sorted.len() - 1)]
}
