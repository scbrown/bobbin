//! `/history` — git commit history for one file.
//!
//! The HTTP half of the MCP `file_history` tool (#55). Both call
//! `GitAnalyzer::get_file_history` and derive the same author breakdown and
//! churn rate, so the two surfaces cannot disagree about a file's history.
#![allow(private_interfaces)]

use std::sync::Arc;

use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::Json;
use serde::{Deserialize, Serialize};

use crate::index::GitAnalyzer;

use super::super::{bad_request, internal_error, AppState, ErrorBody};

#[derive(Deserialize)]
pub(crate) struct HistoryParams {
    /// File path to show history for
    file: String,
    /// Max commits (default 20)
    limit: Option<usize>,
    /// Role for access filtering
    role: Option<String>,
}

#[derive(Serialize)]
pub(crate) struct HistoryResponse {
    file: String,
    count: usize,
    entries: Vec<HistoryItem>,
    stats: HistoryStats,
}

#[derive(Serialize)]
pub(crate) struct HistoryItem {
    date: String,
    author: String,
    message: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    issues: Vec<String>,
}

#[derive(Serialize)]
pub(crate) struct HistoryStats {
    total_commits: usize,
    authors: Vec<HistoryAuthor>,
    /// Commits per 30 days across the returned window. 0.0 when fewer than two
    /// commits were returned — one commit gives no interval to divide by, and
    /// a made-up rate would read as a measurement.
    churn_rate: f32,
}

#[derive(Serialize)]
pub(crate) struct HistoryAuthor {
    name: String,
    commits: usize,
}

pub(crate) async fn history(
    State(state): State<Arc<AppState>>,
    Query(params): Query<HistoryParams>,
) -> Result<Json<HistoryResponse>, (StatusCode, Json<ErrorBody>)> {
    let limit = params.limit.unwrap_or(20);

    let access = super::super::resolve_filter(&state, params.role.as_deref());
    if !access.is_path_allowed(&params.file) {
        return Err(bad_request(format!("File not found: {}", params.file)));
    }

    let git = GitAnalyzer::new(&state.repo_root).map_err(internal_error)?;
    let entries = git
        .get_file_history(&params.file, limit)
        .map_err(internal_error)?;

    let mut author_counts: std::collections::HashMap<String, usize> =
        std::collections::HashMap::new();
    for entry in &entries {
        *author_counts.entry(entry.author.clone()).or_insert(0) += 1;
    }
    let mut authors: Vec<HistoryAuthor> = author_counts
        .into_iter()
        .map(|(name, commits)| HistoryAuthor { name, commits })
        .collect();
    // Most prolific first; name breaks ties so the ordering is stable across
    // requests rather than following HashMap iteration order.
    authors.sort_by(|a, b| b.commits.cmp(&a.commits).then_with(|| a.name.cmp(&b.name)));

    let churn_rate = churn_rate_per_30d(&entries);

    Ok(Json(HistoryResponse {
        file: params.file,
        count: entries.len(),
        stats: HistoryStats {
            total_commits: entries.len(),
            authors,
            churn_rate,
        },
        entries: entries
            .into_iter()
            .map(|h| HistoryItem {
                date: h.date,
                author: h.author,
                message: h.message,
                issues: h.issues,
            })
            .collect(),
    }))
}

/// Commits per 30 days over the span the returned entries cover.
///
/// `git log` returns newest first, so the span runs from the last entry to the
/// first. The span is floored at one day: a file touched five times in one
/// afternoon otherwise divides by ~0 and reports an absurd rate.
fn churn_rate_per_30d(entries: &[crate::index::git::FileHistoryEntry]) -> f32 {
    if entries.len() < 2 {
        return 0.0;
    }
    let first_ts = entries.last().map(|e| e.timestamp).unwrap_or(0);
    let last_ts = entries.first().map(|e| e.timestamp).unwrap_or(0);
    let days = ((last_ts - first_ts) as f32 / 86_400.0).max(1.0);
    (entries.len() as f32 / days) * 30.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::index::git::FileHistoryEntry;

    fn entry(timestamp: i64) -> FileHistoryEntry {
        FileHistoryEntry {
            date: String::new(),
            author: "a".to_string(),
            message: String::new(),
            issues: vec![],
            timestamp,
        }
    }

    #[test]
    fn churn_needs_two_commits_to_have_a_rate() {
        assert_eq!(churn_rate_per_30d(&[]), 0.0);
        assert_eq!(churn_rate_per_30d(&[entry(1_000)]), 0.0);
    }

    #[test]
    fn churn_is_commits_per_thirty_days_newest_first() {
        // 3 commits spanning 30 days => 3 per 30 days.
        let day = 86_400;
        let rate = churn_rate_per_30d(&[entry(30 * day), entry(15 * day), entry(0)]);
        assert!((rate - 3.0).abs() < 1e-3, "got {rate}");
    }

    #[test]
    fn same_day_burst_does_not_divide_by_zero() {
        let rate = churn_rate_per_30d(&[entry(100), entry(50), entry(0)]);
        assert!(rate.is_finite(), "got {rate}");
        // Floored at a one-day span: 3 commits => 90 per 30 days, not infinity.
        assert!((rate - 90.0).abs() < 1e-3, "got {rate}");
    }
}
