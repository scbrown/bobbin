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
        conditions.push("deleted_at IS NULL".to_string());
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

    // Fetch issues
    let issues_query = format!(
        "SELECT id, title, description, status, priority, assignee, notes FROM issues{}",
        where_clause
    );
    let issues: Vec<BeadRow> = conn
        .query_map(
            &issues_query,
            |(id, title, description, status, priority, assignee, notes): (
                String, String, String, String, i32, Option<String>, String,
            )| {
                BeadRow {
                    id,
                    title,
                    description,
                    status,
                    priority,
                    assignee,
                    notes,
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

    // Group comments by issue_id
    let mut comments_by_issue: std::collections::HashMap<String, Vec<&CommentRow>> =
        std::collections::HashMap::new();
    for comment in &comments {
        comments_by_issue
            .entry(comment.issue_id.clone())
            .or_default()
            .push(comment);
    }

    // Convert to Chunks
    let chunks: Vec<Chunk> = issues
        .into_iter()
        .map(|issue| {
            let mut content = format!("{}\n\n{}", issue.title, issue.description);

            if !issue.notes.is_empty() {
                content.push_str("\n\nNotes:\n");
                content.push_str(&issue.notes);
            }

            // Append metadata
            content.push_str(&format!(
                "\n\nStatus: {} | Priority: P{} | Assignee: {}",
                issue.status,
                issue.priority,
                issue.assignee.as_deref().unwrap_or("unassigned")
            ));

            // Append comments
            if let Some(issue_comments) = comments_by_issue.get(&issue.id) {
                content.push_str("\n\nComments:");
                for c in issue_comments {
                    content.push_str(&format!("\n--- {} ---\n{}", c.author, c.text));
                }
            }

            // Generate deterministic ID
            let id_input = format!("beads:{}:{}", rig, issue.id);
            let mut hasher = Sha256::new();
            hasher.update(id_input.as_bytes());
            let id = hex::encode(hasher.finalize());

            Chunk {
                id,
                file_path: format!("beads:{}:{}", rig, issue.id),
                chunk_type: ChunkType::Issue,
                name: Some(issue.title),
                start_line: 0,
                end_line: 0,
                content,
                language: "beads".to_string(),
            }
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
        let query = format!(
            "SELECT id, title, status, priority, assignee FROM issues WHERE id IN ({})",
            placeholders.join(", ")
        );

        let rows: Vec<(String, String, String, i32, Option<String>)> = conn
            .query(&query)
            .await
            .unwrap_or_default();

        for (id, title, status, priority, assignee) in rows {
            result.insert(
                id,
                LiveBeadMetadata {
                    status,
                    priority,
                    assignee,
                    title,
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
