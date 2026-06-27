use anyhow::{anyhow, Context, Result};
use clap::Args;
use colored::Colorize;
use serde::Serialize;
use std::collections::{BTreeSet, HashMap};

use super::OutputConfig;
use crate::config::Config;
use crate::storage::sqlite::{BeadLineageRecord, BugCausalityRecord, MetadataStore};

/// Upper bound on rows pulled for the join. The reconcile view is designed for
/// the tens-to-hundreds-of-rows regime (GH#9 telemetry); this cap keeps a
/// pathological store from blowing up memory while comfortably covering real use.
const SCAN_LIMIT: usize = 100_000;

#[derive(Args)]
pub struct ReconcileArgs {
    /// Restrict the view to a single change bead id (omit for all changes).
    bead: Option<String>,

    /// Max change events to emit (most-recent change first).
    #[arg(long, short = 'n', default_value = "50")]
    limit: usize,
}

/// One unified change_event row (bo-mu4m): a change bead joined to the commits
/// it produced, the files/bundles it touched, and — via `bug_causality` — the
/// bugs later blamed on it plus the supervised outcome labels bo-6i55/L4
/// predictors train on (`introduced_bug`, `ttf_days`).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ChangeEvent {
    pub change_bead_id: String,
    pub feature_id: Option<String>,
    pub commit_shas: Vec<String>,
    pub touched_files: Vec<String>,
    pub bundle_slugs: Vec<String>,
    pub bug_ids: Vec<String>,
    pub lines_added: i64,
    pub lines_deleted: i64,
    /// Supervised label: did a later bug get blamed on this change?
    pub introduced_bug: bool,
    /// Supervised label: days from this change to the first bug blamed on it
    /// (None when the change introduced no bug, or timestamps are unusable).
    pub ttf_days: Option<f64>,
    /// Earliest lineage timestamp for this change (the t0 for `ttf_days`).
    pub changed_at: String,
}

pub async fn run(args: ReconcileArgs, output: OutputConfig) -> Result<()> {
    let repo_root = super::find_bobbin_root()
        .ok_or_else(|| anyhow!("Not inside a bobbin repository (run `bobbin init` first)"))?;
    let store = MetadataStore::open(&Config::db_path(&repo_root))
        .context("Failed to open metadata store")?;

    let lineage = store.list_bead_lineage(args.bead.as_deref(), None, SCAN_LIMIT)?;
    // Causality is fetched whole regardless of the bead filter: a change's bug
    // labels come from causality rows keyed on it as the *culprit*, not the bug.
    let causality = store.list_bug_causality(None, SCAN_LIMIT)?;

    // The bead→earliest-timestamp map (used for ttf) must reflect *all* beads,
    // including bug beads that the filtered lineage may exclude. When a bead
    // filter is in play, fetch the full set for timing only.
    let timing_rows = if args.bead.is_some() {
        store.list_bead_lineage(None, None, SCAN_LIMIT)?
    } else {
        Vec::new()
    };
    let earliest = earliest_by_bead(if args.bead.is_some() {
        &timing_rows
    } else {
        &lineage
    });

    let mut events = build_change_events(&lineage, &causality, &earliest);
    events.truncate(args.limit);

    if output.json {
        println!("{}", serde_json::to_string_pretty(&events)?);
    } else if !output.quiet {
        print_table(&events);
    }

    Ok(())
}

/// Earliest `created_at` per bead across lineage rows. This is the change's t0
/// for time-to-first-bug and the bug's "discovered at" reference.
fn earliest_by_bead(lineage: &[BeadLineageRecord]) -> HashMap<String, String> {
    let mut map: HashMap<String, String> = HashMap::new();
    for r in lineage {
        map.entry(r.bead_id.clone())
            .and_modify(|cur| {
                if r.created_at < *cur {
                    *cur = r.created_at.clone();
                }
            })
            .or_insert_with(|| r.created_at.clone());
    }
    map
}

/// Build the change_event view by aggregating lineage rows per change bead and
/// joining `bug_causality` (where this bead is the culprit) for the outcome
/// labels. Pure over its inputs so it is unit-testable without a DB. Output is
/// ordered most-recent change first (then bead id) for stable, useful display.
pub fn build_change_events(
    lineage: &[BeadLineageRecord],
    causality: &[BugCausalityRecord],
    earliest: &HashMap<String, String>,
) -> Vec<ChangeEvent> {
    // culprit bead → the distinct bugs blamed on it.
    let mut blamed: HashMap<&str, BTreeSet<&str>> = HashMap::new();
    for c in causality {
        if let Some(culprit) = c.culprit_bead_id.as_deref() {
            if !culprit.is_empty() {
                blamed.entry(culprit).or_default().insert(c.bug_id.as_str());
            }
        }
    }

    // Aggregate lineage rows per change bead, preserving first-seen order of the
    // beads themselves so equal-timestamp ties stay deterministic.
    let mut order: Vec<String> = Vec::new();
    let mut groups: HashMap<String, Vec<&BeadLineageRecord>> = HashMap::new();
    for r in lineage {
        groups
            .entry(r.bead_id.clone())
            .or_insert_with(|| {
                order.push(r.bead_id.clone());
                Vec::new()
            })
            .push(r);
    }

    let mut events: Vec<ChangeEvent> = Vec::new();
    for bead_id in &order {
        let rows = &groups[bead_id];

        let feature_id = rows.iter().find_map(|r| r.feature_id.clone());

        let mut commit_shas: Vec<String> = Vec::new();
        let mut touched_files: Vec<String> = Vec::new();
        let mut bundle_slugs: Vec<String> = Vec::new();
        let mut lines_added = 0i64;
        let mut lines_deleted = 0i64;
        for r in rows {
            if let Some(sha) = r.commit_sha.as_deref() {
                if !sha.is_empty() && !commit_shas.iter().any(|s| s == sha) {
                    commit_shas.push(sha.to_string());
                }
            }
            for f in &r.touched_files {
                if !touched_files.iter().any(|x| x == f) {
                    touched_files.push(f.clone());
                }
            }
            for slug in r
                .bundle_slugs
                .as_deref()
                .unwrap_or("")
                .split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
            {
                if !bundle_slugs.iter().any(|x| x == slug) {
                    bundle_slugs.push(slug.to_string());
                }
            }
            lines_added += r.lines_added.unwrap_or(0);
            lines_deleted += r.lines_deleted.unwrap_or(0);
        }

        let changed_at = rows
            .iter()
            .map(|r| r.created_at.as_str())
            .min()
            .unwrap_or("")
            .to_string();

        let bug_ids: Vec<String> = blamed
            .get(bead_id.as_str())
            .map(|set| set.iter().map(|s| s.to_string()).collect())
            .unwrap_or_default();
        let introduced_bug = !bug_ids.is_empty();

        // ttf_days: smallest positive gap from this change to a blamed bug's
        // earliest lineage timestamp. Bugs whose timing is unknown or precedes
        // the change (data noise) are ignored.
        let ttf_days = if introduced_bug {
            bug_ids
                .iter()
                .filter_map(|bug| earliest.get(bug))
                .filter_map(|bug_at| days_between(&changed_at, bug_at))
                .filter(|d| *d >= 0.0)
                .fold(None, |acc: Option<f64>, d| {
                    Some(acc.map_or(d, |m| m.min(d)))
                })
        } else {
            None
        };

        events.push(ChangeEvent {
            change_bead_id: bead_id.clone(),
            feature_id,
            commit_shas,
            touched_files,
            bundle_slugs,
            bug_ids,
            lines_added,
            lines_deleted,
            introduced_bug,
            ttf_days,
            changed_at,
        });
    }

    // Most-recent change first; bead id breaks ties for determinism.
    events.sort_by(|a, b| {
        b.changed_at
            .cmp(&a.changed_at)
            .then_with(|| a.change_bead_id.cmp(&b.change_bead_id))
    });
    events
}

/// Whole-day-fractional gap between two ISO-8601 timestamps (`b - a`) in days,
/// or None if either fails to parse. Accepts the `...Z` form bobbin writes.
fn days_between(a: &str, b: &str) -> Option<f64> {
    let ta = parse_ts(a)?;
    let tb = parse_ts(b)?;
    Some((tb - ta).num_seconds() as f64 / 86_400.0)
}

fn parse_ts(s: &str) -> Option<chrono::DateTime<chrono::Utc>> {
    chrono::DateTime::parse_from_rfc3339(s)
        .ok()
        .map(|dt| dt.with_timezone(&chrono::Utc))
}

fn print_table(events: &[ChangeEvent]) {
    if events.is_empty() {
        println!("{}", "No change events to reconcile yet.".dimmed());
        return;
    }
    for e in events {
        let label = if e.introduced_bug {
            format!("⚠ bug ({})", e.bug_ids.join(",")).red().to_string()
        } else {
            "clean".green().to_string()
        };
        let ttf = e
            .ttf_days
            .map(|d| format!("ttf={:.1}d", d))
            .unwrap_or_default();
        println!(
            "{}  {}  {} commit(s)  {} file(s)  +{}/-{}  {} {}",
            e.changed_at.dimmed(),
            e.change_bead_id.cyan(),
            e.commit_shas.len(),
            e.touched_files.len(),
            e.lines_added,
            e.lines_deleted,
            label,
            ttf.dimmed(),
        );
        if let Some(feat) = &e.feature_id {
            println!("    {} {}", "feature:".dimmed(), feat.magenta());
        }
        if !e.bundle_slugs.is_empty() {
            println!("    {} {}", "bundles:".dimmed(), e.bundle_slugs.join(", "));
        }
    }
    let bugs = events.iter().filter(|e| e.introduced_bug).count();
    println!(
        "{} {} change(s), {} introduced a bug",
        "•".dimmed(),
        events.len(),
        bugs
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::sqlite::TouchedSymbol;

    fn lineage(
        bead: &str,
        sha: &str,
        files: &[&str],
        bundles: Option<&str>,
        at: &str,
    ) -> BeadLineageRecord {
        BeadLineageRecord {
            id: 0,
            created_at: at.to_string(),
            bead_id: bead.to_string(),
            bead_type: None,
            commit_sha: Some(sha.to_string()),
            bundle_slugs: bundles.map(|s| s.to_string()),
            touched_files: files.iter().map(|s| s.to_string()).collect(),
            action_type: Some("commit".to_string()),
            feature_id: None,
            lines_added: Some(10),
            lines_deleted: Some(2),
            touched_symbols: Vec::<TouchedSymbol>::new(),
        }
    }

    fn causality(bug: &str, culprit_bead: &str, file: &str) -> BugCausalityRecord {
        BugCausalityRecord {
            id: 0,
            created_at: "2026-06-20T00:00:00Z".to_string(),
            bug_id: bug.to_string(),
            culprit_sha: Some("sha".to_string()),
            culprit_bead_id: Some(culprit_bead.to_string()),
            file: Some(file.to_string()),
            confidence: Some(0.9),
        }
    }

    #[test]
    fn test_aggregates_rows_per_bead() {
        // Two lineage rows for the same change bead (a `link` + a `commit`) must
        // collapse into one event with unioned files/commits and summed lines.
        let rows = vec![
            lineage("bo-a", "sha1", &["src/a.rs"], Some("search"), "2026-06-10T00:00:00Z"),
            lineage("bo-a", "sha2", &["src/b.rs", "src/a.rs"], Some("search,rag"), "2026-06-10T01:00:00Z"),
        ];
        let events = build_change_events(&rows, &[], &earliest_by_bead(&rows));
        assert_eq!(events.len(), 1);
        let e = &events[0];
        assert_eq!(e.commit_shas, vec!["sha1", "sha2"]);
        assert_eq!(e.touched_files, vec!["src/a.rs", "src/b.rs"]);
        assert_eq!(e.bundle_slugs, vec!["search", "rag"]);
        assert_eq!(e.lines_added, 20);
        assert_eq!(e.lines_deleted, 4);
        assert!(!e.introduced_bug);
        assert_eq!(e.ttf_days, None);
    }

    #[test]
    fn test_introduced_bug_and_ttf() {
        // Change bo-c lands; 5 days later a bug bo-bug is blamed on it.
        let rows = vec![
            lineage("bo-c", "shaC", &["src/x.rs"], None, "2026-06-01T00:00:00Z"),
            lineage("bo-bug", "shaB", &["src/x.rs"], None, "2026-06-06T00:00:00Z"),
        ];
        let cz = vec![causality("bo-bug", "bo-c", "src/x.rs")];
        let events = build_change_events(&rows, &cz, &earliest_by_bead(&rows));
        let c = events.iter().find(|e| e.change_bead_id == "bo-c").unwrap();
        assert!(c.introduced_bug);
        assert_eq!(c.bug_ids, vec!["bo-bug"]);
        assert!((c.ttf_days.unwrap() - 5.0).abs() < 1e-9);
        // The bug bead itself is also a change event, with no bug blamed on it.
        let b = events.iter().find(|e| e.change_bead_id == "bo-bug").unwrap();
        assert!(!b.introduced_bug);
    }

    #[test]
    fn test_ordering_recent_first() {
        let rows = vec![
            lineage("bo-old", "s1", &["a"], None, "2026-06-01T00:00:00Z"),
            lineage("bo-new", "s2", &["b"], None, "2026-06-09T00:00:00Z"),
        ];
        let events = build_change_events(&rows, &[], &earliest_by_bead(&rows));
        assert_eq!(events[0].change_bead_id, "bo-new");
        assert_eq!(events[1].change_bead_id, "bo-old");
    }

    #[test]
    fn test_multiple_bugs_takes_earliest_ttf() {
        let rows = vec![
            lineage("bo-c", "shaC", &["src/x.rs"], None, "2026-06-01T00:00:00Z"),
            lineage("bo-bug1", "b1", &["src/x.rs"], None, "2026-06-11T00:00:00Z"),
            lineage("bo-bug2", "b2", &["src/x.rs"], None, "2026-06-04T00:00:00Z"),
        ];
        let cz = vec![
            causality("bo-bug1", "bo-c", "src/x.rs"),
            causality("bo-bug2", "bo-c", "src/x.rs"),
        ];
        let events = build_change_events(&rows, &cz, &earliest_by_bead(&rows));
        let c = events.iter().find(|e| e.change_bead_id == "bo-c").unwrap();
        assert_eq!(c.bug_ids, vec!["bo-bug1", "bo-bug2"]);
        // Earliest bug (bo-bug2 at +3d) wins over bo-bug1 (+10d).
        assert!((c.ttf_days.unwrap() - 3.0).abs() < 1e-9);
    }
}
