//! Feedback storage using SQLite.
//!
//! Stores injection records (what bobbin injected) and agent feedback
//! (useful/noise/harmful ratings) for tuning search quality.

use anyhow::{Context, Result};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::path::Path;

/// Feedback store backed by SQLite.
pub struct FeedbackStore {
    conn: Connection,
}

/// Input for submitting feedback on an injection.
#[derive(Debug, Deserialize)]
pub struct FeedbackInput {
    pub injection_id: String,
    pub agent: String,
    #[serde(default)]
    pub rating: String,
    #[serde(default)]
    pub reason: String,
}

/// A stored feedback record.
#[derive(Debug, Serialize)]
pub struct FeedbackRecord {
    pub id: i64,
    pub injection_id: String,
    pub timestamp: String,
    pub agent: String,
    pub rating: String,
    pub reason: String,
}

/// Query parameters for listing feedback.
#[derive(Debug, Default, Deserialize)]
pub struct FeedbackQuery {
    pub injection_id: Option<String>,
    pub rating: Option<String>,
    pub agent: Option<String>,
    pub limit: Option<usize>,
}

/// Aggregated feedback statistics.
#[derive(Debug, Serialize)]
pub struct FeedbackStats {
    pub total_injections: u64,
    pub total_feedback: u64,
    pub useful: u64,
    pub noise: u64,
    pub harmful: u64,
}

const VALID_RATINGS: &[&str] = &["useful", "noise", "harmful"];

impl FeedbackStore {
    /// Open or create a feedback store at the given path.
    pub fn open(path: &Path) -> Result<Self> {
        let conn = Connection::open(path)
            .with_context(|| format!("Failed to open feedback database: {}", path.display()))?;

        conn.execute_batch("PRAGMA journal_mode = WAL; PRAGMA foreign_keys = ON;")
            .context("Failed to set pragmas")?;

        let store = Self { conn };
        store.init_schema()?;
        Ok(store)
    }

    fn init_schema(&self) -> Result<()> {
        self.conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS injections (
                injection_id TEXT PRIMARY KEY,
                timestamp TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
                session_id TEXT,
                agent TEXT,
                query TEXT,
                files_json TEXT,
                chunks_json TEXT,
                total_chunks INTEGER,
                budget_lines INTEGER
            );

            CREATE TABLE IF NOT EXISTS feedback (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                injection_id TEXT NOT NULL,
                timestamp TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
                agent TEXT NOT NULL,
                rating TEXT NOT NULL,
                reason TEXT DEFAULT ''
            );

            CREATE INDEX IF NOT EXISTS idx_feedback_injection ON feedback(injection_id);
            CREATE INDEX IF NOT EXISTS idx_feedback_rating ON feedback(rating);
            CREATE INDEX IF NOT EXISTS idx_feedback_agent ON feedback(agent);
        "#,
        )?;
        Ok(())
    }

    /// Store feedback for an injection.
    pub fn store_feedback(&self, input: &FeedbackInput) -> Result<i64> {
        if !VALID_RATINGS.contains(&input.rating.as_str()) {
            anyhow::bail!(
                "Invalid rating '{}'. Must be one of: {}",
                input.rating,
                VALID_RATINGS.join(", ")
            );
        }
        if input.injection_id.is_empty() {
            anyhow::bail!("injection_id is required");
        }

        // Ensure the injection_id exists in the injections table (auto-create if needed)
        self.conn.execute(
            "INSERT OR IGNORE INTO injections (injection_id) VALUES (?1)",
            [&input.injection_id],
        )?;

        self.conn.execute(
            "INSERT INTO feedback (injection_id, agent, rating, reason) VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![input.injection_id, input.agent, input.rating, input.reason],
        )?;

        Ok(self.conn.last_insert_rowid())
    }

    /// List feedback records with optional filters.
    pub fn list_feedback(&self, query: &FeedbackQuery) -> Result<Vec<FeedbackRecord>> {
        let limit = query.limit.unwrap_or(20).min(50);
        let mut sql = String::from(
            "SELECT id, injection_id, timestamp, agent, rating, reason FROM feedback WHERE 1=1",
        );
        let mut params: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();

        if let Some(ref inj) = query.injection_id {
            sql.push_str(&format!(" AND injection_id = ?{}", params.len() + 1));
            params.push(Box::new(inj.clone()));
        }
        if let Some(ref rating) = query.rating {
            sql.push_str(&format!(" AND rating = ?{}", params.len() + 1));
            params.push(Box::new(rating.clone()));
        }
        if let Some(ref agent) = query.agent {
            sql.push_str(&format!(" AND agent = ?{}", params.len() + 1));
            params.push(Box::new(agent.clone()));
        }

        sql.push_str(&format!(
            " ORDER BY timestamp DESC LIMIT ?{}",
            params.len() + 1
        ));
        params.push(Box::new(limit as i64));

        let param_refs: Vec<&dyn rusqlite::types::ToSql> = params.iter().map(|p| p.as_ref()).collect();
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map(param_refs.as_slice(), |row| {
            Ok(FeedbackRecord {
                id: row.get(0)?,
                injection_id: row.get(1)?,
                timestamp: row.get(2)?,
                agent: row.get(3)?,
                rating: row.get(4)?,
                reason: row.get(5)?,
            })
        })?;

        let mut records = Vec::new();
        for row in rows {
            records.push(row?);
        }
        Ok(records)
    }

    /// Get aggregated feedback statistics.
    pub fn stats(&self) -> Result<FeedbackStats> {
        let total_injections: u64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM injections", [], |row| row.get(0))?;

        let total_feedback: u64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM feedback", [], |row| row.get(0))?;

        let useful: u64 = self.conn.query_row(
            "SELECT COUNT(*) FROM feedback WHERE rating = 'useful'",
            [],
            |row| row.get(0),
        )?;
        let noise: u64 = self.conn.query_row(
            "SELECT COUNT(*) FROM feedback WHERE rating = 'noise'",
            [],
            |row| row.get(0),
        )?;
        let harmful: u64 = self.conn.query_row(
            "SELECT COUNT(*) FROM feedback WHERE rating = 'harmful'",
            [],
            |row| row.get(0),
        )?;

        Ok(FeedbackStats {
            total_injections,
            total_feedback,
            useful,
            noise,
            harmful,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;

    fn temp_store() -> (FeedbackStore, NamedTempFile) {
        let f = NamedTempFile::new().unwrap();
        let store = FeedbackStore::open(f.path()).unwrap();
        (store, f)
    }

    #[test]
    fn test_store_and_list_feedback() {
        let (store, _f) = temp_store();
        let input = FeedbackInput {
            injection_id: "inj-abc123".to_string(),
            agent: "aegis/crew/test".to_string(),
            rating: "useful".to_string(),
            reason: "helped find the right file".to_string(),
        };
        let id = store.store_feedback(&input).unwrap();
        assert!(id > 0);

        let records = store.list_feedback(&FeedbackQuery::default()).unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].injection_id, "inj-abc123");
        assert_eq!(records[0].rating, "useful");
    }

    #[test]
    fn test_invalid_rating() {
        let (store, _f) = temp_store();
        let input = FeedbackInput {
            injection_id: "inj-abc123".to_string(),
            agent: "test".to_string(),
            rating: "bad".to_string(),
            reason: String::new(),
        };
        assert!(store.store_feedback(&input).is_err());
    }

    #[test]
    fn test_stats() {
        let (store, _f) = temp_store();
        for (inj, rating) in [
            ("inj-1", "useful"),
            ("inj-1", "noise"),
            ("inj-2", "harmful"),
        ] {
            store
                .store_feedback(&FeedbackInput {
                    injection_id: inj.to_string(),
                    agent: "test".to_string(),
                    rating: rating.to_string(),
                    reason: String::new(),
                })
                .unwrap();
        }
        let stats = store.stats().unwrap();
        assert_eq!(stats.total_injections, 2);
        assert_eq!(stats.total_feedback, 3);
        assert_eq!(stats.useful, 1);
        assert_eq!(stats.noise, 1);
        assert_eq!(stats.harmful, 1);
    }

    #[test]
    fn test_filter_by_rating() {
        let (store, _f) = temp_store();
        for rating in ["useful", "noise", "useful"] {
            store
                .store_feedback(&FeedbackInput {
                    injection_id: "inj-1".to_string(),
                    agent: "test".to_string(),
                    rating: rating.to_string(),
                    reason: String::new(),
                })
                .unwrap();
        }
        let q = FeedbackQuery {
            rating: Some("useful".to_string()),
            ..Default::default()
        };
        let records = store.list_feedback(&q).unwrap();
        assert_eq!(records.len(), 2);
    }

    #[test]
    fn test_react_injection_id() {
        let (store, _f) = temp_store();
        let input = FeedbackInput {
            injection_id: "inj-react-abc123".to_string(),
            agent: "test".to_string(),
            rating: "useful".to_string(),
            reason: "reaction context was helpful".to_string(),
        };
        store.store_feedback(&input).unwrap();
        let records = store.list_feedback(&FeedbackQuery {
            injection_id: Some("inj-react-abc123".to_string()),
            ..Default::default()
        }).unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].injection_id, "inj-react-abc123");
    }
}
