use anyhow::{anyhow, Context, Result};
use clap::{Args, Subcommand};
use colored::Colorize;
use serde::Serialize;
use std::path::Path;
use std::process::Command;

use super::OutputConfig;
use crate::config::Config;
use crate::index::Parser;
use crate::storage::sqlite::{
    BeadLineageRecord, MetadataStore, NewBeadLineage, NewBugCausality, PriorTouch, TouchedSymbol,
};

#[derive(Args)]
pub struct BeadArgs {
    #[command(subcommand)]
    command: BeadCommand,
}

#[derive(Subcommand)]
enum BeadCommand {
    /// Link a bead to a commit and its changeset (workflow telemetry, GH#9)
    Link {
        /// Bead identifier (e.g. bo-abc123)
        bead_id: String,

        /// Commit SHA the bead was resolved in. When given and `--files` is
        /// omitted, the changeset is read from git automatically.
        commit: Option<String>,

        /// Explicit touched files (comma-separated). Overrides git detection.
        #[arg(long)]
        files: Option<String>,

        /// Bead type (task | bug | feature | chore)
        #[arg(long, name = "type")]
        bead_type: Option<String>,

        /// Associated bundle slugs (comma-separated)
        #[arg(long)]
        bundles: Option<String>,

        /// Action type (linked | referenced | completed)
        #[arg(long, default_value = "linked")]
        action: String,
    },

    /// Auto-link a commit to its bead from the commit message / branch name.
    /// Invoked by the git post-commit hook. Extracts the bead id, then records
    /// one `commit` lineage row (idempotent). No bead id found → exit 0 silently.
    AutoLink {
        /// Commit-ish to link (default: HEAD).
        #[arg(long, default_value = "HEAD")]
        commit: String,
    },

    /// Reconstruct bug causality: for each bug bead, infer which prior commit
    /// most likely introduced the bug it fixed (per file) and populate the
    /// `bug_causality` table. Idempotent — safe to run periodically. (bo-s1kb)
    ReconstructCausality {
        /// Restrict to a single bug bead id (default: all bug beads in lineage).
        #[arg(long)]
        bug: Option<String>,

        /// Max bug beads to process when scanning all (default: 200).
        #[arg(long, default_value = "200")]
        limit: usize,
    },

    /// Show recorded lineage for a bead (or recent lineage across all beads)
    History {
        /// Bead identifier to filter by (omit for recent lineage across beads)
        bead_id: Option<String>,

        /// Filter by commit SHA
        #[arg(long)]
        commit: Option<String>,

        /// Maximum number of records
        #[arg(long, short = 'n', default_value = "20")]
        limit: usize,
    },
}

#[derive(Serialize)]
struct LinkOutput {
    id: i64,
    bead_id: String,
    commit_sha: Option<String>,
    touched_files: Vec<String>,
}

#[derive(Serialize)]
struct HistoryEntry {
    id: i64,
    created_at: String,
    bead_id: String,
    bead_type: Option<String>,
    commit_sha: Option<String>,
    bundle_slugs: Option<String>,
    touched_files: Vec<String>,
    action_type: Option<String>,
    feature_id: Option<String>,
    lines_added: Option<i64>,
    lines_deleted: Option<i64>,
    touched_symbols: Vec<TouchedSymbol>,
}

impl From<BeadLineageRecord> for HistoryEntry {
    fn from(r: BeadLineageRecord) -> Self {
        HistoryEntry {
            id: r.id,
            created_at: r.created_at,
            bead_id: r.bead_id,
            bead_type: r.bead_type,
            commit_sha: r.commit_sha,
            bundle_slugs: r.bundle_slugs,
            touched_files: r.touched_files,
            action_type: r.action_type,
            feature_id: r.feature_id,
            lines_added: r.lines_added,
            lines_deleted: r.lines_deleted,
            touched_symbols: r.touched_symbols,
        }
    }
}

pub async fn run(args: BeadArgs, output: OutputConfig) -> Result<()> {
    let repo_root = super::find_bobbin_root()
        .ok_or_else(|| anyhow!("Not inside a bobbin repository (run `bobbin init` first)"))?;
    let store = MetadataStore::open(&Config::db_path(&repo_root))
        .context("Failed to open metadata store")?;

    match args.command {
        BeadCommand::Link {
            bead_id,
            commit,
            files,
            bead_type,
            bundles,
            action,
        } => {
            // Resolve touched files + line counts: explicit --files wins (no
            // line counts available), else derive from the commit via numstat.
            let (touched_files, lines_added, lines_deleted): (Vec<String>, Option<i64>, Option<i64>) =
                if let Some(f) = files {
                    let parsed = f
                        .split(',')
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                        .collect();
                    (parsed, None, None)
                } else if let Some(ref sha) = commit {
                    match commit_numstat(&repo_root, sha) {
                        Ok((files, added, deleted)) => (files, Some(added), Some(deleted)),
                        Err(_) => (Vec::new(), None, None),
                    }
                } else {
                    (Vec::new(), None, None)
                };

            // bundle_slugs (edge E2): explicit --bundles wins, else derive from
            // the bead's `b:<slug>` labels.
            let bundle_slugs = bundles.or_else(|| bundle_slugs_from_labels(&bead_id));

            // feature_id (edge E1 'implements'): walk deps to a feature ancestor.
            let feature_id = resolve_feature_id(&bead_id);

            // touched_symbols (best-effort): parse each committed file version.
            let touched_symbols = match commit.as_ref() {
                Some(sha) => extract_touched_symbols(&repo_root, sha, &touched_files),
                None => Vec::new(),
            };

            let id = store.record_bead_lineage(&NewBeadLineage {
                bead_id: bead_id.clone(),
                bead_type,
                commit_sha: commit.clone(),
                bundle_slugs,
                touched_files: touched_files.clone(),
                action_type: Some(action),
                feature_id,
                lines_added,
                lines_deleted,
                touched_symbols,
            })?;

            if output.json {
                let out = LinkOutput {
                    id,
                    bead_id,
                    commit_sha: commit,
                    touched_files,
                };
                println!("{}", serde_json::to_string_pretty(&out)?);
            } else if !output.quiet {
                println!(
                    "{} Linked {} {} ({} file{})",
                    "✓".green(),
                    bead_id.cyan(),
                    commit
                        .as_deref()
                        .map(|c| format!("→ {}", &c[..c.len().min(8)]))
                        .unwrap_or_default(),
                    touched_files.len(),
                    if touched_files.len() == 1 { "" } else { "s" },
                );
            }
        }

        BeadCommand::AutoLink { commit } => {
            run_auto_link(&repo_root, &store, &commit, &output)?;
        }

        BeadCommand::ReconstructCausality { bug, limit } => {
            run_reconstruct_causality(&store, bug.as_deref(), limit, &output)?;
        }

        BeadCommand::History {
            bead_id,
            commit,
            limit,
        } => {
            let records =
                store.list_bead_lineage(bead_id.as_deref(), commit.as_deref(), limit)?;

            if output.json {
                let entries: Vec<HistoryEntry> =
                    records.into_iter().map(HistoryEntry::from).collect();
                println!("{}", serde_json::to_string_pretty(&entries)?);
            } else if !output.quiet {
                if records.is_empty() {
                    println!("{}", "No bead lineage recorded yet.".dimmed());
                } else {
                    for r in &records {
                        let sha = r
                            .commit_sha
                            .as_deref()
                            .map(|c| &c[..c.len().min(8)])
                            .unwrap_or("-");
                        println!(
                            "{}  {}  {}  {} file(s)  {}",
                            r.created_at.dimmed(),
                            r.bead_id.cyan(),
                            sha.yellow(),
                            r.touched_files.len(),
                            r.action_type.as_deref().unwrap_or("").dimmed(),
                        );
                    }
                }
            }
        }
    }

    Ok(())
}

/// Auto-link a commit to its bead (bo-5em9). Resolves the bead id from the
/// commit message / branch, then records exactly one `commit` lineage row,
/// enriched with the same numstat / feature / symbol data as a manual `link`.
///
/// Failure-isolated by design: the post-commit hook backgrounds this and
/// discards output, so a missing bead, a non-bobbin repo, or a git error must
/// never break the commit. No bead id found → no row, exit Ok silently.
/// Idempotent: a re-fired hook (amend / rebase) does not create a duplicate.
fn run_auto_link(
    repo_root: &Path,
    store: &MetadataStore,
    commit_ref: &str,
    output: &OutputConfig,
) -> Result<()> {
    // Resolve the commit-ish to a full sha. A bad ref (e.g. no commits yet) is
    // not an error worth surfacing from a post-commit hook — just stop.
    let sha = match resolve_commit_sha(repo_root, commit_ref) {
        Some(s) => s,
        None => return Ok(()),
    };

    let message = commit_message(repo_root, &sha).unwrap_or_default();
    let branch = current_branch(repo_root);

    let bead_id = match extract_bead_id(&message, branch.as_deref()) {
        Some(b) => b,
        None => return Ok(()), // not every commit references a bead
    };

    // Idempotency: skip if this (bead, commit) already has a `commit` row. We
    // query by both keys and check action_type to avoid clobbering a manual
    // `link`/`referenced` row for the same pair.
    let existing = store.list_bead_lineage(Some(&bead_id), Some(&sha), 50)?;
    if existing
        .iter()
        .any(|r| r.action_type.as_deref() == Some("commit"))
    {
        if !output.quiet && !output.json {
            println!(
                "{} {} already linked to {}",
                "•".dimmed(),
                bead_id.cyan(),
                (&sha[..sha.len().min(8)]).yellow()
            );
        }
        return Ok(());
    }

    let (touched_files, lines_added, lines_deleted) = match commit_numstat(repo_root, &sha) {
        Ok((files, added, deleted)) => (files, Some(added), Some(deleted)),
        Err(_) => (Vec::new(), None, None),
    };
    let bundle_slugs = bundle_slugs_from_labels(&bead_id);
    let feature_id = resolve_feature_id(&bead_id);
    let touched_symbols = extract_touched_symbols(repo_root, &sha, &touched_files);

    let id = store.record_bead_lineage(&NewBeadLineage {
        bead_id: bead_id.clone(),
        bead_type: None,
        commit_sha: Some(sha.clone()),
        bundle_slugs,
        touched_files: touched_files.clone(),
        action_type: Some("commit".to_string()),
        feature_id,
        lines_added,
        lines_deleted,
        touched_symbols,
    })?;

    if output.json {
        let out = LinkOutput {
            id,
            bead_id,
            commit_sha: Some(sha),
            touched_files,
        };
        println!("{}", serde_json::to_string_pretty(&out)?);
    } else if !output.quiet {
        println!(
            "{} Auto-linked {} → {} ({} file{})",
            "✓".green(),
            bead_id.cyan(),
            (&sha[..sha.len().min(8)]).yellow(),
            touched_files.len(),
            if touched_files.len() == 1 { "" } else { "s" },
        );
    }

    Ok(())
}

/// Resolve a commit-ish to a full sha via `git rev-parse`. None on any failure.
fn resolve_commit_sha(repo_root: &Path, commit_ref: &str) -> Option<String> {
    let out = Command::new("git")
        .current_dir(repo_root)
        .args(["rev-parse", "--verify", commit_ref])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let sha = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if sha.is_empty() {
        None
    } else {
        Some(sha)
    }
}

/// Full commit message body (`%B`) for a sha. None on any failure.
fn commit_message(repo_root: &Path, sha: &str) -> Option<String> {
    let out = Command::new("git")
        .current_dir(repo_root)
        .args(["show", "-s", "--format=%B", sha])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).to_string())
}

/// Current branch name (`git rev-parse --abbrev-ref HEAD`). None on detached
/// HEAD ("HEAD") or any failure.
fn current_branch(repo_root: &Path) -> Option<String> {
    let out = Command::new("git")
        .current_dir(repo_root)
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let b = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if b.is_empty() || b == "HEAD" {
        None
    } else {
        Some(b)
    }
}

/// Extract a bead id from a commit, preferring the most explicit source.
/// Precedence: (a) a `Bead: <id>` trailer, (b) a parenthesized bead id in the
/// subject (e.g. conventional-commit `(bo-xxxx)`), (c) the first bead id token
/// in the subject, (d) the branch name. First match wins; None if none found.
fn extract_bead_id(message: &str, branch: Option<&str>) -> Option<String> {
    let subject = message.lines().next().unwrap_or("");

    // (a) explicit `Bead: <id>` trailer anywhere in the message (case-insensitive key).
    for line in message.lines() {
        let mut parts = line.trim().splitn(2, ':');
        if let (Some(key), Some(val)) = (parts.next(), parts.next()) {
            if key.trim().eq_ignore_ascii_case("bead") {
                if let Some(id) = find_bead_id(val) {
                    return Some(id);
                }
            }
        }
    }

    // (b) parenthesized bead id in the subject.
    if let Some(id) = parenthesized_bead_id(subject) {
        return Some(id);
    }

    // (c) first bead id token anywhere in the subject.
    if let Some(id) = find_bead_id(subject) {
        return Some(id);
    }

    // (d) branch name (last resort).
    branch.and_then(find_bead_id)
}

/// First bead id appearing inside a parenthesized group of `subject`, scanning
/// left to right. Handles nested/multiple groups; returns the first match.
fn parenthesized_bead_id(subject: &str) -> Option<String> {
    let mut depth = 0usize;
    let mut start = 0usize;
    let mut groups: Vec<&str> = Vec::new();
    for (i, c) in subject.char_indices() {
        if c == '(' {
            if depth == 0 {
                start = i + 1;
            }
            depth += 1;
        } else if c == ')' && depth > 0 {
            depth -= 1;
            if depth == 0 {
                groups.push(&subject[start..i]);
            }
        }
    }
    groups.iter().find_map(|g| find_bead_id(g))
}

/// Find the first bead-id-shaped substring in `text`: a lowercase-ascii prefix,
/// a `-`, then an alphanumeric suffix of length >= 3 (e.g. `bo-5em9`). Mirrors
/// the is_bead_command heuristic in hook.rs but scans for an embedded match so
/// it also works on branch names like `bo-5em9-autolink-hook`.
fn find_bead_id(text: &str) -> Option<String> {
    let chars: Vec<char> = text.chars().collect();
    let n = chars.len();
    let mut i = 0;
    while i < n {
        // A prefix must begin at a boundary (start, or after a non-alphanumeric,
        // non-dash char) so we don't match mid-token (e.g. the `o-foo` in `bo-foo`).
        let at_boundary =
            i == 0 || (!chars[i - 1].is_ascii_alphanumeric() && chars[i - 1] != '-');
        if at_boundary && chars[i].is_ascii_lowercase() {
            let mut j = i;
            while j < n && chars[j].is_ascii_lowercase() {
                j += 1;
            }
            if j < n && chars[j] == '-' {
                let suffix_start = j + 1;
                let mut k = suffix_start;
                while k < n && chars[k].is_ascii_alphanumeric() {
                    k += 1;
                }
                if k - suffix_start >= 3 {
                    return Some(chars[i..k].iter().collect());
                }
            }
            // No match from this prefix; resume scanning after the lowercase run.
            i = j.max(i + 1);
            continue;
        }
        i += 1;
    }
    None
}

/// A reconstructed culprit for one file of a bug's fix (bo-s1kb).
#[derive(Debug, Clone, PartialEq)]
struct CausalityCandidate {
    file: String,
    culprit_sha: String,
    culprit_bead_id: String,
    confidence: f64,
}

#[derive(Serialize)]
struct CausalityOutput {
    bug_id: String,
    rows: usize,
}

/// Reconstruct bug causality and populate `bug_causality` (bo-s1kb).
///
/// For each bug bead: gather the files its fix touched, find the most-recent
/// prior commit touching each such file (the candidate culprit), score
/// confidence by how much of the fix's changeset that commit overlaps, and
/// upsert one row per (bug, culprit, file). Idempotent via the table's UNIQUE
/// constraint, so periodic re-runs refresh rather than duplicate.
///
/// Scope (v1): the recency+overlap heuristic over `bead_lineage`. Git-blame
/// sharpening of the exact introducing commit and `change_events` outcome
/// labeling are deferred (see bo-s1kb design notes) — neither blocks the
/// supervised signal this table provides.
fn run_reconstruct_causality(
    store: &MetadataStore,
    bug: Option<&str>,
    limit: usize,
    output: &OutputConfig,
) -> Result<()> {
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

        let candidates = reconstruct_culprits(&fix_files, &prior);
        for c in &candidates {
            store.record_bug_causality(&NewBugCausality {
                bug_id: bug_id.clone(),
                culprit_sha: Some(c.culprit_sha.clone()),
                culprit_bead_id: Some(c.culprit_bead_id.clone()),
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
fn reconstruct_culprits(fix_files: &[String], prior: &[PriorTouch]) -> Vec<CausalityCandidate> {
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

/// Files changed in a commit plus aggregate line counts, via `git show
/// --numstat`. Each numstat line is `added<TAB>deleted<TAB>path`; binary files
/// emit `-` for added/deleted and contribute 0. Paths are repo-relative.
fn commit_numstat(repo_root: &Path, sha: &str) -> Result<(Vec<String>, i64, i64)> {
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
fn parse_numstat(stdout: &str) -> (Vec<String>, i64, i64) {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_numstat_basic() {
        let out = "10\t2\tsrc/a.rs\n5\t0\tsrc/b.rs\n";
        let (files, added, deleted) = parse_numstat(out);
        assert_eq!(files, vec!["src/a.rs", "src/b.rs"]);
        assert_eq!(added, 15);
        assert_eq!(deleted, 2);
    }

    #[test]
    fn test_parse_numstat_binary_counts_zero() {
        // Binary files emit '-' for both columns and must contribute 0.
        let out = "-\t-\tassets/logo.png\n3\t1\tsrc/a.rs\n";
        let (files, added, deleted) = parse_numstat(out);
        assert_eq!(files, vec!["assets/logo.png", "src/a.rs"]);
        assert_eq!(added, 3);
        assert_eq!(deleted, 1);
    }

    #[test]
    fn test_parse_numstat_empty() {
        let (files, added, deleted) = parse_numstat("\n\n");
        assert!(files.is_empty());
        assert_eq!(added, 0);
        assert_eq!(deleted, 0);
    }

    #[test]
    fn test_find_bead_id_basic() {
        assert_eq!(find_bead_id("fix bo-5em9 now"), Some("bo-5em9".to_string()));
        assert_eq!(find_bead_id("aegis-abc123"), Some("aegis-abc123".to_string()));
        // Suffix must be >= 3 chars.
        assert_eq!(find_bead_id("a-bc"), None);
        // No dash → not a bead id.
        assert_eq!(find_bead_id("hello world"), None);
        // Suffix stops at the next dash (branch-style names).
        assert_eq!(
            find_bead_id("bo-5em9-autolink-hook"),
            Some("bo-5em9".to_string())
        );
    }

    #[test]
    fn test_find_bead_id_requires_lowercase_prefix() {
        // Uppercase prefix is not a bead id.
        assert_eq!(find_bead_id("BO-5EM9"), None);
        // Digit-led token is not a bead id (no lowercase prefix).
        assert_eq!(find_bead_id("123-abc"), None);
    }

    #[test]
    fn test_parenthesized_bead_id() {
        assert_eq!(
            parenthesized_bead_id("feat(config): surface knobs (bo-qlfu)"),
            Some("bo-qlfu".to_string())
        );
        // The conventional-commit scope `(config)` is not a bead id; the trailing
        // `(bo-qlfu)` is.
        assert_eq!(parenthesized_bead_id("chore: tidy (cleanup)"), None);
    }

    #[test]
    fn test_extract_bead_id_trailer_wins() {
        let msg = "feat(x): do a thing (bo-aaa111)\n\nBody.\n\nBead: bo-zzz999\n";
        assert_eq!(extract_bead_id(msg, None), Some("bo-zzz999".to_string()));
    }

    #[test]
    fn test_extract_bead_id_paren_over_plain() {
        // Subject has both a plain token and a parenthesized one; paren wins.
        let msg = "bo-aaa111 relates to feature (bo-bbb222)";
        assert_eq!(extract_bead_id(msg, None), Some("bo-bbb222".to_string()));
    }

    #[test]
    fn test_extract_bead_id_subject_token() {
        let msg = "wip on bo-ccc333 stuff";
        assert_eq!(extract_bead_id(msg, None), Some("bo-ccc333".to_string()));
    }

    #[test]
    fn test_extract_bead_id_branch_fallback() {
        // No bead id in the message → fall back to the branch name.
        let msg = "wip: misc cleanup";
        assert_eq!(
            extract_bead_id(msg, Some("bo-5em9-autolink-hook")),
            Some("bo-5em9".to_string())
        );
    }

    #[test]
    fn test_extract_bead_id_none() {
        assert_eq!(extract_bead_id("just a normal commit", None), None);
        assert_eq!(extract_bead_id("just a normal commit", Some("main")), None);
    }

    fn touch(bead: &str, sha: &str, file: &str, at: &str) -> PriorTouch {
        PriorTouch {
            bead_id: bead.to_string(),
            commit_sha: Some(sha.to_string()),
            file: file.to_string(),
            created_at: at.to_string(),
        }
    }

    #[test]
    fn test_reconstruct_culprits_picks_most_recent() {
        let fix_files = vec!["src/a.rs".to_string()];
        // Two prior commits touched a.rs; the more recent (DESC-first) wins.
        let prior = vec![
            touch("bo-new", "sha_new", "src/a.rs", "2026-06-20T00:00:00Z"),
            touch("bo-old", "sha_old", "src/a.rs", "2026-06-01T00:00:00Z"),
        ];
        let got = reconstruct_culprits(&fix_files, &prior);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].culprit_sha, "sha_new");
        assert_eq!(got[0].culprit_bead_id, "bo-new");
        // Single fix file fully overlapped → max confidence.
        assert!((got[0].confidence - 0.95).abs() < 1e-9);
    }

    #[test]
    fn test_reconstruct_culprits_confidence_scales_with_overlap() {
        let fix_files = vec![
            "src/a.rs".to_string(),
            "src/b.rs".to_string(),
            "src/c.rs".to_string(),
            "src/d.rs".to_string(),
        ];
        // sha_wide touched 2 of the 4 fix files; sha_narrow touched only 1.
        let prior = vec![
            touch("bo-wide", "sha_wide", "src/a.rs", "2026-06-10T00:00:00Z"),
            touch("bo-wide", "sha_wide", "src/b.rs", "2026-06-10T00:00:00Z"),
            touch("bo-narrow", "sha_narrow", "src/c.rs", "2026-06-09T00:00:00Z"),
        ];
        let got = reconstruct_culprits(&fix_files, &prior);
        // a.rs and b.rs → sha_wide (2/4 = 0.5); c.rs → sha_narrow (1/4 = 0.25).
        let a = got.iter().find(|c| c.file == "src/a.rs").unwrap();
        let c = got.iter().find(|c| c.file == "src/c.rs").unwrap();
        assert_eq!(a.culprit_sha, "sha_wide");
        assert!((a.confidence - 0.5).abs() < 1e-9);
        assert!((c.confidence - 0.25).abs() < 1e-9);
        // Sorted by confidence desc → wider-blame culprit first.
        assert!(got[0].confidence >= got[got.len() - 1].confidence);
    }

    #[test]
    fn test_reconstruct_culprits_ignores_unrelated_files_and_empty_sha() {
        let fix_files = vec!["src/a.rs".to_string()];
        let prior = vec![
            // Touches a file the fix didn't → ignored.
            touch("bo-x", "sha_x", "src/other.rs", "2026-06-10T00:00:00Z"),
            // Empty sha → skipped.
            PriorTouch {
                bead_id: "bo-y".to_string(),
                commit_sha: None,
                file: "src/a.rs".to_string(),
                created_at: "2026-06-11T00:00:00Z".to_string(),
            },
        ];
        assert!(reconstruct_culprits(&fix_files, &prior).is_empty());
    }
}

/// Fetch a bead as JSON via `bd show <id> --json`. bd may emit a single object
/// or a one-element array; this normalizes to the first object. Best-effort:
/// returns None on any failure so lineage recording never hard-fails on
/// telemetry enrichment.
fn bead_json(bead_id: &str) -> Option<serde_json::Value> {
    let out = Command::new("bd")
        .args(["show", bead_id, "--json"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).ok()?;
    match v {
        serde_json::Value::Array(arr) => arr.into_iter().next(),
        other => Some(other),
    }
}

/// Derive bundle slugs from the bead's `b:<slug>` labels (edge E2). Best-effort:
/// returns None if bd is unavailable or no bundle labels are present.
fn bundle_slugs_from_labels(bead_id: &str) -> Option<String> {
    let v = bead_json(bead_id)?;
    let labels = v.get("labels")?.as_array()?;
    let slugs: Vec<String> = labels
        .iter()
        .filter_map(|l| l.as_str())
        .filter_map(|l| l.strip_prefix("b:"))
        .map(|s| s.to_string())
        .filter(|s| !s.is_empty())
        .collect();
    if slugs.is_empty() {
        None
    } else {
        Some(slugs.join(","))
    }
}

/// Resolve the feature ancestor of a bead by walking its dependency graph (edge
/// E1 'implements'). Returns the id of the first `feature`-typed ancestor found,
/// or None. Best-effort: cycle-guarded (visited set), depth-capped at 10, and
/// NULL on any bd failure.
fn resolve_feature_id(bead_id: &str) -> Option<String> {
    use std::collections::HashSet;
    let mut visited: HashSet<String> = HashSet::new();
    let mut frontier: Vec<String> = vec![bead_id.to_string()];
    let mut depth = 0;
    while !frontier.is_empty() && depth < 10 {
        let mut next: Vec<String> = Vec::new();
        for id in frontier {
            if !visited.insert(id.clone()) {
                continue;
            }
            let v = match bead_json(&id) {
                Some(v) => v,
                None => continue,
            };
            if let Some(deps) = v.get("dependencies").and_then(|d| d.as_array()) {
                for dep in deps {
                    let dep_id = match dep.get("id").and_then(|i| i.as_str()) {
                        Some(i) => i,
                        None => continue,
                    };
                    let dep_type = dep
                        .get("issue_type")
                        .and_then(|t| t.as_str())
                        .unwrap_or("");
                    if dep_type == "feature" {
                        return Some(dep_id.to_string());
                    }
                    next.push(dep_id.to_string());
                }
            }
        }
        frontier = next;
        depth += 1;
    }
    None
}

/// Best-effort symbol extraction for a commit's changed files. For each file we
/// parse its committed version (`git show <sha>:<path>`) and collect named
/// chunks. Binary / unparseable / deleted files are skipped silently.
fn extract_touched_symbols(repo_root: &Path, sha: &str, files: &[String]) -> Vec<TouchedSymbol> {
    let mut parser = match Parser::new() {
        Ok(p) => p,
        Err(_) => return Vec::new(),
    };
    let mut out = Vec::new();
    for file in files {
        let blob = Command::new("git")
            .current_dir(repo_root)
            .args(["show", &format!("{}:{}", sha, file)])
            .output();
        let content = match blob {
            Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).into_owned(),
            _ => continue, // deleted / binary / missing at this revision
        };
        let chunks = match parser.parse_file(Path::new(file), &content) {
            Ok(c) => c,
            Err(_) => continue,
        };
        for chunk in chunks {
            if let Some(name) = chunk.name {
                out.push(TouchedSymbol {
                    file: file.clone(),
                    symbol: name,
                    kind: chunk.chunk_type.to_string(),
                });
            }
        }
    }
    out
}
