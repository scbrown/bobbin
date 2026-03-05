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
    /// Feedback records that have at least one lineage action.
    #[serde(default)]
    pub actioned: u64,
    /// Feedback records with no lineage action.
    #[serde(default)]
    pub unactioned: u64,
    /// Total lineage records.
    #[serde(default)]
    pub lineage_records: u64,
}

/// Input for recording a lineage action that resolves feedback.
#[derive(Debug, Deserialize)]
pub struct LineageInput {
    /// Feedback record IDs this action resolves.
    pub feedback_ids: Vec<i64>,
    /// Type of action taken.
    pub action_type: String,
    /// Associated bead ID (e.g., "bo-a94q").
    #[serde(default)]
    pub bead: Option<String>,
    /// Git commit hash.
    #[serde(default)]
    pub commit_hash: Option<String>,
    /// Human-readable description of what was done.
    pub description: String,
    /// Agent that created this record.
    #[serde(default)]
    pub agent: Option<String>,
}

/// A stored lineage record.
#[derive(Debug, Serialize)]
pub struct LineageRecord {
    pub id: i64,
    pub timestamp: String,
    pub action_type: String,
    pub bead: Option<String>,
    pub commit_hash: Option<String>,
    pub description: String,
    pub agent: Option<String>,
    /// Feedback IDs linked to this action.
    pub feedback_ids: Vec<i64>,
}

/// Query parameters for listing lineage records.
#[derive(Debug, Default, Deserialize)]
pub struct LineageQuery {
    pub feedback_id: Option<i64>,
    pub bead: Option<String>,
    pub commit_hash: Option<String>,
    pub limit: Option<usize>,
}

/// Full injection detail with associated feedback.
#[derive(Debug, Serialize)]
pub struct InjectionDetail {
    pub injection_id: String,
    pub timestamp: String,
    pub session_id: Option<String>,
    pub agent: Option<String>,
    pub query: Option<String>,
    pub files: Vec<String>,
    pub total_chunks: i64,
    pub budget_lines: i64,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub formatted_output: String,
    pub feedback: Vec<FeedbackRecord>,
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

        // Lineage tables for feedback-to-fix traceability
        self.conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS lineage (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                timestamp TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
                action_type TEXT NOT NULL,
                bead TEXT,
                commit_hash TEXT,
                description TEXT NOT NULL,
                agent TEXT
            );

            CREATE TABLE IF NOT EXISTS lineage_feedback (
                lineage_id INTEGER NOT NULL REFERENCES lineage(id),
                feedback_id INTEGER NOT NULL REFERENCES feedback(id),
                PRIMARY KEY (lineage_id, feedback_id)
            );

            CREATE INDEX IF NOT EXISTS idx_lineage_bead ON lineage(bead);
            CREATE INDEX IF NOT EXISTS idx_lineage_commit ON lineage(commit_hash);
        "#,
        )?;

        // Schema migration: add formatted_output column if missing
        let has_col: bool = self.conn.query_row(
            "SELECT COUNT(*) > 0 FROM pragma_table_info('injections') WHERE name = 'formatted_output'",
            [],
            |row| row.get(0),
        ).unwrap_or(false);
        if !has_col {
            self.conn.execute_batch(
                "ALTER TABLE injections ADD COLUMN formatted_output TEXT DEFAULT '';"
            )?;
        }

        Ok(())
    }

    /// Store an injection record (what bobbin injected, for later feedback reference).
    pub fn store_injection(
        &self,
        injection_id: &str,
        session_id: Option<&str>,
        agent: Option<&str>,
        query: &str,
        files: &[String],
        total_chunks: usize,
        budget_lines: usize,
    ) -> Result<()> {
        self.store_injection_with_output(injection_id, session_id, agent, query, files, total_chunks, budget_lines, None)
    }

    /// Store an injection record with the formatted output text agents see.
    pub fn store_injection_with_output(
        &self,
        injection_id: &str,
        session_id: Option<&str>,
        agent: Option<&str>,
        query: &str,
        files: &[String],
        total_chunks: usize,
        budget_lines: usize,
        formatted_output: Option<&str>,
    ) -> Result<()> {
        let files_json = serde_json::to_string(files).unwrap_or_default();
        let now = chrono::Utc::now()
            .format("%Y-%m-%dT%H:%M:%S%.fZ")
            .to_string();
        self.conn.execute(
            "INSERT OR REPLACE INTO injections (injection_id, timestamp, session_id, agent, query, files_json, total_chunks, budget_lines, formatted_output) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            rusqlite::params![injection_id, now, session_id, agent, query, files_json, total_chunks as i64, budget_lines as i64, formatted_output.unwrap_or("")],
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

        // Explicitly set timestamp — older DB schemas may lack DEFAULT clauses.
        let now = chrono::Utc::now()
            .format("%Y-%m-%dT%H:%M:%S%.fZ")
            .to_string();

        // Ensure the injection_id exists in the injections table (auto-create if needed).
        // Include timestamp explicitly for older schemas where it's NOT NULL without DEFAULT.
        self.conn.execute(
            "INSERT OR IGNORE INTO injections (injection_id, timestamp) VALUES (?1, ?2)",
            rusqlite::params![input.injection_id, now],
        )?;

        self.conn.execute(
            "INSERT INTO feedback (injection_id, timestamp, agent, rating, reason) VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![input.injection_id, now, input.agent, input.rating, input.reason],
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

        // Lineage stats
        let lineage_records: u64 = self.conn.query_row(
            "SELECT COUNT(*) FROM lineage",
            [],
            |row| row.get(0),
        ).unwrap_or(0);
        let actioned: u64 = self.conn.query_row(
            "SELECT COUNT(DISTINCT feedback_id) FROM lineage_feedback",
            [],
            |row| row.get(0),
        ).unwrap_or(0);
        let unactioned = total_feedback.saturating_sub(actioned);

        Ok(FeedbackStats {
            total_injections,
            total_feedback,
            useful,
            noise,
            harmful,
            actioned,
            unactioned,
            lineage_records,
        })
    }

    /// Record a lineage action that resolves one or more feedback records.
    pub fn store_lineage(&self, input: &LineageInput) -> Result<i64> {
        const VALID_ACTIONS: &[&str] = &[
            "access_rule",
            "tag_effect",
            "config_change",
            "code_fix",
            "exclusion_rule",
        ];
        if !VALID_ACTIONS.contains(&input.action_type.as_str()) {
            anyhow::bail!(
                "Invalid action_type '{}'. Must be one of: {}",
                input.action_type,
                VALID_ACTIONS.join(", ")
            );
        }
        if input.feedback_ids.is_empty() {
            anyhow::bail!("feedback_ids must not be empty");
        }

        let now = chrono::Utc::now()
            .format("%Y-%m-%dT%H:%M:%S%.fZ")
            .to_string();

        self.conn.execute(
            "INSERT INTO lineage (timestamp, action_type, bead, commit_hash, description, agent) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            rusqlite::params![now, input.action_type, input.bead, input.commit_hash, input.description, input.agent],
        )?;
        let lineage_id = self.conn.last_insert_rowid();

        for &fid in &input.feedback_ids {
            self.conn.execute(
                "INSERT OR IGNORE INTO lineage_feedback (lineage_id, feedback_id) VALUES (?1, ?2)",
                rusqlite::params![lineage_id, fid],
            )?;
        }

        Ok(lineage_id)
    }

    /// List lineage records with optional filters.
    pub fn list_lineage(&self, query: &LineageQuery) -> Result<Vec<LineageRecord>> {
        let limit = query.limit.unwrap_or(20).min(50);
        let mut sql = String::from("SELECT l.id, l.timestamp, l.action_type, l.bead, l.commit_hash, l.description, l.agent FROM lineage l");
        let mut params: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
        let mut where_added = false;

        if let Some(fid) = query.feedback_id {
            sql.push_str(" JOIN lineage_feedback lf ON l.id = lf.lineage_id WHERE lf.feedback_id = ?1");
            params.push(Box::new(fid));
            where_added = true;
        }

        if let Some(ref bead) = query.bead {
            if where_added {
                sql.push_str(&format!(" AND l.bead = ?{}", params.len() + 1));
            } else {
                sql.push_str(&format!(" WHERE l.bead = ?{}", params.len() + 1));
                where_added = true;
            }
            params.push(Box::new(bead.clone()));
        }

        if let Some(ref commit) = query.commit_hash {
            if where_added {
                sql.push_str(&format!(" AND l.commit_hash = ?{}", params.len() + 1));
            } else {
                sql.push_str(&format!(" WHERE l.commit_hash = ?{}", params.len() + 1));
            }
            params.push(Box::new(commit.clone()));
        }

        sql.push_str(&format!(
            " ORDER BY l.timestamp DESC LIMIT ?{}",
            params.len() + 1
        ));
        params.push(Box::new(limit as i64));

        let param_refs: Vec<&dyn rusqlite::types::ToSql> =
            params.iter().map(|p| p.as_ref()).collect();
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map(param_refs.as_slice(), |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, Option<String>>(3)?,
                row.get::<_, Option<String>>(4)?,
                row.get::<_, String>(5)?,
                row.get::<_, Option<String>>(6)?,
            ))
        })?;

        let mut records = Vec::new();
        for row in rows {
            let (id, timestamp, action_type, bead, commit_hash, description, agent) = row?;
            // Fetch linked feedback IDs
            let feedback_ids = self.lineage_feedback_ids(id)?;
            records.push(LineageRecord {
                id,
                timestamp,
                action_type,
                bead,
                commit_hash,
                description,
                agent,
                feedback_ids,
            });
        }
        Ok(records)
    }

    /// Get feedback IDs linked to a lineage record.
    fn lineage_feedback_ids(&self, lineage_id: i64) -> Result<Vec<i64>> {
        let mut stmt = self.conn.prepare(
            "SELECT feedback_id FROM lineage_feedback WHERE lineage_id = ?1 ORDER BY feedback_id",
        )?;
        let rows = stmt.query_map(rusqlite::params![lineage_id], |row| row.get(0))?;
        let mut ids = Vec::new();
        for row in rows {
            ids.push(row?);
        }
        Ok(ids)
    }

    /// Get full injection detail by ID, including associated feedback.
    pub fn get_injection(&self, injection_id: &str) -> Result<Option<InjectionDetail>> {
        let mut stmt = self.conn.prepare(
            "SELECT injection_id, timestamp, session_id, agent, query, files_json, total_chunks, budget_lines, COALESCE(formatted_output, '') FROM injections WHERE injection_id = ?1",
        )?;

        let mut rows = stmt.query(rusqlite::params![injection_id])?;
        let row = match rows.next()? {
            Some(r) => r,
            None => return Ok(None),
        };

        let files_json: Option<String> = row.get(5)?;
        let files: Vec<String> = files_json
            .and_then(|j| serde_json::from_str(&j).ok())
            .unwrap_or_default();
        let formatted_output: String = row.get(8)?;

        let feedback = self.list_feedback(&FeedbackQuery {
            injection_id: Some(injection_id.to_string()),
            limit: Some(50),
            ..Default::default()
        })?;

        Ok(Some(InjectionDetail {
            injection_id: row.get(0)?,
            timestamp: row.get(1)?,
            session_id: row.get(2)?,
            agent: row.get(3)?,
            query: row.get(4)?,
            files,
            total_chunks: row.get::<_, Option<i64>>(6)?.unwrap_or(0),
            budget_lines: row.get::<_, Option<i64>>(7)?.unwrap_or(0),
            formatted_output,
            feedback,
        }))
    }
    /// Get injection IDs from a session that have no feedback yet.
    pub fn unrated_injections_for_session(&self, session_id: &str) -> Result<Vec<String>> {
        let mut stmt = self.conn.prepare(
            "SELECT i.injection_id FROM injections i
             LEFT JOIN feedback f ON i.injection_id = f.injection_id
             WHERE i.session_id = ?1 AND f.id IS NULL
             ORDER BY i.timestamp DESC
             LIMIT 10",
        )?;
        let rows = stmt.query_map(rusqlite::params![session_id], |row| {
            row.get::<_, String>(0)
        })?;
        let mut ids = Vec::new();
        for row in rows {
            ids.push(row?);
        }
        Ok(ids)
    }

    /// Get feedback auto-tags (feedback:hot, feedback:cold) keyed by file path.
    /// Returns empty map if the feedback_tags table doesn't exist yet.
    pub fn get_feedback_tags(&self) -> Result<std::collections::HashMap<String, Vec<String>>> {
        let mut map = std::collections::HashMap::new();
        let has_table: bool = self.conn.query_row(
            "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='table' AND name='feedback_tags'",
            [],
            |row| row.get(0),
        )?;
        if !has_table {
            return Ok(map);
        }
        let mut stmt = self.conn.prepare("SELECT file_path, tag FROM feedback_tags")?;
        let rows = stmt.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        for row in rows {
            let (path, tag) = row?;
            map.entry(path).or_insert_with(Vec::new).push(tag);
        }
        Ok(map)
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

    #[test]
    fn test_store_injection() {
        let (store, _f) = temp_store();
        let files = vec!["src/main.rs".to_string(), "src/lib.rs".to_string()];
        store
            .store_injection(
                "inj-test1234",
                Some("session-abc"),
                Some("aegis/crew/test"),
                "how does auth work?",
                &files,
                5,
                300,
            )
            .unwrap();

        // Verify injection was stored
        let stats = store.stats().unwrap();
        assert_eq!(stats.total_injections, 1);

        // Feedback referencing this injection should work
        store
            .store_feedback(&FeedbackInput {
                injection_id: "inj-test1234".to_string(),
                agent: "test".to_string(),
                rating: "useful".to_string(),
                reason: "found the auth module".to_string(),
            })
            .unwrap();
        let stats = store.stats().unwrap();
        assert_eq!(stats.total_feedback, 1);
    }

    #[test]
    fn test_get_injection_detail() {
        let (store, _f) = temp_store();
        let files = vec!["src/main.rs".to_string(), "src/lib.rs".to_string()];
        store
            .store_injection("inj-detail", Some("sess-1"), Some("aegis/crew/ian"), "how does auth work?", &files, 5, 300)
            .unwrap();
        store
            .store_feedback(&FeedbackInput {
                injection_id: "inj-detail".to_string(),
                agent: "aegis/crew/ellie".to_string(),
                rating: "useful".to_string(),
                reason: "great context".to_string(),
            })
            .unwrap();

        let detail = store.get_injection("inj-detail").unwrap().unwrap();
        assert_eq!(detail.injection_id, "inj-detail");
        assert_eq!(detail.query.as_deref(), Some("how does auth work?"));
        assert_eq!(detail.files.len(), 2);
        assert_eq!(detail.total_chunks, 5);
        assert_eq!(detail.budget_lines, 300);
        assert_eq!(detail.feedback.len(), 1);
        assert_eq!(detail.feedback[0].rating, "useful");

        // Non-existent injection returns None
        assert!(store.get_injection("inj-nope").unwrap().is_none());
    }

    #[test]
    fn test_lineage_store_and_list() {
        let (store, _f) = temp_store();
        // Create feedback records first
        let fid1 = store
            .store_feedback(&FeedbackInput {
                injection_id: "inj-1".to_string(),
                agent: "test".to_string(),
                rating: "noise".to_string(),
                reason: "init function noise".to_string(),
            })
            .unwrap();
        let fid2 = store
            .store_feedback(&FeedbackInput {
                injection_id: "inj-2".to_string(),
                agent: "test".to_string(),
                rating: "noise".to_string(),
                reason: "init function noise".to_string(),
            })
            .unwrap();

        // Record lineage action
        let lid = store
            .store_lineage(&LineageInput {
                feedback_ids: vec![fid1, fid2],
                action_type: "tag_effect".to_string(),
                bead: Some("bo-a94q".to_string()),
                commit_hash: Some("4ded620".to_string()),
                description: "Deployed auto:init exclude".to_string(),
                agent: Some("aegis/crew/ian".to_string()),
            })
            .unwrap();
        assert!(lid > 0);

        // List all lineage
        let records = store.list_lineage(&LineageQuery::default()).unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].action_type, "tag_effect");
        assert_eq!(records[0].bead.as_deref(), Some("bo-a94q"));
        assert_eq!(records[0].feedback_ids, vec![fid1, fid2]);

        // Query by bead
        let records = store
            .list_lineage(&LineageQuery {
                bead: Some("bo-a94q".to_string()),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(records.len(), 1);

        // Query by feedback_id
        let records = store
            .list_lineage(&LineageQuery {
                feedback_id: Some(fid1),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(records.len(), 1);
    }

    #[test]
    fn test_lineage_stats() {
        let (store, _f) = temp_store();
        let fid1 = store
            .store_feedback(&FeedbackInput {
                injection_id: "inj-1".to_string(),
                agent: "test".to_string(),
                rating: "noise".to_string(),
                reason: "noise".to_string(),
            })
            .unwrap();
        let _fid2 = store
            .store_feedback(&FeedbackInput {
                injection_id: "inj-2".to_string(),
                agent: "test".to_string(),
                rating: "noise".to_string(),
                reason: "noise".to_string(),
            })
            .unwrap();

        // Before lineage: 0 actioned, 2 unactioned
        let stats = store.stats().unwrap();
        assert_eq!(stats.actioned, 0);
        assert_eq!(stats.unactioned, 2);
        assert_eq!(stats.lineage_records, 0);

        // Record lineage for fid1 only
        store
            .store_lineage(&LineageInput {
                feedback_ids: vec![fid1],
                action_type: "code_fix".to_string(),
                bead: None,
                commit_hash: Some("abc123".to_string()),
                description: "Fixed it".to_string(),
                agent: None,
            })
            .unwrap();

        let stats = store.stats().unwrap();
        assert_eq!(stats.actioned, 1);
        assert_eq!(stats.unactioned, 1);
        assert_eq!(stats.lineage_records, 1);
    }

    #[test]
    fn test_lineage_invalid_action_type() {
        let (store, _f) = temp_store();
        let fid = store
            .store_feedback(&FeedbackInput {
                injection_id: "inj-1".to_string(),
                agent: "test".to_string(),
                rating: "noise".to_string(),
                reason: "test".to_string(),
            })
            .unwrap();
        let result = store.store_lineage(&LineageInput {
            feedback_ids: vec![fid],
            action_type: "invalid".to_string(),
            bead: None,
            commit_hash: None,
            description: "test".to_string(),
            agent: None,
        });
        assert!(result.is_err());
    }
}
