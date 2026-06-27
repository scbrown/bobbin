use anyhow::{anyhow, Context, Result};
use clap::{Args, Subcommand};
use colored::Colorize;
use serde::Serialize;
use std::path::Path;
use std::process::Command;

use super::OutputConfig;
use crate::config::Config;
use crate::index::Parser;
use crate::storage::sqlite::{BeadLineageRecord, MetadataStore, NewBeadLineage, TouchedSymbol};

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
