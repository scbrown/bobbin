use anyhow::Context;
use anyhow::Result;
use chrono::Utc;
use clap::Args;
use colored::Colorize;
use serde::Serialize;
use std::path::PathBuf;

use super::OutputConfig;
use super::calibrate::{self, CalibrationResult, DefaultCalibrationGuard, CalibrationGuard, capture_snapshot_from_index};
use crate::config::Config;
use crate::index::git::GitAnalyzer;
use crate::storage::VectorStore;
use crate::types::IndexStats;

#[derive(Args)]
pub struct StatusArgs {
    /// Show detailed statistics
    #[arg(long)]
    detailed: bool,

    /// Show stats for a specific repository only
    #[arg(long, short = 'r')]
    repo: Option<String>,

    /// Directory to check status in (defaults to current directory)
    #[arg(default_value = ".")]
    path: PathBuf,
}

#[derive(Serialize)]
struct StatusOutput {
    status: String,
    path: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    repos: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    stats: Option<IndexStats>,
    #[serde(skip_serializing_if = "Option::is_none")]
    calibration: Option<CalibrationStatus>,
    #[serde(skip_serializing_if = "Option::is_none")]
    git: Option<GitStatus>,
}

#[derive(Serialize)]
struct CalibrationStatus {
    calibrated: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    calibrated_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    config_summary: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    f1: Option<f32>,
    stale: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    chunk_delta_pct: Option<f32>,
}

#[derive(Serialize)]
struct GitStatus {
    repo_age_days: u32,
    recent_commit_rate: f32,
}

pub async fn run(args: StatusArgs, output: OutputConfig) -> Result<()> {
    // Thin-client mode: proxy through remote server
    if let Some(ref server_url) = output.server {
        return run_remote(args, output.clone(), server_url).await;
    }

    let repo_root = args
        .path
        .canonicalize()
        .with_context(|| format!("Invalid path: {}", args.path.display()))?;

    let config_path = Config::config_path(&repo_root);
    let data_dir = Config::data_dir(&repo_root);

    if !config_path.exists() {
        if output.json {
            let json_output = StatusOutput {
                status: "not_initialized".to_string(),
                path: repo_root.display().to_string(),
                repos: vec![],
                stats: None,
                calibration: None,
                git: None,
            };
            println!("{}", serde_json::to_string_pretty(&json_output)?);
        } else if !output.quiet {
            println!(
                "{} Bobbin not initialized in {}",
                "!".yellow(),
                repo_root.display()
            );
            println!("Run `bobbin init` to initialize.");
        }
        return Ok(());
    }

    // Get stats from LanceDB (primary storage)
    let lance_path = Config::lance_path(&repo_root);
    let vector_store = VectorStore::open(&lance_path)
        .await
        .context("Failed to open vector store")?;

    let repos = vector_store.get_all_repos().await?;
    let stats = vector_store.get_stats(args.repo.as_deref()).await?;

    // Calibration state
    let calibration_result = calibrate::load_calibration(&repo_root);
    let cal_status = build_calibration_status(&calibration_result, stats.total_chunks);

    // Git info
    let git_status = GitAnalyzer::new(&repo_root)
        .ok()
        .map(|git| build_git_status(&git));

    if output.json {
        let json_output = StatusOutput {
            status: "ready".to_string(),
            path: data_dir.display().to_string(),
            repos,
            stats: Some(stats),
            calibration: Some(cal_status),
            git: git_status,
        };
        println!("{}", serde_json::to_string_pretty(&json_output)?);
    } else if !output.quiet {
        println!("{} Bobbin status for {}", "✓".green(), repo_root.display());

        // --- Index section ---
        println!("\n{}", "Index".bold());

        if repos.len() > 1 || (repos.len() == 1 && repos[0] != "default") {
            println!("  Repositories: {}", repos.join(", ").cyan());
        }
        if let Some(ref repo) = args.repo {
            println!("  Showing:      {}", repo.cyan());
        }

        println!("  Files:        {}", stats.total_files.to_string().cyan());
        println!("  Chunks:       {}", stats.total_chunks.to_string().cyan());

        // Language distribution
        if !stats.languages.is_empty() {
            let total_files: u64 = stats.languages.iter().map(|l| l.file_count).sum();
            let lang_parts: Vec<String> = stats
                .languages
                .iter()
                .take(5)
                .map(|l| {
                    let pct = if total_files > 0 {
                        (l.file_count as f32 / total_files as f32 * 100.0) as u32
                    } else {
                        0
                    };
                    format!("{} ({}%)", l.language, pct)
                })
                .collect();
            println!("  Languages:    {}", lang_parts.join(", "));
        }

        if let Some(ts) = stats.last_indexed {
            let dt = chrono::DateTime::from_timestamp(ts, 0)
                .map(|t| format_relative_time(t.timestamp()))
                .unwrap_or_else(|| "Unknown".to_string());
            println!("  Last indexed: {}", dt);
        }

        // Show dependency stats
        if let Ok((total_deps, resolved_deps)) = vector_store.get_dependency_stats().await {
            if total_deps > 0 {
                println!(
                    "  Dependencies: {} ({} resolved)",
                    total_deps.to_string().cyan(),
                    resolved_deps
                );
            }
        }

        // --- Calibration section ---
        println!("\n{}", "Calibration".bold());
        print_calibration_status(&calibration_result, stats.total_chunks);

        // --- Git section ---
        if let Some(ref gs) = git_status {
            println!("\n{}", "Git".bold());
            let age_str = format_age_days(gs.repo_age_days);
            println!("  Repo age:     {}", age_str);
            println!(
                "  Commit rate:  {:.1} commits/week",
                gs.recent_commit_rate
            );
        }

        // --- Detailed language breakdown ---
        if args.detailed && !stats.languages.is_empty() {
            println!("\n{}", "Language Details".bold());
            for lang in &stats.languages {
                println!(
                    "    {}: {} files, {} chunks",
                    lang.language.blue(),
                    lang.file_count,
                    lang.chunk_count
                );
            }
        }
    }

    Ok(())
}

// --- Helpers ---

fn build_calibration_status(
    cal: &Option<CalibrationResult>,
    current_chunks: u64,
) -> CalibrationStatus {
    let Some(cal) = cal else {
        return CalibrationStatus {
            calibrated: false,
            calibrated_at: None,
            config_summary: None,
            f1: None,
            stale: false,
            chunk_delta_pct: None,
        };
    };

    let prev_chunks = cal.snapshot.chunk_count;
    let delta_pct = if prev_chunks > 0 {
        ((current_chunks as f64 - prev_chunks as f64) / prev_chunks as f64 * 100.0) as f32
    } else {
        0.0
    };

    // Check staleness using the CalibrationGuard
    let snapshot = capture_snapshot_from_index(current_chunks as usize);
    let guard = DefaultCalibrationGuard;
    let stale = guard.should_recalibrate(&snapshot, Some(cal));

    let bc = &cal.best_config;
    let mut summary = format!("sw={:.2} dd={:.2} k={:.0}", bc.semantic_weight, bc.doc_demotion, bc.rrf_k);
    if let Some(hl) = bc.recency_half_life_days {
        summary.push_str(&format!(" hl={:.0}", hl));
    }
    if let Some(rw) = bc.recency_weight {
        summary.push_str(&format!(" rw={:.2}", rw));
    }
    if let Some(cd) = bc.coupling_depth {
        summary.push_str(&format!(" cd={}", cd));
    }
    if let Some(b) = bc.budget_lines {
        if b != 300 {
            summary.push_str(&format!(" b={}", b));
        }
    }
    if let Some(sl) = bc.search_limit {
        if sl != 20 {
            summary.push_str(&format!(" sl={}", sl));
        }
    }

    let f1 = cal.top_results.first().map(|r| r.f1);

    CalibrationStatus {
        calibrated: true,
        calibrated_at: Some(cal.calibrated_at.clone()),
        config_summary: Some(summary),
        f1,
        stale,
        chunk_delta_pct: Some(delta_pct),
    }
}

fn print_calibration_status(cal: &Option<CalibrationResult>, current_chunks: u64) {
    let Some(cal) = cal else {
        println!(
            "  Status:       {}",
            "not calibrated".yellow()
        );
        println!("  Run `bobbin calibrate` to find optimal search parameters.");
        return;
    };

    // Date formatting
    let cal_date = chrono::DateTime::parse_from_rfc3339(&cal.calibrated_at)
        .map(|t| t.format("%Y-%m-%d").to_string())
        .unwrap_or_else(|_| "unknown".to_string());

    let bc = &cal.best_config;
    let mut config_str = format!("sw={:.2} dd={:.2} k={:.0}", bc.semantic_weight, bc.doc_demotion, bc.rrf_k);
    if let Some(hl) = bc.recency_half_life_days {
        config_str.push_str(&format!(" hl={:.0}", hl));
    }
    if let Some(rw) = bc.recency_weight {
        config_str.push_str(&format!(" rw={:.2}", rw));
    }
    if let Some(cd) = bc.coupling_depth {
        config_str.push_str(&format!(" cd={}", cd));
    }
    if let Some(b) = bc.budget_lines {
        if b != 300 {
            config_str.push_str(&format!(" b={}", b));
        }
    }
    if let Some(sl) = bc.search_limit {
        if sl != 20 {
            config_str.push_str(&format!(" sl={}", sl));
        }
    }

    let f1 = cal.top_results.first().map(|r| r.f1).unwrap_or(0.0);

    println!(
        "  Status:       {} ({})",
        "calibrated".green(),
        cal_date
    );
    println!(
        "  Config:       {} (F1={:.3})",
        config_str.cyan(),
        f1
    );

    // Staleness check
    let prev_chunks = cal.snapshot.chunk_count;
    let delta_pct = if prev_chunks > 0 {
        ((current_chunks as f64 - prev_chunks as f64) / prev_chunks as f64 * 100.0) as f32
    } else {
        0.0
    };

    let snapshot = capture_snapshot_from_index(current_chunks as usize);
    let guard = DefaultCalibrationGuard;
    let stale = guard.should_recalibrate(&snapshot, Some(cal));

    if stale {
        println!(
            "  Stale:        {} (chunk delta: {:+.0}%)",
            "yes — recalibration recommended".yellow(),
            delta_pct
        );
    } else {
        println!(
            "  Stale:        {} (chunk delta: {:+.0}%)",
            "no".green(),
            delta_pct
        );
    }

    if cal.terse_warning {
        println!(
            "  {}",
            "⚠ Terse commit messages may reduce calibration accuracy".yellow()
        );
    }
}

fn build_git_status(git: &GitAnalyzer) -> GitStatus {
    let repo_age_days = git_repo_age_days(git).unwrap_or(0);
    let recent_commit_rate = git
        .get_commit_log(500, None)
        .ok()
        .map(|commits| {
            let thirty_days_ago = Utc::now().timestamp() - (30 * 86400);
            let count = commits.iter().filter(|c| c.timestamp > thirty_days_ago).count();
            count as f32 / 4.3
        })
        .unwrap_or(0.0);

    GitStatus {
        repo_age_days,
        recent_commit_rate,
    }
}

fn git_repo_age_days(git: &GitAnalyzer) -> Result<u32> {
    let output = std::process::Command::new("git")
        .args(["log", "--reverse", "--format=%ct", "-1"])
        .current_dir(git.repo_root())
        .output()
        .context("Failed to get repo age")?;
    let first_ts: i64 = String::from_utf8_lossy(&output.stdout)
        .trim()
        .parse()
        .unwrap_or(0);
    if first_ts == 0 {
        return Ok(0);
    }
    let now = Utc::now().timestamp();
    Ok(((now - first_ts) / 86400) as u32)
}

fn format_relative_time(ts: i64) -> String {
    let now = Utc::now().timestamp();
    let diff = now - ts;
    if diff < 60 {
        "just now".to_string()
    } else if diff < 3600 {
        format!("{} minutes ago", diff / 60)
    } else if diff < 86400 {
        format!("{} hours ago", diff / 3600)
    } else {
        format!("{} days ago", diff / 86400)
    }
}

fn format_age_days(days: u32) -> String {
    if days < 30 {
        format!("{} days", days)
    } else if days < 365 {
        let months = days as f32 / 30.44;
        format!("{:.1} months", months)
    } else {
        let years = days as f32 / 365.25;
        format!("{:.1} years", years)
    }
}

/// Run status via remote HTTP server (thin-client mode).
async fn run_remote(args: StatusArgs, output: OutputConfig, server_url: &str) -> Result<()> {
    use crate::http::client::Client;

    let client = Client::new(server_url);
    let resp = client.status().await?;

    if output.json {
        let json_output = StatusOutput {
            status: resp.status,
            path: server_url.to_string(),
            repos: vec![],
            calibration: None, // Not available via remote
            git: None,         // Not available via remote
            stats: Some(IndexStats {
                total_files: resp.index.total_files,
                total_chunks: resp.index.total_chunks,
                total_embeddings: resp.index.total_embeddings,
                languages: resp
                    .index
                    .languages
                    .iter()
                    .map(|l| crate::types::LanguageStats {
                        language: l.language.clone(),
                        file_count: l.file_count,
                        chunk_count: l.chunk_count,
                    })
                    .collect(),
                last_indexed: resp.index.last_indexed,
                index_size_bytes: resp.index.index_size_bytes,
            }),
        };
        println!("{}", serde_json::to_string_pretty(&json_output)?);
    } else if !output.quiet {
        println!(
            "{} Bobbin status via {}",
            "✓".green(),
            server_url
        );
        println!();
        println!("  Status:       {}", resp.status.green());
        println!(
            "  Total files:  {}",
            resp.index.total_files.to_string().cyan()
        );
        println!(
            "  Total chunks: {}",
            resp.index.total_chunks.to_string().cyan()
        );

        if let Some(ts) = resp.index.last_indexed {
            let dt = chrono::DateTime::from_timestamp(ts, 0)
                .map(|t| t.to_rfc3339())
                .unwrap_or_else(|| "Unknown".to_string());
            println!("  Last indexed: {}", dt);
        }

        if args.detailed {
            println!("\n  Languages:");
            for lang in &resp.index.languages {
                println!(
                    "    {}: {} files, {} chunks",
                    lang.language.blue(),
                    lang.file_count,
                    lang.chunk_count
                );
            }
        }
    }

    Ok(())
}
