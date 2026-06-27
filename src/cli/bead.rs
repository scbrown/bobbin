use anyhow::{anyhow, Context, Result};
use clap::{Args, Subcommand};
use colored::Colorize;
use serde::Serialize;
use std::path::Path;
use std::process::Command;

use super::OutputConfig;
use crate::config::Config;
use crate::storage::sqlite::{BeadLineageRecord, MetadataStore, NewBeadLineage};

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
            // Resolve touched files: explicit --files wins, else derive from commit.
            let touched_files: Vec<String> = if let Some(f) = files {
                f.split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect()
            } else if let Some(ref sha) = commit {
                files_in_commit(&repo_root, sha).unwrap_or_default()
            } else {
                Vec::new()
            };

            let id = store.record_bead_lineage(&NewBeadLineage {
                bead_id: bead_id.clone(),
                bead_type,
                commit_sha: commit.clone(),
                bundle_slugs: bundles,
                touched_files: touched_files.clone(),
                action_type: Some(action),
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

/// Return the list of files changed in a commit using git, repo-relative.
fn files_in_commit(repo_root: &Path, sha: &str) -> Result<Vec<String>> {
    let out = Command::new("git")
        .current_dir(repo_root)
        .args(["show", "--name-only", "--pretty=format:", sha])
        .output()
        .context("Failed to run git")?;
    if !out.status.success() {
        return Err(anyhow!(
            "git show failed for {}: {}",
            sha,
            String::from_utf8_lossy(&out.stderr)
        ));
    }
    let files = String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect();
    Ok(files)
}
