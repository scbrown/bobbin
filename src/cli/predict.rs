use anyhow::{anyhow, Context, Result};
use clap::Args;
use colored::Colorize;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet, HashSet};

use super::OutputConfig;
use crate::config::Config;
use crate::storage::sqlite::{BeadLineageRecord, BugCausalityRecord, MetadataStore};
use crate::types::FileCoupling;

/// Upper bound on lineage/causality rows pulled for the prediction. Like the
/// reconcile view, predict targets the tens-to-hundreds-of-rows telemetry
/// regime (GH#9, bo-6i55 / "L2 frequency").
const SCAN_LIMIT: usize = 100_000;

#[derive(Args)]
pub struct PredictArgs {
    /// What to predict from: a bead id (resolved to its touched files via
    /// lineage) or a file path. Use --files to pass an explicit file list.
    target: String,

    /// Treat `target` as a comma-separated list of file paths, not a bead id.
    #[arg(long)]
    files: bool,

    /// Max co-files / predicted bundles to return.
    #[arg(long, short = 'n', default_value = "10")]
    limit: usize,
}

/// A co-changed file, ranked by accumulated coupling score (bo-6i55 L2).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CoFile {
    pub file: String,
    pub score: f32,
    pub co_changes: u32,
}

/// Per-file bug risk: P(bug | file) ≈ distinct changes touching the file that
/// were later blamed for a bug, over all distinct changes touching it.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct FileRisk {
    pub file: String,
    pub changes: usize,
    pub buggy: usize,
    pub risk: f64,
}

/// A predicted bundle the work is likely to belong to (most common bundle_slug
/// among changes that touched the input files).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct BundlePrediction {
    pub slug: String,
    pub count: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Prediction {
    pub target: String,
    pub input_files: Vec<String>,
    pub co_files: Vec<CoFile>,
    pub file_risk: Vec<FileRisk>,
    /// P(bug) over the union of changes touching any input file.
    pub overall_risk: f64,
    pub predicted_bundles: Vec<BundlePrediction>,
}

pub async fn run(args: PredictArgs, output: OutputConfig) -> Result<()> {
    let repo_root = super::find_bobbin_root()
        .ok_or_else(|| anyhow!("Not inside a bobbin repository (run `bobbin init` first)"))?;
    let store = MetadataStore::open(&Config::db_path(&repo_root))
        .context("Failed to open metadata store")?;

    let lineage = store.list_bead_lineage(None, None, SCAN_LIMIT)?;
    let causality = store.list_bug_causality(None, SCAN_LIMIT)?;

    // Resolve the input file set.
    let input_files = resolve_input_files(&args, &store)?;
    if input_files.is_empty() {
        return Err(anyhow!(
            "No input files for '{}': a bead with no recorded lineage, or an empty --files list",
            args.target
        ));
    }

    // Gather coupling neighbours for each input file.
    let mut couplings: Vec<FileCoupling> = Vec::new();
    for f in &input_files {
        couplings.extend(store.get_coupling(f, args.limit.saturating_mul(5).max(20))?);
    }

    let prediction = build_prediction(
        &args.target,
        &input_files,
        &couplings,
        &lineage,
        &causality,
        args.limit,
    );

    if output.json {
        println!("{}", serde_json::to_string_pretty(&prediction)?);
    } else if !output.quiet {
        print_report(&prediction);
    }

    Ok(())
}

/// Resolve the target into a concrete file set. Explicit `--files` wins; else a
/// path-shaped target (contains `/` or `.`) is taken literally; else the target
/// is treated as a bead id and its touched files are read from lineage.
fn resolve_input_files(args: &PredictArgs, store: &MetadataStore) -> Result<Vec<String>> {
    if args.files {
        return Ok(split_files(&args.target));
    }
    if looks_like_path(&args.target) {
        return Ok(vec![args.target.clone()]);
    }
    // Bead id → union of files across its lineage rows.
    let rows = store.list_bead_lineage(Some(&args.target), None, SCAN_LIMIT)?;
    let mut files: Vec<String> = Vec::new();
    for r in &rows {
        for f in &r.touched_files {
            if !files.contains(f) {
                files.push(f.clone());
            }
        }
    }
    Ok(files)
}

fn split_files(s: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for f in s.split(',').map(str::trim).filter(|s| !s.is_empty()) {
        if !out.iter().any(|x| x == f) {
            out.push(f.to_string());
        }
    }
    out
}

/// Heuristic: a token with a path separator or an extension dot is a file, not a
/// bead id (bead ids look like `bo-6i55`).
fn looks_like_path(s: &str) -> bool {
    s.contains('/') || s.contains('.')
}

/// Build the full prediction. Pure over its inputs so it is unit-testable
/// without a DB.
pub fn build_prediction(
    target: &str,
    input_files: &[String],
    couplings: &[FileCoupling],
    lineage: &[BeadLineageRecord],
    causality: &[BugCausalityRecord],
    limit: usize,
) -> Prediction {
    let input_set: HashSet<&str> = input_files.iter().map(|s| s.as_str()).collect();

    let co_files = rank_cofiles(&input_set, couplings, limit);

    // Beads (changes) blamed for a bug, by id.
    let culprits: HashSet<&str> = causality
        .iter()
        .filter_map(|c| c.culprit_bead_id.as_deref())
        .filter(|s| !s.is_empty())
        .collect();

    // Per-file risk and the union of changes touching any input file.
    let mut file_risk: Vec<FileRisk> = Vec::new();
    let mut union_changes: BTreeSet<&str> = BTreeSet::new();
    for f in input_files {
        let beads = beads_touching(f, lineage);
        let buggy = beads.iter().filter(|b| culprits.contains(*b)).count();
        let changes = beads.len();
        file_risk.push(FileRisk {
            file: f.clone(),
            changes,
            buggy,
            risk: if changes == 0 {
                0.0
            } else {
                buggy as f64 / changes as f64
            },
        });
        union_changes.extend(beads);
    }

    let overall_buggy = union_changes.iter().filter(|b| culprits.contains(*b)).count();
    let overall_risk = if union_changes.is_empty() {
        0.0
    } else {
        overall_buggy as f64 / union_changes.len() as f64
    };

    let predicted_bundles = rank_bundles(&union_changes, lineage, limit);

    Prediction {
        target: target.to_string(),
        input_files: input_files.to_vec(),
        co_files,
        file_risk,
        overall_risk,
        predicted_bundles,
    }
}

/// Distinct change beads whose lineage touched `file`.
fn beads_touching<'a>(file: &str, lineage: &'a [BeadLineageRecord]) -> BTreeSet<&'a str> {
    let mut set = BTreeSet::new();
    for r in lineage {
        if r.touched_files.iter().any(|f| f == file) {
            set.insert(r.bead_id.as_str());
        }
    }
    set
}

/// Accumulate coupling neighbours of the input files (excluding the inputs
/// themselves), summing score + co_changes, ranked score-desc then file-asc.
fn rank_cofiles(input: &HashSet<&str>, couplings: &[FileCoupling], limit: usize) -> Vec<CoFile> {
    let mut agg: BTreeMap<String, (f32, u32)> = BTreeMap::new();
    for c in couplings {
        // The neighbour is whichever endpoint is not an input file.
        let neighbour = match (input.contains(c.file_a.as_str()), input.contains(c.file_b.as_str()))
        {
            (true, false) => &c.file_b,
            (false, true) => &c.file_a,
            // Both-in (internal edge) or neither-in (spurious) → not a neighbour.
            _ => continue,
        };
        let e = agg.entry(neighbour.clone()).or_insert((0.0, 0));
        e.0 += c.score;
        e.1 += c.co_changes;
    }
    let mut out: Vec<CoFile> = agg
        .into_iter()
        .map(|(file, (score, co_changes))| CoFile {
            file,
            score,
            co_changes,
        })
        .collect();
    out.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.file.cmp(&b.file))
    });
    out.truncate(limit);
    out
}

/// Tally bundle slugs across the given change beads, ranked count-desc then
/// slug-asc. A bead contributes each of its distinct slugs once.
fn rank_bundles(
    changes: &BTreeSet<&str>,
    lineage: &[BeadLineageRecord],
    limit: usize,
) -> Vec<BundlePrediction> {
    // bead → its distinct bundle slugs (dedup within a bead across rows).
    let mut by_bead: BTreeMap<&str, BTreeSet<String>> = BTreeMap::new();
    for r in lineage {
        if !changes.contains(r.bead_id.as_str()) {
            continue;
        }
        let entry = by_bead.entry(r.bead_id.as_str()).or_default();
        for slug in r
            .bundle_slugs
            .as_deref()
            .unwrap_or("")
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            entry.insert(slug.to_string());
        }
    }
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for slugs in by_bead.values() {
        for s in slugs {
            *counts.entry(s.clone()).or_insert(0) += 1;
        }
    }
    let mut out: Vec<BundlePrediction> = counts
        .into_iter()
        .map(|(slug, count)| BundlePrediction { slug, count })
        .collect();
    out.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.slug.cmp(&b.slug)));
    out.truncate(limit);
    out
}

fn print_report(p: &Prediction) {
    println!(
        "{} {}  ({} input file{})",
        "predict".bold(),
        p.target.cyan(),
        p.input_files.len(),
        if p.input_files.len() == 1 { "" } else { "s" },
    );

    println!("\n{}", "Co-changed files:".bold());
    if p.co_files.is_empty() {
        println!("  {}", "(none — coupling not calibrated?)".dimmed());
    } else {
        for cf in &p.co_files {
            println!(
                "  {:<40} {} {}",
                cf.file,
                format!("score={:.3}", cf.score).yellow(),
                format!("co={}", cf.co_changes).dimmed(),
            );
        }
    }

    println!(
        "\n{} overall P(bug)={}",
        "Bug risk:".bold(),
        format!("{:.0}%", p.overall_risk * 100.0).red(),
    );
    for fr in &p.file_risk {
        println!(
            "  {:<40} {} {}",
            fr.file,
            format!("{:.0}%", fr.risk * 100.0).red(),
            format!("({}/{} changes buggy)", fr.buggy, fr.changes).dimmed(),
        );
    }

    println!("\n{}", "Predicted bundle(s):".bold());
    if p.predicted_bundles.is_empty() {
        println!("  {}", "(none)".dimmed());
    } else {
        for b in &p.predicted_bundles {
            println!("  {:<30} {}", b.slug.magenta(), format!("×{}", b.count).dimmed());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::sqlite::TouchedSymbol;

    fn coupling(a: &str, b: &str, score: f32, co: u32) -> FileCoupling {
        FileCoupling {
            file_a: a.to_string(),
            file_b: b.to_string(),
            score,
            co_changes: co,
            last_co_change: 0,
        }
    }

    fn lineage(bead: &str, files: &[&str], bundles: Option<&str>) -> BeadLineageRecord {
        BeadLineageRecord {
            id: 0,
            created_at: "2026-06-10T00:00:00Z".to_string(),
            bead_id: bead.to_string(),
            bead_type: None,
            commit_sha: Some(format!("sha-{bead}")),
            bundle_slugs: bundles.map(|s| s.to_string()),
            touched_files: files.iter().map(|s| s.to_string()).collect(),
            action_type: Some("commit".to_string()),
            feature_id: None,
            lines_added: Some(1),
            lines_deleted: Some(0),
            touched_symbols: Vec::<TouchedSymbol>::new(),
        }
    }

    fn causality(bug: &str, culprit_bead: &str) -> BugCausalityRecord {
        BugCausalityRecord {
            id: 0,
            created_at: "2026-06-20T00:00:00Z".to_string(),
            bug_id: bug.to_string(),
            culprit_sha: Some("sha".to_string()),
            culprit_bead_id: Some(culprit_bead.to_string()),
            file: Some("src/x.rs".to_string()),
            confidence: Some(0.9),
        }
    }

    #[test]
    fn test_rank_cofiles_picks_neighbour_and_sums() {
        let input: HashSet<&str> = ["src/a.rs"].into_iter().collect();
        let couplings = vec![
            coupling("src/a.rs", "src/b.rs", 0.8, 5),
            coupling("src/c.rs", "src/a.rs", 0.9, 3), // a is file_b → neighbour is c
            coupling("src/a.rs", "src/b.rs", 0.1, 1), // accumulates onto b
        ];
        let got = rank_cofiles(&input, &couplings, 10);
        // c (0.9) ranks above b (0.8+0.1=0.9 tie → file asc puts b first? scores equal)
        assert_eq!(got.len(), 2);
        // b total score 0.9, c 0.9 → tie broken by file asc: b before c.
        assert_eq!(got[0].file, "src/b.rs");
        assert!((got[0].score - 0.9).abs() < 1e-6);
        assert_eq!(got[0].co_changes, 6);
        assert_eq!(got[1].file, "src/c.rs");
    }

    #[test]
    fn test_rank_cofiles_excludes_internal_edges() {
        // Edge between two input files is not a neighbour of the set.
        let input: HashSet<&str> = ["src/a.rs", "src/b.rs"].into_iter().collect();
        let couplings = vec![coupling("src/a.rs", "src/b.rs", 0.9, 4)];
        assert!(rank_cofiles(&input, &couplings, 10).is_empty());
    }

    #[test]
    fn test_bug_risk_fraction() {
        // src/x.rs touched by 3 changes; bo-c1 was blamed for a bug → 1/3.
        let lineage = vec![
            lineage("bo-c1", &["src/x.rs"], None),
            lineage("bo-c2", &["src/x.rs"], None),
            lineage("bo-c3", &["src/x.rs", "src/y.rs"], None),
        ];
        let cz = vec![causality("bo-bug", "bo-c1")];
        let p = build_prediction(
            "src/x.rs",
            &["src/x.rs".to_string()],
            &[],
            &lineage,
            &cz,
            10,
        );
        assert_eq!(p.file_risk.len(), 1);
        assert_eq!(p.file_risk[0].changes, 3);
        assert_eq!(p.file_risk[0].buggy, 1);
        assert!((p.file_risk[0].risk - 1.0 / 3.0).abs() < 1e-9);
        assert!((p.overall_risk - 1.0 / 3.0).abs() < 1e-9);
    }

    #[test]
    fn test_predicted_bundles_ranked() {
        // Two input files; bundles tallied over the union of touching changes.
        let lineage = vec![
            lineage("bo-c1", &["src/x.rs"], Some("search,rag")),
            lineage("bo-c2", &["src/x.rs"], Some("search")),
            lineage("bo-c3", &["src/y.rs"], Some("rag")),
        ];
        let p = build_prediction(
            "feat",
            &["src/x.rs".to_string(), "src/y.rs".to_string()],
            &[],
            &lineage,
            &[],
            10,
        );
        // search: bo-c1 + bo-c2 = 2; rag: bo-c1 + bo-c3 = 2 → tie, slug asc.
        assert_eq!(p.predicted_bundles[0].slug, "rag");
        assert_eq!(p.predicted_bundles[0].count, 2);
        assert_eq!(p.predicted_bundles[1].slug, "search");
        assert_eq!(p.predicted_bundles[1].count, 2);
    }

    #[test]
    fn test_looks_like_path() {
        assert!(looks_like_path("src/a.rs"));
        assert!(looks_like_path("README.md"));
        assert!(!looks_like_path("bo-6i55"));
    }

    #[test]
    fn test_split_files_dedups() {
        assert_eq!(
            split_files("a.rs, b.rs , a.rs"),
            vec!["a.rs".to_string(), "b.rs".to_string()]
        );
    }
}
