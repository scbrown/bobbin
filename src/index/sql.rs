//! Generic SQL database source — index query results as searchable chunks.
//!
//! A clone of the beads pattern (`beads.rs` proved non-filesystem MySQL
//! sources work) generalized to any table: a configured query runs against
//! a MySQL-protocol database, each row becomes one chunk with a stable
//! id derived from the row's primary key, and the shared content-hash
//! incremental driver ([`super::source::index_hashed_source`]) keeps
//! re-index runs cheap and removals swept.
//!
//! Credentials never live in config: `url_env` names an environment
//! variable holding the connection URL (`mysql://user:pass@host:port/db`).

use anyhow::{Context, Result};
use mysql_async::prelude::*;

use super::source::ChunkSource;
use crate::config::SqlSourceConfig;
use crate::types::{Chunk, ChunkType};

/// One configured SQL source, ready to fetch.
pub struct SqlSource {
    config: SqlSourceConfig,
    url: String,
    repo_key: String,
}

impl SqlSource {
    /// Resolve the connection URL from the configured environment variable.
    pub fn new(config: &SqlSourceConfig) -> Result<Self> {
        let url = std::env::var(&config.url_env).with_context(|| {
            format!(
                "SQL source '{}': environment variable {} is not set (expected a mysql:// URL)",
                config.name, config.url_env
            )
        })?;
        Ok(Self {
            repo_key: format!("sql-{}", config.name),
            config: config.clone(),
            url,
        })
    }
}

impl ChunkSource for SqlSource {
    fn name(&self) -> &str {
        &self.config.name
    }

    fn repo_key(&self) -> &str {
        // Distinct namespace per source so rows never collide with source
        // repos or other SQL sources.
        &self.repo_key
    }

    fn source_label(&self) -> &str {
        "sql"
    }

    async fn fetch(&self) -> Result<Vec<Chunk>> {
        let pool = mysql_async::Pool::new(self.url.as_str());
        let mut conn = pool
            .get_conn()
            .await
            .with_context(|| format!("SQL source '{}': connection failed", self.config.name))?;

        let rows: Vec<mysql_async::Row> = conn
            .query(&self.config.query)
            .await
            .with_context(|| format!("SQL source '{}': query failed", self.config.name))?;

        let mut chunks = Vec::new();
        for row in rows {
            let columns: Vec<(String, String)> = row
                .columns_ref()
                .iter()
                .enumerate()
                .map(|(i, col)| {
                    (
                        col.name_str().to_string(),
                        row.as_ref(i).map(value_to_string).unwrap_or_default(),
                    )
                })
                .collect();
            let Some(pk) = columns
                .iter()
                .find(|(n, _)| *n == self.config.id_column)
                .map(|(_, v)| v.clone())
            else {
                anyhow::bail!(
                    "SQL source '{}': id_column '{}' not in query results",
                    self.config.name,
                    self.config.id_column
                );
            };
            chunks.push(row_to_chunk(&self.config, &pk, &columns));
        }

        drop(conn);
        pool.disconnect().await.ok();
        Ok(chunks)
    }
}

/// Render a MySQL value as text for content assembly.
fn value_to_string(v: &mysql_async::Value) -> String {
    match v {
        mysql_async::Value::NULL => String::new(),
        mysql_async::Value::Bytes(b) => String::from_utf8_lossy(b).to_string(),
        other => other.as_sql(false).trim_matches('\'').to_string(),
    }
}

/// Build the chunk for one row. Pure, so the content layout is testable
/// without a live database (the beads precedent).
fn row_to_chunk(config: &SqlSourceConfig, pk: &str, columns: &[(String, String)]) -> Chunk {
    let key = format!("sql:{}:{}", config.name, pk);
    let id = {
        use sha2::{Digest, Sha256};
        hex::encode(Sha256::digest(key.as_bytes()))
    };

    let mut content = format!("{} #{}\n", config.name, pk);
    for (name, value) in columns {
        if config.text_columns.is_empty() || config.text_columns.iter().any(|c| c == name) {
            if !value.is_empty() {
                content.push_str(&format!("{}: {}\n", name, value));
            }
        }
    }

    let mut tags: Vec<String> = config
        .tag_columns
        .iter()
        .filter_map(|tc| {
            columns
                .iter()
                .find(|(n, v)| n == tc && !v.is_empty())
                .map(|(n, v)| format!("{}:{}", n, v))
        })
        .collect();
    tags.sort();

    Chunk {
        id,
        file_path: key,
        chunk_type: ChunkType::Doc,
        name: Some(format!("{} #{}", config.name, pk)),
        start_line: 0,
        end_line: 0,
        content,
        language: "sql".to_string(),
        tags: tags.join(","),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config() -> SqlSourceConfig {
        SqlSourceConfig {
            name: "tickets".to_string(),
            url_env: "TEST_SQL_URL".to_string(),
            query: "SELECT * FROM tickets".to_string(),
            id_column: "id".to_string(),
            text_columns: vec!["title".to_string(), "body".to_string()],
            tag_columns: vec!["status".to_string()],
        }
    }

    #[test]
    fn row_content_uses_text_columns_and_tags() {
        let columns = vec![
            ("id".to_string(), "42".to_string()),
            ("title".to_string(), "Login broken".to_string()),
            ("body".to_string(), "Cannot sign in".to_string()),
            ("status".to_string(), "open".to_string()),
            ("secret".to_string(), "do-not-index".to_string()),
        ];
        let chunk = row_to_chunk(&test_config(), "42", &columns);

        assert_eq!(chunk.file_path, "sql:tickets:42");
        assert_eq!(chunk.name.as_deref(), Some("tickets #42"));
        assert!(chunk.content.contains("title: Login broken"));
        assert!(chunk.content.contains("body: Cannot sign in"));
        // Non-text columns stay out of the embedded content.
        assert!(!chunk.content.contains("do-not-index"));
        assert_eq!(chunk.tags, "status:open");
        // Stable id: same key always hashes the same.
        let again = row_to_chunk(&test_config(), "42", &columns);
        assert_eq!(chunk.id, again.id);
    }

    #[test]
    fn empty_text_columns_means_all_columns() {
        let mut config = test_config();
        config.text_columns = vec![];
        let columns = vec![
            ("id".to_string(), "1".to_string()),
            ("anything".to_string(), "value".to_string()),
        ];
        let chunk = row_to_chunk(&config, "1", &columns);
        assert!(chunk.content.contains("anything: value"));
    }
}
