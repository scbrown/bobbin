use anyhow::{anyhow, Context, Result};
use clap::{Args, Subcommand};
use colored::Colorize;
use serde::Serialize;

use crate::cli::OutputConfig;
use crate::config::Config;
use crate::storage::sqlite::{
    BeadLineageRecord, MetadataStore, NewBeadLineage, TouchedSymbol,
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

    /// Report work items already recorded that look like near-duplicates of a
    /// proposed one. Advisory: prints candidates and exits 0 regardless, so it
    /// can be wired into a creation hook without ever blocking a write.
    ///
    /// Every verdict carries its score, threshold, model and a corpus
    /// watermark, so the suggestion can be checked rather than taken on trust.
    Similar {
        /// Title of the proposed work item.
        title: String,

        /// Description of the proposed work item, scored alongside the title.
        #[arg(long)]
        description: Option<String>,

        /// Minimum similarity score to report.
        #[arg(long, default_value = "0.55")]
        threshold: f32,

        /// Maximum candidates to report.
        #[arg(long, default_value = "5")]
        limit: usize,

        /// Also consider closed items (default: open only, since a closed
        /// duplicate is history rather than a collision).
        #[arg(long)]
        all_statuses: bool,
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
    // Dispatched before the metadata store is opened: `similar` reads only the
    // vector index, and opening a store it does not use would make it fail in
    // repositories where the rest of `bobbin bead` legitimately cannot run.
    if let BeadCommand::Similar {
        title,
        description,
        threshold,
        limit,
        all_statuses,
    } = &args.command
    {
        return run_similar(
            title,
            description.as_deref(),
            *threshold,
            *limit,
            !all_statuses,
            &output,
        )
        .await;
    }

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

        // Handled above, before the store is opened.
        BeadCommand::Similar { .. } => unreachable!("dispatched before store open"),
        BeadCommand::ReconstructCausality { bug, limit } => {
            run_reconstruct_causality(&repo_root, &store, bug.as_deref(), limit, &output)?;
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

mod autolink;
mod causality;
mod helpers;
mod similar;

use causality::commit_numstat;
use helpers::*;

use autolink::run_auto_link;
use similar::run_similar;
use causality::run_reconstruct_causality;

#[cfg(test)]
mod tests;
