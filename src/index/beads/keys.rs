//! Bead chunk identity and row visibility — the two things the batch sweep and
//! the single-bead fast path MUST agree on.
//!
//! Split out of `beads.rs` (which is at its 500-line ceiling, and whose own
//! header explains why an allowlist entry is not the answer). The split is not
//! only for size: these are the pure functions, testable with no Dolt
//! connection, and they are the contract surface between
//! `bobbin index --include-beads` and `bobbin index-bead <id>`. Two spellings
//! of a chunk key, or two sets of visibility rules, would give the two paths
//! separate corpora that each silently delete the other's rows.

use crate::config::BeadsConfig;

/// Rig name for a configured database (`beads_aegis` → `aegis`).
pub fn rig_of(db_name: &str) -> &str {
    db_name.strip_prefix("beads_").unwrap_or(db_name)
}

/// The `file_path` key a bead's chunk is stored under.
///
/// This IS the identity the incremental machinery hashes against, so the batch
/// sweep and the single-bead path must agree on it exactly — they share one
/// function for that reason.
pub fn bead_file_path(rig: &str, bead_id: &str) -> String {
    format!("beads:{rig}:{bead_id}")
}

/// Every key one bead id could occupy across the configured databases.
///
/// A bead id is unique within its rig, not across rigs, and the caller
/// generally does not know which rig it belongs to. `rig_filter` narrows to a
/// single rig when the operator names one.
pub fn bead_file_paths(
    config: &BeadsConfig,
    bead_id: &str,
    rig_filter: Option<&str>,
) -> Vec<String> {
    config
        .databases
        .iter()
        .map(|db| rig_of(db))
        .filter(|rig| rig_filter.is_none_or(|want| want == *rig))
        .map(|rig| bead_file_path(rig, bead_id))
        .collect()
}

/// Escape a value for a single-quoted SQL literal.
fn sql_literal(value: &str) -> String {
    value.replace('\'', "''")
}

/// The `WHERE` clause the issues query runs under, as a pure function so the
/// visibility rules can be asserted without a live Dolt connection.
///
/// `only_id` narrows to one bead **without relaxing anything else**. That is
/// the load-bearing property of the single-bead path: a bead that the batch
/// sweep would not index (closed, aged out, deleted) must not become indexable
/// just because someone asked for it by name, or `index-bead` would quietly
/// re-admit exactly the rows `--include-beads` filters out.
pub(crate) fn issues_where_clause(config: &BeadsConfig, only_id: Option<&str>) -> String {
    let mut conditions = Vec::new();
    if !config.include_closed {
        conditions.push("status NOT IN ('closed', 'deleted')".to_string());
    } else {
        // When including closed beads, still exclude deleted ones
        conditions.push("status != 'deleted'".to_string());
    }
    if config.max_age_days > 0 {
        // Age bounds CLOSED beads only — an OPEN bead is active work and must be
        // indexed regardless of age. Previously this applied to all beads, so
        // rigs whose open beads are all older than max_age_days (e.g.
        // beads_goldblum: 632 open, none <90d) indexed ZERO beads.
        conditions.push(format!(
            "(status NOT IN ('closed', 'deleted') OR created_at >= DATE_SUB(NOW(), INTERVAL {} DAY))",
            config.max_age_days
        ));
    }
    if let Some(id) = only_id {
        conditions.push(format!("id = '{}'", sql_literal(id)));
    }

    if conditions.is_empty() {
        String::new()
    } else {
        format!(" WHERE {}", conditions.join(" AND "))
    }
}
