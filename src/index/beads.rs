use anyhow::{Context, Result};
use mysql_async::prelude::*;
use sha2::{Digest, Sha256};
use std::collections::HashMap;

use crate::config::BeadsConfig;
use crate::types::{Chunk, ChunkType};

/// Live metadata for a bead, fetched from Dolt
#[derive(Debug, Clone)]
pub struct LiveBeadMetadata {
    pub status: String,
    pub priority: i32,
    pub assignee: Option<String>,
    pub title: String,
    pub issue_type: String,
    pub owner: String,
    pub labels: Vec<String>,
    pub created_at: Option<String>,
}

/// A single bead (issue) fetched from Dolt
#[derive(Debug)]
struct BeadRow {
    id: String,
    title: String,
    description: String,
    status: String,
    priority: i32,
    assignee: Option<String>,
    notes: String,
    /// Raw `metadata` JSON column (may be empty/`{}`).
    metadata: String,
}

/// Stable content hash for a bead chunk, used for incremental indexing
/// (skip re-embedding beads whose assembled content is unchanged).
pub fn content_hash(content: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(content.as_bytes());
    hex::encode(hasher.finalize())
}

/// True if a bead `metadata` JSON string carries meaningful content worth
/// indexing (i.e. not empty, `{}`, or `null`).
fn metadata_is_meaningful(metadata: &str) -> bool {
    let t = metadata.trim();
    !(t.is_empty() || t == "{}" || t == "null")
}

/// True if a bead carries any excluded label (case-insensitive). Used to keep
/// sensitive beads (e.g. `security`, `escalation`) out of the index entirely.
fn bead_excluded(labels: &[String], exclude_labels: &[String]) -> bool {
    if exclude_labels.is_empty() {
        return false;
    }
    labels.iter().any(|l| {
        exclude_labels
            .iter()
            .any(|x| x.eq_ignore_ascii_case(l))
    })
}

/// Assemble the embeddable text for a bead from its fields, comments, and labels.
///
/// Kept as a pure function so the content layout can be unit-tested without a
/// live Dolt connection.
fn build_bead_content(issue: &BeadRow, comments: &[&CommentRow], labels: &[String]) -> String {
    let mut content = format!("{}\n\n{}", issue.title, issue.description);

    if !issue.notes.is_empty() {
        content.push_str("\n\nNotes:\n");
        content.push_str(&issue.notes);
    }

    // Labels (e.g. `b:<slug>`, `pitch`) — valuable for semantic + filtered search.
    if !labels.is_empty() {
        content.push_str("\n\nLabels: ");
        content.push_str(&labels.join(", "));
    }

    // Structured metadata JSON (guidance/lineage records, source refs, etc.).
    if metadata_is_meaningful(&issue.metadata) {
        content.push_str("\n\nMetadata:\n");
        content.push_str(issue.metadata.trim());
    }

    // Append metadata
    content.push_str(&format!(
        "\n\nStatus: {} | Priority: P{} | Assignee: {}",
        issue.status,
        issue.priority,
        issue.assignee.as_deref().unwrap_or("unassigned")
    ));

    // Append comments
    if !comments.is_empty() {
        content.push_str("\n\nComments:");
        for c in comments {
            content.push_str(&format!("\n--- {} ---\n{}", c.author, c.text));
        }
    }

    content
}

/// A comment on a bead
#[derive(Debug)]
struct CommentRow {
    issue_id: String,
    author: String,
    text: String,
}

/// Fetch beads from all configured Dolt databases and convert to Chunks.
pub async fn fetch_beads(config: &BeadsConfig) -> Result<Vec<Chunk>> {
    if !config.enabled || config.databases.is_empty() {
        return Ok(vec![]);
    }

    let mut all_chunks = Vec::new();

    for db_name in &config.databases {
        let chunks = fetch_from_database(config, db_name).await
            .with_context(|| format!("Failed to fetch beads from {}", db_name))?;
        all_chunks.extend(chunks);
    }

    Ok(all_chunks)
}

/// Fetch beads from a single Dolt database
async fn fetch_from_database(config: &BeadsConfig, db_name: &str) -> Result<Vec<Chunk>> {
    // Extract rig name from db_name (e.g., "beads_aegis" -> "aegis")
    let rig = db_name.strip_prefix("beads_").unwrap_or(db_name);

    let url = format!(
        "mysql://{}@{}:{}/{}",
        config.user, config.host, config.port, db_name
    );
    let pool = mysql_async::Pool::new(url.as_str());
    let mut conn = pool.get_conn().await
        .with_context(|| format!("Failed to connect to Dolt at {}:{}", config.host, config.port))?;

    // Build WHERE clause
    let mut conditions = Vec::new();
    if !config.include_closed {
        conditions.push("status NOT IN ('closed', 'deleted')".to_string());
    } else {
        // When including closed beads, still exclude deleted ones
        conditions.push("status != 'deleted'".to_string());
    }
    if config.max_age_days > 0 {
        conditions.push(format!(
            "created_at >= DATE_SUB(NOW(), INTERVAL {} DAY)",
            config.max_age_days
        ));
    }

    let where_clause = if conditions.is_empty() {
        String::new()
    } else {
        format!(" WHERE {}", conditions.join(" AND "))
    };

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
                String, String, String, String, i32, Option<String>, String, Option<String>,
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
            let placeholders: Vec<String> = issue_ids.iter().map(|id| format!("'{}'", id.replace('\'', "''"))).collect();
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

    // Group comments by issue_id
    let mut comments_by_issue: std::collections::HashMap<String, Vec<&CommentRow>> =
        std::collections::HashMap::new();
    for comment in &comments {
        comments_by_issue
            .entry(comment.issue_id.clone())
            .or_default()
            .push(comment);
    }

    // Convert to Chunks, skipping beads with excluded labels (e.g. security).
    let chunks: Vec<Chunk> = issues
        .into_iter()
        .filter_map(|issue| {
            let labels = labels_by_issue.get(&issue.id).cloned().unwrap_or_default();
            if bead_excluded(&labels, &config.exclude_labels) {
                return None;
            }
            let issue_comments: Vec<&CommentRow> = comments_by_issue
                .get(&issue.id)
                .cloned()
                .unwrap_or_default();

            let content = build_bead_content(&issue, &issue_comments, &labels);

            // Generate deterministic ID
            let id_input = format!("beads:{}:{}", rig, issue.id);
            let mut hasher = Sha256::new();
            hasher.update(id_input.as_bytes());
            let id = hex::encode(hasher.finalize());

            Some(Chunk {
                id,
                file_path: format!("beads:{}:{}", rig, issue.id),
                chunk_type: ChunkType::Issue,
                name: Some(issue.title),
                start_line: 0,
                end_line: 0,
                content,
                language: "beads".to_string(),
                tags: labels.join(","),
            })
        })
        .collect();

    drop(conn);
    pool.disconnect().await?;

    Ok(chunks)
}

/// Fetch live metadata for specific beads from Dolt.
///
/// Takes a list of (rig, bead_id) pairs and returns a map from bead_id to metadata.
/// Used by search_beads MCP tool to enrich results with current status/priority.
pub async fn fetch_bead_metadata(
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
        let mut conn = match pool.get_conn().await {
            Ok(c) => c,
            Err(e) => {
                eprintln!("Warning: failed to connect to Dolt for live enrichment: {}", e);
                continue;
            }
        };

        let placeholders: Vec<String> = ids
            .iter()
            .map(|id| format!("'{}'", id.replace('\'', "''")))
            .collect();
        let in_clause = placeholders.join(", ");

        let query = format!(
            "SELECT id, title, status, priority, assignee, COALESCE(issue_type, 'task'), COALESCE(owner, ''), COALESCE(DATE_FORMAT(created_at, '%Y-%m-%d'), '') FROM issues WHERE id IN ({})",
            in_clause
        );

        let rows: Vec<(String, String, String, i32, Option<String>, String, String, String)> = conn
            .query(&query)
            .await
            .unwrap_or_default();

        // Fetch labels for these beads
        let labels_query = format!(
            "SELECT issue_id, label FROM labels WHERE issue_id IN ({})",
            in_clause
        );
        let label_rows: Vec<(String, String)> = conn
            .query(&labels_query)
            .await
            .unwrap_or_default();

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
                    created_at: if created_at.is_empty() { None } else { Some(created_at) },
                },
            );
        }

        drop(conn);
        let _ = pool.disconnect().await;
    }

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_chunk_id_deterministic() {
        let input = "beads:aegis:aegis-0a9";
        let mut hasher = Sha256::new();
        hasher.update(input.as_bytes());
        let id1 = hex::encode(hasher.finalize());

        let mut hasher = Sha256::new();
        hasher.update(input.as_bytes());
        let id2 = hex::encode(hasher.finalize());

        assert_eq!(id1, id2);
        assert_eq!(id1.len(), 64);
    }

    fn sample_issue() -> BeadRow {
        BeadRow {
            id: "aegis-0a9".to_string(),
            title: "Fix the widget".to_string(),
            description: "The widget is broken.".to_string(),
            status: "open".to_string(),
            priority: 1,
            assignee: Some("strider".to_string()),
            notes: "investigated already".to_string(),
            metadata: String::new(),
        }
    }

    #[test]
    fn test_metadata_is_meaningful() {
        assert!(!metadata_is_meaningful(""));
        assert!(!metadata_is_meaningful("  {}  "));
        assert!(!metadata_is_meaningful("null"));
        assert!(metadata_is_meaningful(r#"{"source":"T1"}"#));
    }

    #[test]
    fn test_build_bead_content_includes_metadata() {
        let mut issue = sample_issue();
        issue.metadata = r#"{"source":"guidance-T1","ref":"doc#42"}"#.to_string();
        let content = build_bead_content(&issue, &[], &[]);
        assert!(content.contains("Metadata:"));
        assert!(content.contains("guidance-T1"));
    }

    #[test]
    fn test_build_bead_content_skips_empty_metadata() {
        let issue = sample_issue(); // metadata = ""
        let content = build_bead_content(&issue, &[], &[]);
        assert!(!content.contains("Metadata:"));
    }

    #[test]
    fn test_bead_excluded_by_label() {
        let exclude = vec!["security".to_string(), "escalation".to_string()];
        assert!(bead_excluded(&["security".to_string()], &exclude));
        assert!(bead_excluded(&["Escalation".to_string()], &exclude)); // case-insensitive
        assert!(bead_excluded(&["pitch".to_string(), "security".to_string()], &exclude));
        assert!(!bead_excluded(&["pitch".to_string()], &exclude));
        assert!(!bead_excluded(&["security".to_string()], &[])); // empty = no exclusion
    }

    #[test]
    fn test_content_hash_stable_and_sensitive() {
        let a = content_hash("hello");
        assert_eq!(a, content_hash("hello"));
        assert_ne!(a, content_hash("hello!"));
        assert_eq!(a.len(), 64);
    }

    #[test]
    fn test_build_bead_content_includes_labels() {
        let issue = sample_issue();
        let labels = vec!["pitch".to_string(), "b:search".to_string()];
        let content = build_bead_content(&issue, &[], &labels);
        assert!(content.contains("Fix the widget"));
        assert!(content.contains("The widget is broken."));
        assert!(content.contains("Notes:\ninvestigated already"));
        assert!(content.contains("Labels: pitch, b:search"));
        assert!(content.contains("Status: open | Priority: P1 | Assignee: strider"));
    }

    #[test]
    fn test_build_bead_content_no_labels_no_comments() {
        let issue = sample_issue();
        let content = build_bead_content(&issue, &[], &[]);
        assert!(!content.contains("Labels:"));
        assert!(!content.contains("Comments:"));
        assert!(content.contains("Assignee: strider"));
    }

    #[test]
    fn test_build_bead_content_includes_comments() {
        let issue = sample_issue();
        let c = CommentRow {
            issue_id: "aegis-0a9".to_string(),
            author: "ian".to_string(),
            text: "looks good".to_string(),
        };
        let content = build_bead_content(&issue, &[&c], &[]);
        assert!(content.contains("Comments:"));
        assert!(content.contains("--- ian ---\nlooks good"));
    }

    #[test]
    fn test_disabled_config_returns_empty() {
        let config = BeadsConfig::default();
        let rt = tokio::runtime::Runtime::new().unwrap();
        let chunks = rt.block_on(fetch_beads(&config)).unwrap();
        assert!(chunks.is_empty());
    }

    #[test]
    fn test_empty_databases_returns_empty() {
        let config = BeadsConfig {
            enabled: true,
            databases: vec![],
            ..Default::default()
        };
        let rt = tokio::runtime::Runtime::new().unwrap();
        let chunks = rt.block_on(fetch_beads(&config)).unwrap();
        assert!(chunks.is_empty());
    }
}
