use anyhow::{bail, Context, Result};
use chrono::Utc;
use clap::Args;
use colored::Colorize;
use indicatif::{ProgressBar, ProgressStyle};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use super::OutputConfig;
use crate::config::Config;
use crate::index::git::GitAnalyzer;
use crate::index::Embedder;
use crate::search::context::{ContentMode, ContextAssembler, ContextConfig};
use crate::storage::{MetadataStore, VectorStore};

// --- CLI Args ---

#[derive(Args)]
pub struct CalibrateArgs {
    /// Number of commits to sample from git history
    #[arg(long, short = 'n', default_value = "20")]
    samples: usize,

    /// Time range to sample from (git --since format)
    #[arg(long, default_value = "6 months ago")]
    since: String,

    /// Max results per probe (search limit). If omitted, sweeps [10, 20, 30, 40].
    #[arg(long)]
    search_limit: Option<usize>,

    /// Budget lines per probe. If omitted, sweeps [150, 300, 500].
    #[arg(long)]
    budget: Option<usize>,

    /// Apply best config to .bobbin/calibration.json
    #[arg(long)]
    apply: bool,

    /// Show detailed per-commit results
    #[arg(long)]
    verbose: bool,

    /// Extended calibration: sweep recency and coupling parameters
    #[arg(long)]
    full: bool,

    /// Resume an interrupted --full sweep from cache
    #[arg(long)]
    resume: bool,

    /// Directory to calibrate
    #[arg(default_value = ".")]
    path: PathBuf,
}

// --- Calibration Results ---

/// Persisted calibration state
#[derive(Debug, Serialize, Deserialize)]
pub struct CalibrationResult {
    pub calibrated_at: String,
    pub snapshot: ProjectSnapshot,
    pub best_config: CalibratedConfig,
    pub top_results: Vec<GridResult>,
    pub sample_count: usize,
    pub probe_count: usize,
    pub terse_warning: bool,
}

/// Point-in-time snapshot of project characteristics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectSnapshot {
    pub chunk_count: usize,
    pub file_count: usize,
    pub primary_language: String,
    pub language_distribution: Vec<(String, f32)>,
    pub repo_age_days: u32,
    pub recent_commit_rate: f32,
}

/// The calibrated search config values
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CalibratedConfig {
    pub semantic_weight: f32,
    pub doc_demotion: f32,
    pub rrf_k: f32,
    /// Best recency half-life in days (only set by --full)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recency_half_life_days: Option<f32>,
    /// Best recency weight (only set by --full)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recency_weight: Option<f32>,
    /// Best coupling depth (only set by --full)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub coupling_depth: Option<usize>,
    /// Best budget_lines (context line budget)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub budget_lines: Option<usize>,
    /// Best search_limit (initial search results)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub search_limit: Option<usize>,
}

/// Result for a single grid point
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GridResult {
    pub semantic_weight: f32,
    pub doc_demotion: f32,
    pub rrf_k: f32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recency_half_life_days: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recency_weight: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub coupling_depth: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub budget_lines: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub search_limit: Option<usize>,
    pub precision: f32,
    pub recall: f32,
    pub f1: f32,
}

// --- Commit Sampling ---

/// A sampled commit for calibration probing
struct SampledCommit {
    hash: String,
    message: String,
    files: Vec<String>,
}

/// Sample commits from git history suitable for calibration.
///
/// Filters: skip merges, reverts, >30 files, <2 files, noise prefixes.
/// Stratified sampling across the time range.
fn sample_commits(
    git: &GitAnalyzer,
    since: &str,
    target_count: usize,
) -> Result<Vec<SampledCommit>> {
    // Fetch a large pool of commits to filter from
    let scan_depth = target_count * 20; // Oversample heavily
    let all_commits = git.get_commit_log(scan_depth, None)?;

    // Parse the since date for filtering
    let since_ts = parse_since_timestamp(git, since)?;

    let mut candidates: Vec<SampledCommit> = Vec::new();

    for commit in &all_commits {
        // Time filter
        if commit.timestamp < since_ts {
            continue;
        }

        // Skip merges (no files or "Merge" prefix)
        if commit.files.is_empty() {
            continue;
        }
        if commit.message.starts_with("Merge ") {
            continue;
        }

        // Skip reverts
        if commit.message.starts_with("Revert ") {
            continue;
        }

        // Skip noise commits
        if is_noise_commit(&commit.message) {
            continue;
        }

        // File count bounds: 2..=30
        let file_count = commit.files.len();
        if file_count < 2 || file_count > 30 {
            continue;
        }

        candidates.push(SampledCommit {
            hash: commit.hash.clone(),
            message: commit.message.clone(),
            files: commit.files.clone(),
        });
    }

    if candidates.is_empty() {
        bail!(
            "No suitable commits found in the last {}. \
             Need commits with 2-30 files, non-merge, non-noise.",
            since
        );
    }

    // Stratified sampling: take evenly spaced commits across the candidate list
    // (candidates are in reverse chronological order from git log)
    let selected = if candidates.len() <= target_count {
        candidates
    } else {
        let step = candidates.len() as f64 / target_count as f64;
        (0..target_count)
            .map(|i| {
                let idx = (i as f64 * step) as usize;
                // Safety: idx < candidates.len() because step = len/count
                let idx = idx.min(candidates.len() - 1);
                SampledCommit {
                    hash: candidates[idx].hash.clone(),
                    message: candidates[idx].message.clone(),
                    files: candidates[idx].files.clone(),
                }
            })
            .collect()
    };

    Ok(selected)
}

/// Check if a commit message indicates a non-code change
fn is_noise_commit(message: &str) -> bool {
    let lower = message.to_lowercase();
    let noise_prefixes = [
        "chore:", "chore(", "ci:", "ci(", "docs:", "docs(",
        "style:", "style(", "build:", "build(", "release:",
        "bump ", "auto-merge", "update dependency",
    ];
    noise_prefixes.iter().any(|p| lower.starts_with(p))
}

/// Parse a "since" string into a unix timestamp.
///
/// Uses `git log --format="" --since=<since> -1` to leverage git's date
/// parsing, then uses `git log --format=%ct --since=<since> --diff-filter=A -1 --reverse`
/// to find the boundary. We actually just need to compute what timestamp the
/// --since flag resolves to, so we ask git for it via `--date=unix`.
fn parse_since_timestamp(git: &GitAnalyzer, since: &str) -> Result<i64> {
    // Use `git log` with --since to find the effective cutoff date.
    // We ask for the boundary by getting the earliest commit IN the range
    // and subtracting 1 second, so `commit.timestamp < since_ts` correctly
    // excludes commits before the window.
    //
    // Simpler approach: compute relative date ourselves for common patterns.
    let now = chrono::Utc::now().timestamp();

    // Try common patterns first
    let lower = since.to_lowercase();
    if let Some(ts) = parse_relative_date(&lower, now) {
        return Ok(ts);
    }

    // Fallback: ask git for the oldest commit in the range and use its timestamp - 1
    let output = std::process::Command::new("git")
        .args([
            "log",
            "--no-merges",
            "--format=%ct",
            &format!("--since={}", since),
            "--reverse",
        ])
        .current_dir(git.repo_root())
        .output()
        .context("Failed to parse --since timestamp")?;

    let first_line = String::from_utf8_lossy(&output.stdout);
    let ts = first_line
        .lines()
        .next()
        .and_then(|l| l.trim().parse::<i64>().ok())
        .unwrap_or(0);

    // Return ts - 1 so that the boundary commit itself passes the < check
    Ok(if ts > 0 { ts - 1 } else { 0 })
}

/// Parse common relative date strings into timestamps.
fn parse_relative_date(s: &str, now: i64) -> Option<i64> {
    let parts: Vec<&str> = s.split_whitespace().collect();
    if parts.len() < 3 || parts.last() != Some(&"ago") {
        return None;
    }
    let n: i64 = parts[0].parse().ok()?;
    let unit = parts[1].trim_end_matches('s'); // "months" -> "month"
    let seconds_per_unit = match unit {
        "second" => 1,
        "minute" => 60,
        "hour" => 3600,
        "day" => 86400,
        "week" => 7 * 86400,
        "month" => 30 * 86400,
        "year" => 365 * 86400,
        _ => return None,
    };
    Some(now - n * seconds_per_unit)
}

/// Detect terse commit messages in the sample
fn detect_terse_messages(commits: &[SampledCommit]) -> bool {
    if commits.is_empty() {
        return false;
    }
    let terse_count = commits
        .iter()
        .filter(|c| is_terse_message(&c.message))
        .count();
    (terse_count as f32 / commits.len() as f32) > 0.5
}

fn is_terse_message(message: &str) -> bool {
    let msg = message.trim();
    if msg.len() < 20 {
        return true;
    }
    let generic = ["fix", "update", "wip", "temp", "fixup", "squash"];
    let lower = msg.to_lowercase();
    generic.iter().any(|g| lower == *g)
}

// --- Scorer ---

/// Score a single probe: compare context bundle files against ground truth files
fn score_probe(injected_files: &[String], ground_truth_files: &[String]) -> (f32, f32, f32) {
    let injected: HashSet<&str> = injected_files.iter().map(|s| s.as_str()).collect();
    let truth: HashSet<&str> = ground_truth_files.iter().map(|s| s.as_str()).collect();

    let overlap: HashSet<&&str> = injected.intersection(&truth).collect();

    let precision = if injected.is_empty() {
        0.0
    } else {
        overlap.len() as f32 / injected.len() as f32
    };

    let recall = if truth.is_empty() {
        0.0
    } else {
        overlap.len() as f32 / truth.len() as f32
    };

    let f1 = if precision + recall > 0.0 {
        2.0 * precision * recall / (precision + recall)
    } else {
        0.0
    };

    (precision, recall, f1)
}

// --- Grid ---

struct GridPoint {
    semantic_weight: f32,
    doc_demotion: f32,
    rrf_k: f32,
    recency_half_life_days: Option<f32>,
    recency_weight: Option<f32>,
    budget_lines: usize,
    search_limit: usize,
}

/// Build the core parameter grid (sw × dd × k × sl × b points).
fn build_grid(budgets: &[usize], search_limits: &[usize]) -> Vec<GridPoint> {
    build_grid_with_recency(&[], &[], budgets, search_limits)
}

/// Build grid with optional recency parameter sweep.
/// Empty recency slices → use config defaults (None in grid point).
fn build_grid_with_recency(
    half_lives: &[f32],
    recency_weights: &[f32],
    budgets: &[usize],
    search_limits: &[usize],
) -> Vec<GridPoint> {
    let sws = [0.0, 0.3, 0.5, 0.7, 0.9];
    let dds = [0.1, 0.3, 0.5];
    let ks = [60.0]; // Keep k fixed for v1

    // If no recency values specified, use a single None entry
    let hl_iter: Vec<Option<f32>> = if half_lives.is_empty() {
        vec![None]
    } else {
        half_lives.iter().map(|&v| Some(v)).collect()
    };
    let rw_iter: Vec<Option<f32>> = if recency_weights.is_empty() {
        vec![None]
    } else {
        recency_weights.iter().map(|&v| Some(v)).collect()
    };

    let mut grid = Vec::new();
    for &sw in &sws {
        for &dd in &dds {
            for &k in &ks {
                for &hl in &hl_iter {
                    for &rw in &rw_iter {
                        for &b in budgets {
                            for &sl in search_limits {
                                grid.push(GridPoint {
                                    semantic_weight: sw,
                                    doc_demotion: dd,
                                    rrf_k: k,
                                    recency_half_life_days: hl,
                                    recency_weight: rw,
                                    budget_lines: b,
                                    search_limit: sl,
                                });
                            }
                        }
                    }
                }
            }
        }
    }
    grid
}

// --- Project Snapshot ---

async fn capture_snapshot(
    vector_store: &VectorStore,
    git: &GitAnalyzer,
) -> Result<ProjectSnapshot> {
    let chunk_count = vector_store.count().await? as usize;

    // Repo age: time from first commit to now
    let repo_age_days = git_repo_age_days(git).unwrap_or(0);

    // Recent commit rate: commits in last 30 days / 4.3 weeks
    let recent_commits = git
        .get_commit_log(500, None)
        .ok()
        .map(|commits| {
            let thirty_days_ago = Utc::now().timestamp() - (30 * 86400);
            commits
                .iter()
                .filter(|c| c.timestamp > thirty_days_ago)
                .count()
        })
        .unwrap_or(0);
    let recent_commit_rate = recent_commits as f32 / 4.3;

    Ok(ProjectSnapshot {
        chunk_count,
        file_count: 0, // TODO: add count_files to VectorStore
        primary_language: "unknown".to_string(), // TODO: add language stats
        language_distribution: vec![],
        repo_age_days,
        recent_commit_rate,
    })
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

/// Create a lightweight ProjectSnapshot from just a chunk count.
/// Used by index.rs auto-calibrate guard to avoid reopening stores.
pub fn capture_snapshot_from_index(chunk_count: usize) -> ProjectSnapshot {
    ProjectSnapshot {
        chunk_count,
        file_count: 0,
        primary_language: "unknown".to_string(),
        language_distribution: vec![],
        repo_age_days: 0,
        recent_commit_rate: 0.0,
    }
}

// --- Persistence ---

fn calibration_path(repo_root: &std::path::Path) -> PathBuf {
    Config::data_dir(repo_root).join("calibration.json")
}

fn cache_path(repo_root: &std::path::Path) -> PathBuf {
    Config::data_dir(repo_root).join("calibration_cache.json")
}

fn save_calibration(repo_root: &std::path::Path, result: &CalibrationResult) -> Result<()> {
    let path = calibration_path(repo_root);
    let json = serde_json::to_string_pretty(result)?;
    std::fs::write(&path, json).with_context(|| format!("Failed to write {}", path.display()))?;
    Ok(())
}

/// Load calibration results (if they exist)
pub fn load_calibration(repo_root: &std::path::Path) -> Option<CalibrationResult> {
    let path = calibration_path(repo_root);
    let content = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&content).ok()
}

// --- Sweep Cache (for --full resume) ---

/// Cached results from an interrupted --full sweep.
/// Keyed by coupling_depth → list of grid results for that depth.
#[derive(Debug, Serialize, Deserialize)]
struct SweepCache {
    /// Map of coupling_depth → completed grid results
    completed_depths: HashMap<String, Vec<GridResult>>,
    /// Total commits sampled (must match to resume)
    sample_count: usize,
    /// Commit hashes for validation
    sample_hashes: Vec<String>,
}

fn save_cache(repo_root: &std::path::Path, cache: &SweepCache) -> Result<()> {
    let path = cache_path(repo_root);
    let json = serde_json::to_string_pretty(cache)?;
    std::fs::write(&path, json).with_context(|| format!("Failed to write cache {}", path.display()))?;
    Ok(())
}

fn load_cache(repo_root: &std::path::Path) -> Option<SweepCache> {
    let path = cache_path(repo_root);
    let content = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&content).ok()
}

fn clear_cache(repo_root: &std::path::Path) {
    let path = cache_path(repo_root);
    let _ = std::fs::remove_file(path);
}

// --- Probe Runner ---

/// Run probes for a grid against sampled commits. Returns scored grid results.
/// The coupling_depth tag is attached to each result for --full sweeps.
///
/// Opens VectorStore, MetadataStore, and Embedder once and reuses across all
/// probes. ContextAssembler.set_config() swaps search params between grid points
/// without reopening stores.
async fn run_probes(
    grid: &[GridPoint],
    commits: &[SampledCommit],
    lance_path: &std::path::Path,
    db_path: &std::path::Path,
    config: &Config,
    model_dir: &std::path::Path,
    coupling_depth: Option<usize>,
    repo_root: &std::path::Path,
    pb: Option<&ProgressBar>,
) -> Result<Vec<GridResult>> {
    // Open stores and embedder once for all probes
    let vs = VectorStore::open(lance_path).await?;
    let ms = MetadataStore::open(db_path)?;
    let embedder = Embedder::from_config(&config.embedding, model_dir)?;

    // Build initial assembler with a placeholder config (overwritten per grid point)
    let initial_config = ContextConfig {
        budget_lines: 300,
        depth: 1,
        max_coupled: 3,
        coupling_threshold: 0.1,
        semantic_weight: 0.5,
        content_mode: ContentMode::None,
        search_limit: 20,
        doc_demotion: 0.3,
        recency_half_life_days: config.search.recency_half_life_days,
        recency_weight: config.search.recency_weight,
        rrf_k: 60.0,
    };
    let mut assembler = ContextAssembler::new(embedder, vs, ms, initial_config);

    let mut grid_results: Vec<GridResult> = Vec::new();
    let prefix = repo_root.to_string_lossy();

    for point in grid {
        // Swap config for this grid point
        assembler.set_config(ContextConfig {
            budget_lines: point.budget_lines,
            depth: 1,
            max_coupled: 3,
            coupling_threshold: 0.1,
            semantic_weight: point.semantic_weight,
            content_mode: ContentMode::None,
            search_limit: point.search_limit,
            doc_demotion: point.doc_demotion,
            recency_half_life_days: point
                .recency_half_life_days
                .unwrap_or(config.search.recency_half_life_days),
            recency_weight: point
                .recency_weight
                .unwrap_or(config.search.recency_weight),
            rrf_k: point.rrf_k,
        });

        let mut total_precision = 0.0_f32;
        let mut total_recall = 0.0_f32;
        let mut total_f1 = 0.0_f32;
        let mut valid_probes = 0usize;

        for commit in commits {
            let bundle = assembler.assemble(&commit.message, None).await;

            if let Ok(bundle) = bundle {
                let injected: Vec<String> = bundle
                    .files
                    .iter()
                    .map(|f| {
                        f.path
                            .strip_prefix(prefix.as_ref())
                            .unwrap_or(&f.path)
                            .trim_start_matches('/')
                            .to_string()
                    })
                    .collect();
                let (p, r, f1) = score_probe(&injected, &commit.files);
                total_precision += p;
                total_recall += r;
                total_f1 += f1;
                valid_probes += 1;
            }

            if let Some(pb) = pb {
                pb.inc(1);
            }
        }

        let n = valid_probes.max(1) as f32;
        grid_results.push(GridResult {
            semantic_weight: point.semantic_weight,
            doc_demotion: point.doc_demotion,
            rrf_k: point.rrf_k,
            recency_half_life_days: point.recency_half_life_days,
            recency_weight: point.recency_weight,
            coupling_depth,
            budget_lines: Some(point.budget_lines),
            search_limit: Some(point.search_limit),
            precision: total_precision / n,
            recall: total_recall / n,
            f1: total_f1 / n,
        });
    }

    Ok(grid_results)
}

/// Re-index coupling data for a specific depth, replacing existing coupling table.
fn reindex_coupling(
    git: &GitAnalyzer,
    db_path: &std::path::Path,
    depth: usize,
    threshold: u32,
) -> Result<()> {
    let couplings = git.analyze_coupling(depth, threshold)?;
    let ms = MetadataStore::open(db_path)?;
    ms.clear_coupling()?;
    ms.begin_transaction()?;
    for c in &couplings {
        ms.upsert_coupling(c)?;
    }
    ms.commit()?;
    Ok(())
}

/// Format a GridResult line for display, adapting columns to --full mode.
fn format_result(r: &GridResult, full: bool) -> String {
    let mut s = format!(
        "  sw={:.2} dd={:.2} k={:.0}",
        r.semantic_weight, r.doc_demotion, r.rrf_k
    );
    // Show budget/search_limit when they differ from defaults
    if let Some(b) = r.budget_lines {
        if b != 300 {
            s.push_str(&format!(" b={}", b));
        }
    }
    if let Some(sl) = r.search_limit {
        if sl != 20 {
            s.push_str(&format!(" sl={}", sl));
        }
    }
    if full {
        if let Some(hl) = r.recency_half_life_days {
            s.push_str(&format!(" hl={:.0}", hl));
        }
        if let Some(rw) = r.recency_weight {
            s.push_str(&format!(" rw={:.2}", rw));
        }
        if let Some(cd) = r.coupling_depth {
            s.push_str(&format!(" cd={}", cd));
        }
    }
    s.push_str(&format!(
        "  F1={:.3}  P={:.3}  R={:.3}",
        r.f1, r.precision, r.recall
    ));
    s
}

// --- Main Entry Point ---

pub async fn run(args: CalibrateArgs, output: OutputConfig) -> Result<()> {
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
    let lance_path = Config::lance_path(&repo_root);
    let db_path = Config::db_path(&repo_root);
    let model_dir = Config::model_cache_dir()?;

    let vector_store = VectorStore::open(&lance_path)
        .await
        .context("Failed to open vector store")?;

    let count = vector_store.count().await?;
    if count == 0 {
        bail!("Index is empty. Run `bobbin index` first.");
    }

    let git = GitAnalyzer::new(&repo_root)?;

    // Phase 1: Sample commits
    if !output.quiet {
        if args.full {
            eprintln!(
                "{}",
                "Extended calibration (includes recency and coupling parameters)...".bold()
            );
        } else {
            eprintln!(
                "{}",
                "Calibrating search parameters against git history...".bold()
            );
        }
    }

    let commits = sample_commits(&git, &args.since, args.samples)?;
    let is_terse = detect_terse_messages(&commits);

    if is_terse && !output.quiet {
        eprintln!(
            "{}",
            "⚠ Many commit messages are too short for reliable calibration.\n  \
             Calibration accuracy may be reduced."
                .yellow()
        );
    }

    if !output.quiet {
        eprintln!(
            "  Sampled {} commits across last {}",
            commits.len(),
            args.since
        );
    }

    // Validate embedder can be loaded before starting the sweep
    let _embedder_check =
        Embedder::from_config(&config.embedding, &model_dir)
            .context("Failed to load embedding model")?;

    // Phase 2: Build grid and run probes
    let grid_results = if args.full {
        run_full_sweep(
            &args, &output, &config, &commits, &git, &lance_path, &db_path, &model_dir, &repo_root,
        )
        .await?
    } else {
        run_core_sweep(
            &args, &output, &config, &commits, &lance_path, &db_path, &model_dir, &repo_root,
        )
        .await?
    };

    // Phase 3: Sort results and report
    let mut sorted = grid_results;
    sorted.sort_by(|a, b| b.f1.partial_cmp(&a.f1).unwrap());

    let best = sorted
        .first()
        .expect("Grid should have at least one result");

    // Find current config result for comparison
    let current_f1 = sorted
        .iter()
        .find(|r| {
            (r.semantic_weight - config.search.semantic_weight).abs() < 0.01
                && (r.doc_demotion - config.search.doc_demotion).abs() < 0.01
        })
        .map(|r| r.f1)
        .unwrap_or(0.0);

    // Capture snapshot
    let snapshot = capture_snapshot(&vector_store, &git).await?;
    let total_probes = sorted.len() * commits.len();

    let calibration = CalibrationResult {
        calibrated_at: Utc::now().to_rfc3339(),
        snapshot,
        best_config: CalibratedConfig {
            semantic_weight: best.semantic_weight,
            doc_demotion: best.doc_demotion,
            rrf_k: best.rrf_k,
            recency_half_life_days: best.recency_half_life_days,
            recency_weight: best.recency_weight,
            coupling_depth: best.coupling_depth,
            budget_lines: best.budget_lines,
            search_limit: best.search_limit,
        },
        top_results: sorted.iter().take(10).cloned().collect(),
        sample_count: commits.len(),
        probe_count: total_probes,
        terse_warning: is_terse,
    };

    // Output
    if output.json {
        println!("{}", serde_json::to_string_pretty(&calibration)?);
    } else if !output.quiet {
        eprintln!();
        eprintln!("{}", "Calibration results (top 5 by F1):".bold());
        for result in sorted.iter().take(5) {
            eprintln!("{}", format_result(result, args.full));
        }

        eprintln!();
        eprintln!(
            "  Current config F1: {:.3} (sw={:.2})",
            current_f1, config.search.semantic_weight
        );
        eprintln!(
            "  Best config F1:    {:.3} (sw={:.2})  {}",
            best.f1,
            best.semantic_weight,
            if best.f1 > current_f1 && current_f1 > 0.0 {
                let pct = ((best.f1 - current_f1) / current_f1 * 100.0) as i32;
                format!("[+{}% improvement]", pct).green().to_string()
            } else {
                String::new()
            }
        );
    }

    // Apply
    if args.apply {
        save_calibration(&repo_root, &calibration)?;
        if !output.quiet {
            eprintln!(
                "\n  {} Applied best config to .bobbin/calibration.json",
                "✓".green()
            );
        }
    } else if !output.quiet && !output.json {
        eprintln!(
            "\n  Run with {} to apply best config.",
            "--apply".bold()
        );
    }

    // Clean up cache on successful completion of --full
    if args.full {
        clear_cache(&repo_root);
    }

    Ok(())
}

/// Core sweep: semantic_weight × doc_demotion × rrf_k × search_limit × budget.
async fn run_core_sweep(
    args: &CalibrateArgs,
    output: &OutputConfig,
    config: &Config,
    commits: &[SampledCommit],
    lance_path: &std::path::Path,
    db_path: &std::path::Path,
    model_dir: &std::path::Path,
    repo_root: &std::path::Path,
) -> Result<Vec<GridResult>> {
    let budgets = match args.budget {
        Some(b) => vec![b],
        None => vec![150, 300, 500],
    };
    let search_limits = match args.search_limit {
        Some(sl) => vec![sl],
        None => vec![10, 20, 30, 40],
    };
    let grid = build_grid(&budgets, &search_limits);
    let total_probes = grid.len() * commits.len();

    if !output.quiet {
        eprintln!(
            "  Grid: {} configs × {} commits = {} probes",
            grid.len(),
            commits.len(),
            total_probes
        );
    }

    let pb = if !output.quiet {
        let pb = ProgressBar::new(total_probes as u64);
        pb.set_style(
            ProgressStyle::default_bar()
                .template("  Running {pos}/{len} probes {bar:30} {eta}")
                .unwrap()
                .progress_chars("█▓░"),
        );
        Some(pb)
    } else {
        None
    };

    let results = run_probes(
        &grid,
        commits,
        lance_path,
        db_path,
        config,
        model_dir,
        None,
        repo_root,
        pb.as_ref(),
    )
    .await?;

    if let Some(pb) = &pb {
        pb.finish_and_clear();
    }

    Ok(results)
}

/// Full sweep: core grid × recency params, then per coupling_depth.
///
/// Grid: 5 sw × 3 dd × 1 k × 4 hl × 4 rw = 240 configs
/// Per coupling depth: 240 × 4 depths = 960 configs
/// Total probes: 960 × 20 commits = 19,200 (with default samples)
///
/// Coupling depth sweep re-indexes coupling data per depth value,
/// so each depth iteration is a separate probe run.
async fn run_full_sweep(
    args: &CalibrateArgs,
    output: &OutputConfig,
    config: &Config,
    commits: &[SampledCommit],
    git: &GitAnalyzer,
    lance_path: &std::path::Path,
    db_path: &std::path::Path,
    model_dir: &std::path::Path,
    repo_root: &std::path::Path,
) -> Result<Vec<GridResult>> {
    let half_lives: Vec<f32> = vec![7.0, 14.0, 30.0, 90.0];
    let recency_weights: Vec<f32> = vec![0.0, 0.15, 0.30, 0.50];
    let coupling_depths: Vec<usize> = vec![500, 2000, 5000, 20000];
    let budgets: Vec<usize> = match args.budget {
        Some(b) => vec![b],
        None => vec![150, 300, 500],
    };
    let search_limits: Vec<usize> = match args.search_limit {
        Some(sl) => vec![sl],
        None => vec![10, 20, 30, 40],
    };

    let grid = build_grid_with_recency(&half_lives, &recency_weights, &budgets, &search_limits);
    let total_configs = grid.len() * coupling_depths.len();
    let total_probes = total_configs * commits.len();

    if !output.quiet {
        eprintln!(
            "  Grid: {} configs × {} coupling depths = {} total configs",
            grid.len(),
            coupling_depths.len(),
            total_configs,
        );
        eprintln!(
            "  Total: {} configs × {} commits = {} probes",
            total_configs,
            commits.len(),
            total_probes,
        );
    }

    // Load cache for resume
    let mut cache = if args.resume {
        load_cache(repo_root).unwrap_or_else(|| {
            if !output.quiet {
                eprintln!("  No cache found, starting fresh.");
            }
            SweepCache {
                completed_depths: HashMap::new(),
                sample_count: commits.len(),
                sample_hashes: commits.iter().map(|c| c.hash.clone()).collect(),
            }
        })
    } else {
        SweepCache {
            completed_depths: HashMap::new(),
            sample_count: commits.len(),
            sample_hashes: commits.iter().map(|c| c.hash.clone()).collect(),
        }
    };

    // Validate cache matches current sample set
    if args.resume && cache.sample_count != commits.len() {
        if !output.quiet {
            eprintln!(
                "  {} Cache sample count mismatch ({} vs {}), starting fresh.",
                "⚠".yellow(),
                cache.sample_count,
                commits.len()
            );
        }
        cache.completed_depths.clear();
    }

    let mut all_results: Vec<GridResult> = Vec::new();

    // Collect cached results
    for (depth_key, results) in &cache.completed_depths {
        if !output.quiet {
            eprintln!("  Restored {} results from cache (depth={})", results.len(), depth_key);
        }
        all_results.extend(results.iter().cloned());
    }

    // Calculate remaining probes for progress bar
    let remaining_depths: Vec<&usize> = coupling_depths
        .iter()
        .filter(|d| !cache.completed_depths.contains_key(&d.to_string()))
        .collect();
    let remaining_probes = remaining_depths.len() * grid.len() * commits.len();

    let pb = if !output.quiet && remaining_probes > 0 {
        let pb = ProgressBar::new(remaining_probes as u64);
        pb.set_style(
            ProgressStyle::default_bar()
                .template("  [{msg}] {pos}/{len} probes {bar:30} {eta}")
                .unwrap()
                .progress_chars("█▓░"),
        );
        Some(pb)
    } else {
        None
    };

    // Save original coupling data to restore after sweep
    let original_coupling_depth = config.git.coupling_depth;

    for &depth in &coupling_depths {
        let depth_key = depth.to_string();
        if cache.completed_depths.contains_key(&depth_key) {
            continue;
        }

        if let Some(pb) = &pb {
            pb.set_message(format!("cd={}", depth));
        }

        // Re-index coupling data for this depth
        if !output.quiet {
            if let Some(pb) = &pb {
                pb.suspend(|| {
                    eprintln!("  Re-indexing coupling data (depth={})...", depth);
                });
            }
        }
        reindex_coupling(git, db_path, depth, config.git.coupling_threshold)?;

        // Run probes with this coupling depth
        let depth_results = run_probes(
            &grid,
            commits,
            lance_path,
            db_path,
            config,
            model_dir,
            Some(depth),
            repo_root,
            pb.as_ref(),
        )
        .await?;

        all_results.extend(depth_results.iter().cloned());
        cache
            .completed_depths
            .insert(depth_key, depth_results);

        // Save cache after each depth completes
        save_cache(repo_root, &cache)?;
    }

    if let Some(pb) = &pb {
        pb.finish_and_clear();
    }

    // Restore original coupling data
    if !output.quiet {
        eprintln!(
            "  Restoring coupling data (depth={})...",
            original_coupling_depth
        );
    }
    reindex_coupling(git, db_path, original_coupling_depth, config.git.coupling_threshold)?;

    Ok(all_results)
}

// --- CalibrationGuard ---

/// Determines whether a project needs (re)calibration.
pub trait CalibrationGuard {
    fn should_recalibrate(
        &self,
        current: &ProjectSnapshot,
        previous: Option<&CalibrationResult>,
    ) -> bool;
}

/// Default guard: recalibrate on first run, >20% chunk change,
/// primary language change, or >30 days since last calibration.
pub struct DefaultCalibrationGuard;

impl CalibrationGuard for DefaultCalibrationGuard {
    fn should_recalibrate(
        &self,
        current: &ProjectSnapshot,
        previous: Option<&CalibrationResult>,
    ) -> bool {
        let Some(prev) = previous else {
            return true;
        };

        // Chunk count changed >20%
        let prev_chunks = prev.snapshot.chunk_count;
        if prev_chunks > 0 {
            let delta =
                (current.chunk_count as f64 - prev_chunks as f64).abs() / prev_chunks as f64;
            if delta > 0.2 {
                return true;
            }
        }

        // Primary language changed
        if current.primary_language != prev.snapshot.primary_language
            && current.primary_language != "unknown"
            && prev.snapshot.primary_language != "unknown"
        {
            return true;
        }

        // Last calibration >30 days ago
        if let Ok(cal_time) = chrono::DateTime::parse_from_rfc3339(&prev.calibrated_at) {
            let age = Utc::now() - cal_time.with_timezone(&Utc);
            if age.num_days() > 30 {
                return true;
            }
        }

        false
    }
}

impl CalibrateArgs {
    /// Construct args suitable for auto-calibration after indexing.
    pub fn default_for_auto(path: PathBuf) -> Self {
        Self {
            samples: 20,
            since: "6 months ago".to_string(),
            search_limit: Some(20),
            budget: Some(300),
            apply: true,
            verbose: false,
            full: false,
            resume: false,
            path,
        }
    }
}

// --- Tests ---

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_score_probe_perfect() {
        let injected = vec!["a.rs".into(), "b.rs".into()];
        let truth = vec!["a.rs".into(), "b.rs".into()];
        let (p, r, f1) = score_probe(&injected, &truth);
        assert!((p - 1.0).abs() < 0.001);
        assert!((r - 1.0).abs() < 0.001);
        assert!((f1 - 1.0).abs() < 0.001);
    }

    #[test]
    fn test_score_probe_no_overlap() {
        let injected = vec!["a.rs".into(), "b.rs".into()];
        let truth = vec!["c.rs".into(), "d.rs".into()];
        let (p, r, f1) = score_probe(&injected, &truth);
        assert!((p - 0.0).abs() < 0.001);
        assert!((r - 0.0).abs() < 0.001);
        assert!((f1 - 0.0).abs() < 0.001);
    }

    #[test]
    fn test_score_probe_partial() {
        let injected = vec!["a.rs".into(), "b.rs".into(), "c.rs".into()];
        let truth = vec!["a.rs".into(), "d.rs".into()];
        let (p, r, _f1) = score_probe(&injected, &truth);
        // precision: 1/3 = 0.333
        assert!((p - 0.333).abs() < 0.01);
        // recall: 1/2 = 0.5
        assert!((r - 0.5).abs() < 0.001);
    }

    #[test]
    fn test_score_probe_empty_injected() {
        let injected: Vec<String> = vec![];
        let truth = vec!["a.rs".into()];
        let (p, r, f1) = score_probe(&injected, &truth);
        assert!((p - 0.0).abs() < 0.001);
        assert!((r - 0.0).abs() < 0.001);
        assert!((f1 - 0.0).abs() < 0.001);
    }

    #[test]
    fn test_is_noise_commit() {
        assert!(is_noise_commit("chore: update deps"));
        assert!(is_noise_commit("ci: fix pipeline"));
        assert!(is_noise_commit("docs: update readme"));
        assert!(is_noise_commit("Bump version to 1.2.3"));
        assert!(!is_noise_commit("Fix parser to handle nested types"));
        assert!(!is_noise_commit("Add webhook support for real-time indexing"));
    }

    #[test]
    fn test_is_terse_message() {
        assert!(is_terse_message("fix"));
        assert!(is_terse_message("update"));
        assert!(is_terse_message("wip"));
        assert!(is_terse_message("short msg"));
        assert!(!is_terse_message("Fix parser to handle nested generic types correctly"));
    }

    #[test]
    fn test_detect_terse_majority() {
        let commits = vec![
            SampledCommit { hash: "a".into(), message: "fix".into(), files: vec![] },
            SampledCommit { hash: "b".into(), message: "wip".into(), files: vec![] },
            SampledCommit { hash: "c".into(), message: "This is a proper commit message about fixing auth".into(), files: vec![] },
        ];
        // 2/3 terse > 50%
        assert!(detect_terse_messages(&commits));
    }

    #[test]
    fn test_detect_terse_minority() {
        let commits = vec![
            SampledCommit { hash: "a".into(), message: "fix".into(), files: vec![] },
            SampledCommit { hash: "b".into(), message: "Fix parser to handle nested types".into(), files: vec![] },
            SampledCommit { hash: "c".into(), message: "Add webhook support for real-time reindexing".into(), files: vec![] },
        ];
        // 1/3 terse < 50%
        assert!(!detect_terse_messages(&commits));
    }

    #[test]
    fn test_build_grid_size() {
        let grid = build_grid(&[300], &[20]);
        // 5 sw × 3 dd × 1 k × 1 b × 1 sl = 15
        assert_eq!(grid.len(), 15);
        // Core grid points should have no recency overrides
        assert!(grid[0].recency_half_life_days.is_none());
        assert!(grid[0].recency_weight.is_none());
        assert_eq!(grid[0].budget_lines, 300);
        assert_eq!(grid[0].search_limit, 20);
    }

    #[test]
    fn test_build_grid_sweep_budget_search_limit() {
        let grid = build_grid(&[150, 300, 500], &[10, 20, 30, 40]);
        // 5 sw × 3 dd × 1 k × 3 b × 4 sl = 180
        assert_eq!(grid.len(), 180);
    }

    #[test]
    fn test_build_grid_with_recency() {
        let half_lives = [7.0, 14.0, 30.0, 90.0];
        let recency_weights = [0.0, 0.15, 0.30, 0.50];
        let grid = build_grid_with_recency(&half_lives, &recency_weights, &[300], &[20]);
        // 5 sw × 3 dd × 1 k × 4 hl × 4 rw × 1 b × 1 sl = 240
        assert_eq!(grid.len(), 240);
        // All extended grid points should have recency overrides
        assert!(grid[0].recency_half_life_days.is_some());
        assert!(grid[0].recency_weight.is_some());
    }

    #[test]
    fn test_build_grid_recency_empty_fallback() {
        // Empty arrays should fall back to None (same as core grid)
        let grid = build_grid_with_recency(&[], &[], &[300], &[20]);
        assert_eq!(grid.len(), 15);
        assert!(grid[0].recency_half_life_days.is_none());
    }

    #[test]
    fn test_format_result_core() {
        let r = GridResult {
            semantic_weight: 0.3,
            doc_demotion: 0.3,
            rrf_k: 60.0,
            recency_half_life_days: None,
            recency_weight: None,
            coupling_depth: None,
            budget_lines: Some(300),
            search_limit: Some(20),
            precision: 0.4,
            recall: 0.5,
            f1: 0.444,
        };
        let s = format_result(&r, false);
        assert!(s.contains("sw=0.30"));
        assert!(!s.contains("hl="));
        // Default values should not appear in output
        assert!(!s.contains("b="));
        assert!(!s.contains("sl="));
    }

    #[test]
    fn test_format_result_non_default_budget_search_limit() {
        let r = GridResult {
            semantic_weight: 0.3,
            doc_demotion: 0.3,
            rrf_k: 60.0,
            recency_half_life_days: None,
            recency_weight: None,
            coupling_depth: None,
            budget_lines: Some(500),
            search_limit: Some(40),
            precision: 0.4,
            recall: 0.5,
            f1: 0.444,
        };
        let s = format_result(&r, false);
        assert!(s.contains("b=500"));
        assert!(s.contains("sl=40"));
    }

    #[test]
    fn test_format_result_full() {
        let r = GridResult {
            semantic_weight: 0.3,
            doc_demotion: 0.3,
            rrf_k: 60.0,
            recency_half_life_days: Some(14.0),
            recency_weight: Some(0.15),
            coupling_depth: Some(5000),
            budget_lines: Some(300),
            search_limit: Some(20),
            precision: 0.4,
            recall: 0.5,
            f1: 0.444,
        };
        let s = format_result(&r, true);
        assert!(s.contains("hl=14"));
        assert!(s.contains("rw=0.15"));
        assert!(s.contains("cd=5000"));
    }

    #[test]
    fn test_sweep_cache_roundtrip() {
        let mut cache = SweepCache {
            completed_depths: HashMap::new(),
            sample_count: 20,
            sample_hashes: vec!["abc123".into()],
        };
        cache.completed_depths.insert(
            "5000".into(),
            vec![GridResult {
                semantic_weight: 0.3,
                doc_demotion: 0.3,
                rrf_k: 60.0,
                recency_half_life_days: Some(30.0),
                recency_weight: Some(0.3),
                coupling_depth: Some(5000),
                budget_lines: Some(300),
                search_limit: Some(20),
                precision: 0.4,
                recall: 0.5,
                f1: 0.444,
            }],
        );
        let json = serde_json::to_string(&cache).unwrap();
        let loaded: SweepCache = serde_json::from_str(&json).unwrap();
        assert_eq!(loaded.sample_count, 20);
        assert_eq!(loaded.completed_depths.len(), 1);
        assert_eq!(loaded.completed_depths["5000"][0].f1, 0.444);
    }

    // --- CalibrationGuard tests ---

    fn make_snapshot(chunks: usize, lang: &str) -> ProjectSnapshot {
        ProjectSnapshot {
            chunk_count: chunks,
            file_count: 0,
            primary_language: lang.to_string(),
            language_distribution: vec![],
            repo_age_days: 100,
            recent_commit_rate: 5.0,
        }
    }

    fn make_calibration(chunks: usize, lang: &str, days_ago: i64) -> CalibrationResult {
        let cal_time = Utc::now() - chrono::Duration::days(days_ago);
        CalibrationResult {
            calibrated_at: cal_time.to_rfc3339(),
            snapshot: make_snapshot(chunks, lang),
            best_config: CalibratedConfig {
                semantic_weight: 0.7,
                doc_demotion: 0.3,
                rrf_k: 60.0,
                recency_half_life_days: None,
                recency_weight: None,
                coupling_depth: None,
                budget_lines: None,
                search_limit: None,
            },
            top_results: vec![],
            sample_count: 20,
            probe_count: 300,
            terse_warning: false,
        }
    }

    #[test]
    fn test_guard_first_run_always_calibrates() {
        let guard = DefaultCalibrationGuard;
        let current = make_snapshot(1000, "rust");
        assert!(guard.should_recalibrate(&current, None));
    }

    #[test]
    fn test_guard_chunk_delta_over_20_pct() {
        let guard = DefaultCalibrationGuard;
        let current = make_snapshot(1300, "rust"); // 30% increase from 1000
        let prev = make_calibration(1000, "rust", 5);
        assert!(guard.should_recalibrate(&current, Some(&prev)));
    }

    #[test]
    fn test_guard_chunk_delta_under_20_pct() {
        let guard = DefaultCalibrationGuard;
        let current = make_snapshot(1100, "rust"); // 10% increase from 1000
        let prev = make_calibration(1000, "rust", 5);
        assert!(!guard.should_recalibrate(&current, Some(&prev)));
    }

    #[test]
    fn test_guard_language_change() {
        let guard = DefaultCalibrationGuard;
        let current = make_snapshot(1000, "python");
        let prev = make_calibration(1000, "rust", 5);
        assert!(guard.should_recalibrate(&current, Some(&prev)));
    }

    #[test]
    fn test_guard_language_unknown_ignored() {
        let guard = DefaultCalibrationGuard;
        let current = make_snapshot(1000, "unknown");
        let prev = make_calibration(1000, "rust", 5);
        assert!(!guard.should_recalibrate(&current, Some(&prev)));
    }

    #[test]
    fn test_guard_age_over_30_days() {
        let guard = DefaultCalibrationGuard;
        let current = make_snapshot(1000, "rust");
        let prev = make_calibration(1000, "rust", 35);
        assert!(guard.should_recalibrate(&current, Some(&prev)));
    }

    #[test]
    fn test_guard_age_under_30_days() {
        let guard = DefaultCalibrationGuard;
        let current = make_snapshot(1000, "rust");
        let prev = make_calibration(1000, "rust", 10);
        assert!(!guard.should_recalibrate(&current, Some(&prev)));
    }

    #[test]
    fn test_guard_no_change() {
        let guard = DefaultCalibrationGuard;
        let current = make_snapshot(1000, "rust");
        let prev = make_calibration(1000, "rust", 5);
        assert!(!guard.should_recalibrate(&current, Some(&prev)));
    }

    #[test]
    fn test_default_for_auto() {
        let args = CalibrateArgs::default_for_auto(PathBuf::from("/tmp/test"));
        assert!(args.apply);
        assert!(!args.verbose);
        assert_eq!(args.samples, 20);
        assert_eq!(args.search_limit, Some(20));
        assert_eq!(args.budget, Some(300));
        assert_eq!(args.path, PathBuf::from("/tmp/test"));
    }

    #[test]
    fn test_capture_snapshot_from_index() {
        let snap = capture_snapshot_from_index(500);
        assert_eq!(snap.chunk_count, 500);
        assert_eq!(snap.primary_language, "unknown");
    }
}
