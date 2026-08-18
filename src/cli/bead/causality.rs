//! `bobbin bead reconstruct-causality` — attribute a bug bead to the commits
//! that plausibly introduced it, via prior touches and blame.
//!
//! Split out of the former `src/cli/bead.rs` (bobbin-aoz).

use anyhow::{anyhow, Context, Result};

use super::helpers::*;
use crate::cli::OutputConfig;
use colored::Colorize;
use serde::Serialize;
use std::path::Path;
use std::process::Command;

use crate::storage::sqlite::{
    MetadataStore, NewBugCausality, PriorTouch,
};

#[derive(Debug, Clone, PartialEq)]
pub(super) struct CausalityCandidate {
    pub(super) file: String,
    pub(super) culprit_sha: String,
    pub(super) culprit_bead_id: String,
    pub(super) confidence: f64,
}

#[derive(Serialize)]
struct CausalityOutput {
    bug_id: String,
    rows: usize,
}

/// Reconstruct bug causality and populate `bug_causality` (bo-s1kb, bo-8nmm).
///
/// For each bug bead: gather the files its fix touched, find the most-recent
/// prior commit touching each such file (the candidate culprit), score
/// confidence by how much of the fix's changeset that commit overlaps, then
/// *sharpen* each file with git blame — blaming the exact lines the fix removed
/// against the fix's parent yields the precise introducing commit at higher
/// confidence. Upserts one row per (bug, culprit, file); idempotent via the
/// table's UNIQUE constraint, so periodic re-runs refresh rather than duplicate.
///
/// Layering: blame (bo-8nmm) is the sharp signal and replaces the recency+overlap
/// heuristic (bo-s1kb) per file where available, falling back to it otherwise
/// (pure additions, new files, root commits, non-git trees). `change_events`
/// outcome labeling remains deferred (see bo-s1kb design notes).
pub(super) fn run_reconstruct_causality(
    repo_root: &Path,
    store: &MetadataStore,
    bug: Option<&str>,
    limit: usize,
    output: &OutputConfig,
) -> Result<()> {
    // Git-blame sharpening (bo-8nmm) needs a repo handle; absence (e.g. running
    // against a non-git tree) degrades gracefully to the recency+overlap path.
    let git = crate::index::GitAnalyzer::new(repo_root).ok();
    // Resolve the set of bug beads to process.
    let bug_ids: Vec<String> = match bug {
        Some(b) => vec![b.to_string()],
        None => store
            .distinct_lineage_bead_ids()?
            .into_iter()
            .filter(|(id, ty)| is_bug_bead(id, ty.as_deref()))
            .map(|(id, _)| id)
            .take(limit)
            .collect(),
    };

    let mut results: Vec<CausalityOutput> = Vec::new();
    for bug_id in &bug_ids {
        // The bug's own fix changeset: all files it touched, plus the earliest
        // timestamp (the boundary before which a culprit must have landed).
        let fix_rows = store.list_bead_lineage(Some(bug_id), None, 1000)?;
        if fix_rows.is_empty() {
            continue;
        }
        let mut fix_files: Vec<String> = Vec::new();
        for r in &fix_rows {
            for f in &r.touched_files {
                if !fix_files.contains(f) {
                    fix_files.push(f.clone());
                }
            }
        }
        let before = fix_rows
            .iter()
            .map(|r| r.created_at.as_str())
            .min()
            .unwrap_or("")
            .to_string();
        if fix_files.is_empty() || before.is_empty() {
            continue;
        }

        // Candidate culprits: prior commits touching the same files, excluding
        // the bug's own lineage rows.
        let prior: Vec<PriorTouch> = store
            .prior_lineage_touching_files(&fix_files, &before)?
            .into_iter()
            .filter(|t| &t.bead_id != bug_id)
            .collect();

        let fallback = reconstruct_culprits(&fix_files, &prior);

        // Sharpen with git blame (bo-8nmm): for each fix commit, blame the lines
        // it removed against the parent to find the exact introducing commit.
        // Per-file culprit shas accumulate across all of the bug's fix commits.
        let mut blame_map: std::collections::HashMap<String, Vec<String>> =
            std::collections::HashMap::new();
        if let Some(git) = git.as_ref() {
            for row in &fix_rows {
                let Some(sha) = row.commit_sha.as_deref().filter(|s| !s.is_empty()) else {
                    continue;
                };
                for file in &row.touched_files {
                    if let Ok(entries) = git.blame_fix_culprits(sha, file) {
                        if entries.is_empty() {
                            continue;
                        }
                        let shas = blame_map.entry(file.clone()).or_default();
                        for e in entries {
                            shas.push(e.commit_hash);
                        }
                    }
                }
            }
        }

        let candidates = merge_causality(fallback, &blame_map);
        for c in &candidates {
            store.record_bug_causality(&NewBugCausality {
                bug_id: bug_id.clone(),
                culprit_sha: Some(c.culprit_sha.clone()),
                // Blame-derived culprits know only the sha, not a bead.
                culprit_bead_id: Some(c.culprit_bead_id.clone())
                    .filter(|b| !b.is_empty()),
                file: Some(c.file.clone()),
                confidence: Some(c.confidence),
            })?;
        }
        if !candidates.is_empty() {
            results.push(CausalityOutput {
                bug_id: bug_id.clone(),
                rows: candidates.len(),
            });
        }
    }

    if output.json {
        println!("{}", serde_json::to_string_pretty(&results)?);
    } else if !output.quiet {
        let total: usize = results.iter().map(|r| r.rows).sum();
        if results.is_empty() {
            println!("{}", "No bug causality reconstructed.".dimmed());
        } else {
            for r in &results {
                println!(
                    "{} {} {} culprit row{}",
                    "✓".green(),
                    r.bug_id.cyan(),
                    r.rows,
                    if r.rows == 1 { "" } else { "s" },
                );
            }
            println!(
                "{} {} bug(s), {} causality row(s) recorded",
                "•".dimmed(),
                results.len(),
                total
            );
        }
    }

    Ok(())
}

/// Is `bead_id` a bug? Trusts the lineage `bead_type` column when present,
/// else falls back to `bd show` (best-effort; unknown → not a bug).
fn is_bug_bead(bead_id: &str, lineage_type: Option<&str>) -> bool {
    if let Some(t) = lineage_type {
        if !t.is_empty() {
            return t.eq_ignore_ascii_case("bug");
        }
    }
    bead_json(bead_id)
        .and_then(|v| {
            v.get("issue_type")
                .and_then(|t| t.as_str())
                .map(|t| t.eq_ignore_ascii_case("bug"))
        })
        .unwrap_or(false)
}

/// Pure causality heuristic (bo-s1kb): given the files a bug's fix touched and
/// the prior commits that touched those files (most-recent first), pick the
/// most-recent prior commit per file as that file's culprit and score
/// confidence by the fraction of the fix's files that commit also touched
/// (concentrated blame ⇒ higher confidence). Deterministic ordering: confidence
/// desc, then file asc.
pub(super) fn reconstruct_culprits(fix_files: &[String], prior: &[PriorTouch]) -> Vec<CausalityCandidate> {
    use std::collections::{HashMap, HashSet};
    let fix_set: HashSet<&str> = fix_files.iter().map(|s| s.as_str()).collect();

    // commit_sha → set of fix-files it touched (overlap breadth).
    let mut overlap: HashMap<&str, HashSet<&str>> = HashMap::new();
    // file → most-recent prior touch (prior is already DESC by time).
    let mut chosen: HashMap<&str, &PriorTouch> = HashMap::new();
    for t in prior {
        let sha = match t.commit_sha.as_deref() {
            Some(s) if !s.is_empty() => s,
            _ => continue,
        };
        if !fix_set.contains(t.file.as_str()) {
            continue;
        }
        overlap.entry(sha).or_default().insert(t.file.as_str());
        chosen.entry(t.file.as_str()).or_insert(t);
    }

    let n = fix_files.len().max(1) as f64;
    let mut out: Vec<CausalityCandidate> = chosen
        .into_iter()
        .map(|(file, t)| {
            let sha = t.commit_sha.clone().unwrap_or_default();
            let breadth = overlap.get(sha.as_str()).map(|s| s.len()).unwrap_or(1) as f64;
            CausalityCandidate {
                file: file.to_string(),
                culprit_sha: sha,
                culprit_bead_id: t.bead_id.clone(),
                confidence: (breadth / n).clamp(0.1, 0.95),
            }
        })
        .collect();
    out.sort_by(|a, b| {
        b.confidence
            .partial_cmp(&a.confidence)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.file.cmp(&b.file))
    });
    out
}

/// Pick the dominant culprit from a per-line list of blame commit shas: the sha
/// that blames the most lines. Returns `(sha, attributed_lines, total_lines)`.
/// Ties break to the lexically-smallest sha for determinism. None if empty.
pub(super) fn dominant_culprit(shas: &[String]) -> Option<(String, usize, usize)> {
    use std::collections::HashMap;
    if shas.is_empty() {
        return None;
    }
    let total = shas.len();
    let mut counts: HashMap<&str, usize> = HashMap::new();
    for s in shas {
        *counts.entry(s.as_str()).or_default() += 1;
    }
    // Max by count, then smallest sha. Iterate sorted for deterministic tie-break.
    let mut pairs: Vec<(&str, usize)> = counts.into_iter().collect();
    pairs.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(b.0)));
    let (sha, n) = pairs[0];
    Some((sha.to_string(), n, total))
}

/// Confidence for a blame-derived culprit. Blame is a precise signal, so it is
/// floored at 0.6 (above the recency+overlap heuristic's typical range) and
/// scales toward 0.98 as the culprit's share of the blamed lines approaches 1.0.
pub(super) fn blame_confidence(attributed: usize, total: usize) -> f64 {
    let frac = attributed as f64 / (total.max(1) as f64);
    (0.6 + 0.38 * frac).clamp(0.6, 0.98)
}

/// Merge the recency+overlap `fallback` candidates with git-blame results
/// (per-file lists of culprit shas). Where blame produced a dominant culprit for
/// a file, it replaces that file's fallback candidate (sharper, exact commit);
/// files with no blame keep their fallback; files seen only by blame are added.
/// Deterministic order: confidence desc, then file asc.
pub(super) fn merge_causality(
    fallback: Vec<CausalityCandidate>,
    blame_map: &std::collections::HashMap<String, Vec<String>>,
) -> Vec<CausalityCandidate> {
    use std::collections::HashMap;
    let mut by_file: HashMap<String, CausalityCandidate> =
        fallback.into_iter().map(|c| (c.file.clone(), c)).collect();

    for (file, shas) in blame_map {
        if let Some((sha, attributed, total)) = dominant_culprit(shas) {
            by_file.insert(
                file.clone(),
                CausalityCandidate {
                    file: file.clone(),
                    culprit_sha: sha,
                    culprit_bead_id: String::new(), // unknown from blame alone
                    confidence: blame_confidence(attributed, total),
                },
            );
        }
    }

    let mut out: Vec<CausalityCandidate> = by_file.into_values().collect();
    out.sort_by(|a, b| {
        b.confidence
            .partial_cmp(&a.confidence)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.file.cmp(&b.file))
    });
    out
}

/// Files changed in a commit plus aggregate line counts, via `git show
/// --numstat`. Each numstat line is `added<TAB>deleted<TAB>path`; binary files
/// emit `-` for added/deleted and contribute 0. Paths are repo-relative.
pub(super) fn commit_numstat(repo_root: &Path, sha: &str) -> Result<(Vec<String>, i64, i64)> {
    let out = Command::new("git")
        .current_dir(repo_root)
        .args(["show", "--numstat", "--pretty=format:", sha])
        .output()
        .context("Failed to run git")?;
    if !out.status.success() {
        return Err(anyhow!(
            "git show failed for {}: {}",
            sha,
            String::from_utf8_lossy(&out.stderr)
        ));
    }
    Ok(parse_numstat(&String::from_utf8_lossy(&out.stdout)))
}

/// Parse `git --numstat` output into (files, total_added, total_deleted). Each
/// line is `added<TAB>deleted<TAB>path`; binary files emit `-` and contribute 0.
pub(super) fn parse_numstat(stdout: &str) -> (Vec<String>, i64, i64) {
    let mut files = Vec::new();
    let mut total_added = 0i64;
    let mut total_deleted = 0i64;
    for line in stdout.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let mut parts = line.splitn(3, '\t');
        let added = parts.next().unwrap_or("-");
        let deleted = parts.next().unwrap_or("-");
        let path = match parts.next() {
            Some(p) if !p.trim().is_empty() => p.trim().to_string(),
            _ => continue,
        };
        // Binary files report '-'; parse failures count as 0.
        total_added += added.parse::<i64>().unwrap_or(0);
        total_deleted += deleted.parse::<i64>().unwrap_or(0);
        files.push(path);
    }
    (files, total_added, total_deleted)
}
