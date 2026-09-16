//! Dolt (MySQL-protocol) bead backend.
//!
//! The original store. Split out of `beads.rs` when the br JSONL backend
//! landed (aegis-205llh) so that the two backends sit beside each other and
//! neither owns the shared model: chunk identity, content assembly and label
//! exclusion all live in the parent module and are called from here, so a
//! source switch cannot silently re-key the corpus.
//!
//! This module is expected to be DELETED when the Dolt server is retired
//! (aegis-h7xuql deliverable 3). Nothing else should grow into it.

use anyhow::{Context, Result};
use mysql_async::prelude::*;
use std::collections::HashMap;

use super::keys::{issues_where_clause, rig_of};
use super::{rows_to_chunks, BeadRow, CommentRow, LiveBeadMetadata};
use crate::config::BeadsConfig;
use crate::types::Chunk;

/// Fetch beads from a single Dolt database, optionally narrowed to one id.
pub(super) async fn fetch_from_database(
    config: &BeadsConfig,
    db_name: &str,
    only_id: Option<&str>,
) -> Result<Vec<Chunk>> {
    // Extract rig name from db_name (e.g., "beads_aegis" -> "aegis")
    let rig = rig_of(db_name);

    let url = format!(
        "mysql://{}@{}:{}/{}",
        config.user, config.host, config.port, db_name
    );
    let pool = mysql_async::Pool::new(url.as_str());
    let mut conn = pool.get_conn().await.with_context(|| {
        format!(
            "Failed to connect to Dolt at {}:{}",
            config.host, config.port
        )
    })?;

    let where_clause = issues_where_clause(config, only_id);

    // Fetch issues. `metadata` is a JSON column — CAST to text so the driver
    // returns a String; COALESCE guards against NULL.
    let issues_query = format!(
        "SELECT id, title, description, status, priority, assignee, notes, \
         CAST(COALESCE(metadata, JSON_OBJECT()) AS CHAR) FROM issues{}",
        where_clause
    );
    let issues: Vec<BeadRow> = conn
        .query_map(
            &issues_query,
            |(id, title, description, status, priority, assignee, notes, metadata): (
                String,
                String,
                String,
                String,
                i32,
                Option<String>,
                String,
                Option<String>,
            )| {
                BeadRow {
                    id,
                    title,
                    description,
                    status,
                    priority,
                    assignee,
                    notes,
                    metadata: metadata.unwrap_or_default(),
                }
            },
        )
        .await
        .with_context(|| "Failed to query issues")?;

    // Fetch comments if enabled
    let comments: Vec<CommentRow> = if config.include_comments {
        let issue_ids: Vec<&str> = issues.iter().map(|i| i.id.as_str()).collect();
        if issue_ids.is_empty() {
            vec![]
        } else {
            // Build IN clause
            let placeholders: Vec<String> = issue_ids
                .iter()
                .map(|id| format!("'{}'", id.replace('\'', "''")))
                .collect();
            let comments_query = format!(
                "SELECT issue_id, author, text FROM comments WHERE issue_id IN ({}) ORDER BY created_at ASC",
                placeholders.join(", ")
            );
            conn.query_map(
                &comments_query,
                |(issue_id, author, text): (String, String, String)| CommentRow {
                    issue_id,
                    author,
                    text,
                },
            )
            .await
            .with_context(|| "Failed to query comments")?
        }
    } else {
        vec![]
    };

    // Fetch labels (e.g. `b:<slug>`, `pitch`) so they're searchable. The labels
    // table is part of the same Dolt schema used by live enrichment below.
    let mut labels_by_issue: HashMap<String, Vec<String>> = HashMap::new();
    {
        let issue_ids: Vec<&str> = issues.iter().map(|i| i.id.as_str()).collect();
        if !issue_ids.is_empty() {
            let placeholders: Vec<String> = issue_ids
                .iter()
                .map(|id| format!("'{}'", id.replace('\'', "''")))
                .collect();
            let labels_query = format!(
                "SELECT issue_id, label FROM labels WHERE issue_id IN ({})",
                placeholders.join(", ")
            );
            // Best-effort: a missing labels table should not fail bead indexing.
            let label_rows: Vec<(String, String)> =
                conn.query(&labels_query).await.unwrap_or_default();
            for (issue_id, label) in label_rows {
                labels_by_issue.entry(issue_id).or_default().push(label);
            }
        }
    }

    // Rows -> chunks through the SHARED constructor. Both backends call this
    // one function so chunk identity and label exclusion cannot drift apart.
    let chunks = rows_to_chunks(
        rig,
        issues,
        &comments,
        &labels_by_issue,
        &config.exclude_labels,
    );

    drop(conn);
    pool.disconnect().await?;

    Ok(chunks)
}

/// Fetch live metadata for specific beads from Dolt.
///
/// Takes a list of (rig, bead_id) pairs and returns a map from bead_id to metadata.
/// Used by search_beads MCP tool to enrich results with current status/priority.
pub(super) async fn fetch_bead_metadata(
    config: &BeadsConfig,
    bead_ids: &[(String, String)], // (rig, bead_id)
) -> Result<HashMap<String, LiveBeadMetadata>> {
    if !config.enabled || bead_ids.is_empty() {
        return Ok(HashMap::new());
    }

    let mut result = HashMap::new();

    // Group bead_ids by rig -> database
    let mut by_db: HashMap<String, Vec<&str>> = HashMap::new();
    for (rig, bead_id) in bead_ids {
        let db_name = format!("beads_{}", rig);
        if config.databases.contains(&db_name) {
            by_db.entry(db_name).or_default().push(bead_id.as_str());
        }
    }

    for (db_name, ids) in &by_db {
        let url = format!(
            "mysql://{}@{}:{}/{}",
            config.user, config.host, config.port, db_name
        );
        let pool = mysql_async::Pool::new(url.as_str());
        let mut conn = pool
            .get_conn()
            .await
            .with_context(|| format!("connect to {db_name} for live bead enrichment"))?;

        let placeholders: Vec<String> = ids
            .iter()
            .map(|id| format!("'{}'", id.replace('\'', "''")))
            .collect();
        let in_clause = placeholders.join(", ");

        let query = format!(
            "SELECT id, title, status, priority, assignee, COALESCE(issue_type, 'task'), COALESCE(owner, ''), COALESCE(DATE_FORMAT(created_at, '%Y-%m-%d'), '') FROM issues WHERE id IN ({})",
            in_clause
        );

        let rows: Vec<(
            String,
            String,
            String,
            i32,
            Option<String>,
            String,
            String,
            String,
        )> = conn
            .query(&query)
            .await
            .with_context(|| format!("query live bead metadata from {db_name}"))?;

        // Fetch labels for these beads
        let labels_query = format!(
            "SELECT issue_id, label FROM labels WHERE issue_id IN ({})",
            in_clause
        );
        let label_rows: Vec<(String, String)> = conn
            .query(&labels_query)
            .await
            .with_context(|| format!("query live bead labels from {db_name}"))?;

        let mut labels_by_id: HashMap<String, Vec<String>> = HashMap::new();
        for (issue_id, label) in label_rows {
            labels_by_id.entry(issue_id).or_default().push(label);
        }

        for (id, title, status, priority, assignee, issue_type, owner, created_at) in rows {
            let labels = labels_by_id.remove(&id).unwrap_or_default();
            result.insert(
                id,
                LiveBeadMetadata {
                    status,
                    priority,
                    assignee,
                    title,
                    issue_type,
                    owner,
                    labels,
                    created_at: if created_at.is_empty() {
                        None
                    } else {
                        Some(created_at)
                    },
                },
            );
        }

        drop(conn);
        pool.disconnect()
            .await
            .with_context(|| format!("disconnect live bead enrichment pool for {db_name}"))?;
    }

    Ok(result)
}
