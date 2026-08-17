//! `bobbin bead auto-link` — infer a bead id from a commit and record lineage.
//!
//! Split out of the former `src/cli/bead.rs` (bobbin-aoz), which was the
//! largest non-allowlisted file in the tree at 1,179 lines.

use anyhow::Result;
use colored::Colorize;
use std::path::Path;
use std::process::Command;

use crate::storage::sqlite::{
    MetadataStore, NewBeadLineage,
};

use super::causality::commit_numstat;
use super::helpers::*;
use super::LinkOutput;
use crate::cli::OutputConfig;

pub(super) fn run_auto_link(
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
pub(super) fn extract_bead_id(message: &str, branch: Option<&str>) -> Option<String> {
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
pub(super) fn parenthesized_bead_id(subject: &str) -> Option<String> {
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
pub(super) fn find_bead_id(text: &str) -> Option<String> {
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

