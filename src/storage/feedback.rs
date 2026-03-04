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

/// Per-file overlap statistics.
#[derive(Debug, Serialize)]
pub struct FileOverlapStat {
    pub file_path: String,
    pub used_count: u64,
    pub injected_count: u64,
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

            CREATE TABLE IF NOT EXISTS overlaps (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                injection_id TEXT NOT NULL,
                file_path TEXT NOT NULL,
                tool_name TEXT NOT NULL,
                session_id TEXT,
                timestamp TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
            );

            CREATE INDEX IF NOT EXISTS idx_overlaps_injection ON overlaps(injection_id);
            CREATE INDEX IF NOT EXISTS idx_overlaps_file ON overlaps(file_path);
            CREATE INDEX IF NOT EXISTS idx_overlaps_session ON overlaps(session_id);

            CREATE TABLE IF NOT EXISTS feedback_tags (
                file_path TEXT NOT NULL,
                tag TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                PRIMARY KEY (file_path, tag)
            );
        "#,
        )?;
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
        let files_json = serde_json::to_string(files).unwrap_or_default();
        let now = chrono::Utc::now()
            .format("%Y-%m-%dT%H:%M:%S%.fZ")
            .to_string();
        self.conn.execute(
            "INSERT OR REPLACE INTO injections (injection_id, timestamp, session_id, agent, query, files_json, total_chunks, budget_lines) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            rusqlite::params![injection_id, now, session_id, agent, query, files_json, total_chunks as i64, budget_lines as i64],
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

    /// Get recent injections for a session (for overlap checking).
    /// Returns list of (injection_id, files_json) tuples.
    pub fn get_session_injections(&self, session_id: &str, limit: usize) -> Result<Vec<(String, Vec<String>)>> {
        let mut stmt = self.conn.prepare(
            "SELECT injection_id, files_json FROM injections WHERE session_id = ?1 ORDER BY timestamp DESC LIMIT ?2"
        )?;
        let rows = stmt.query_map(rusqlite::params![session_id, limit as i64], |row| {
            let inj_id: String = row.get(0)?;
            let files_json: Option<String> = row.get(1)?;
            Ok((inj_id, files_json))
        })?;

        let mut results = Vec::new();
        for row in rows {
            let (inj_id, files_json) = row?;
            let files: Vec<String> = files_json
                .and_then(|j| serde_json::from_str(&j).ok())
                .unwrap_or_default();
            results.push((inj_id, files));
        }
        Ok(results)
    }

    /// Store an overlap record (an injected file was later used by the agent).
    pub fn store_overlap(
        &self,
        injection_id: &str,
        file_path: &str,
        tool_name: &str,
        session_id: Option<&str>,
    ) -> Result<()> {
        let now = chrono::Utc::now()
            .format("%Y-%m-%dT%H:%M:%S%.fZ")
            .to_string();
        self.conn.execute(
            "INSERT INTO overlaps (injection_id, file_path, tool_name, session_id, timestamp) VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![injection_id, file_path, tool_name, session_id, now],
        )?;
        Ok(())
    }

    /// Get per-file overlap counts (for auto-tagging).
    /// Returns vec of (file_path, used_count, injected_count) sorted by used_count desc.
    pub fn file_overlap_stats(&self, limit: usize) -> Result<Vec<FileOverlapStat>> {
        let mut stmt = self.conn.prepare(
            r#"
            SELECT
                o.file_path,
                COUNT(DISTINCT o.injection_id) as used_count,
                (SELECT COUNT(DISTINCT i.injection_id)
                 FROM injections i
                 WHERE i.files_json LIKE '%' || o.file_path || '%') as injected_count
            FROM overlaps o
            GROUP BY o.file_path
            ORDER BY used_count DESC
            LIMIT ?1
            "#
        )?;
        let rows = stmt.query_map(rusqlite::params![limit as i64], |row| {
            Ok(FileOverlapStat {
                file_path: row.get(0)?,
                used_count: row.get(1)?,
                injected_count: row.get(2)?,
            })
        })?;
        let mut results = Vec::new();
        for row in rows {
            results.push(row?);
        }
        Ok(results)
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

    /// Compute auto-tag assignments from feedback + overlap signals.
    /// Returns (hot_files, cold_files) — lists of file paths to tag.
    ///
    /// Thresholds:
    /// - feedback:hot: file appears in >= `hot_threshold` overlaps (agent used it after injection)
    /// - feedback:cold: file was injected >= `cold_injections` times but appeared in
    ///   0 overlaps, OR has >= `cold_noise_threshold` noise/harmful feedback
    pub fn compute_autotags(
        &self,
        hot_threshold: u64,
        cold_injections: u64,
        cold_noise_threshold: u64,
    ) -> Result<AutoTagResult> {
        // Hot files: files with enough overlaps (agent actually used them)
        let mut hot_stmt = self.conn.prepare(
            "SELECT file_path, COUNT(DISTINCT injection_id) as used_count \
             FROM overlaps GROUP BY file_path HAVING used_count >= ?1"
        )?;
        let hot_files: Vec<String> = hot_stmt
            .query_map(rusqlite::params![hot_threshold as i64], |row| row.get(0))?
            .filter_map(|r| r.ok())
            .collect();

        // Cold files: injected frequently but never used (no overlaps at all)
        // We look at files in injections that have no matching overlap records
        let mut cold_stmt = self.conn.prepare(
            r#"
            SELECT DISTINCT jf.file_path
            FROM (
                SELECT injection_id, json_each.value as file_path
                FROM injections, json_each(injections.files_json)
            ) jf
            LEFT JOIN overlaps o ON o.file_path = jf.file_path
            WHERE o.id IS NULL
            GROUP BY jf.file_path
            HAVING COUNT(DISTINCT jf.injection_id) >= ?1
            "#
        )?;
        let mut cold_files: Vec<String> = cold_stmt
            .query_map(rusqlite::params![cold_injections as i64], |row| row.get(0))?
            .filter_map(|r| r.ok())
            .collect();

        // Also cold: files with enough noise/harmful feedback
        let mut noise_stmt = self.conn.prepare(
            r#"
            SELECT DISTINCT i.files_json
            FROM feedback f
            JOIN injections i ON i.injection_id = f.injection_id
            WHERE f.rating IN ('noise', 'harmful')
            GROUP BY f.injection_id
            HAVING COUNT(*) >= ?1
            "#
        )?;
        let noise_rows: Vec<String> = noise_stmt
            .query_map(rusqlite::params![cold_noise_threshold as i64], |row| {
                let json: String = row.get(0)?;
                Ok(json)
            })?
            .filter_map(|r| r.ok())
            .collect();
        for json in &noise_rows {
            if let Ok(files) = serde_json::from_str::<Vec<String>>(json) {
                for f in files {
                    if !cold_files.contains(&f) && !hot_files.contains(&f) {
                        cold_files.push(f);
                    }
                }
            }
        }

        Ok(AutoTagResult { hot_files, cold_files })
    }
}

/// Result of auto-tag computation.
#[derive(Debug, Serialize)]
pub struct AutoTagResult {
    pub hot_files: Vec<String>,
    pub cold_files: Vec<String>,
}

impl FeedbackStore {
    /// Apply computed auto-tags to the feedback_tags table.
    /// Clears previous feedback:hot/cold tags and inserts new ones.
    pub fn apply_autotags(&self, result: &AutoTagResult) -> Result<()> {
        let now = chrono::Utc::now()
            .format("%Y-%m-%dT%H:%M:%S%.fZ")
            .to_string();

        // Clear old feedback tags
        self.conn.execute(
            "DELETE FROM feedback_tags WHERE tag IN ('feedback:hot', 'feedback:cold')",
            [],
        )?;

        // Insert new ones
        let mut stmt = self.conn.prepare(
            "INSERT OR REPLACE INTO feedback_tags (file_path, tag, updated_at) VALUES (?1, ?2, ?3)"
        )?;
        for f in &result.hot_files {
            stmt.execute(rusqlite::params![f, "feedback:hot", now])?;
        }
        for f in &result.cold_files {
            stmt.execute(rusqlite::params![f, "feedback:cold", now])?;
        }
        Ok(())
    }

    /// Get all feedback tags (for merging during indexing).
    /// Returns a map of file_path -> vec of tags.
    pub fn get_feedback_tags(&self) -> Result<std::collections::HashMap<String, Vec<String>>> {
        let mut stmt = self.conn.prepare(
            "SELECT file_path, tag FROM feedback_tags ORDER BY file_path"
        )?;
        let rows = stmt.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;

        let mut map: std::collections::HashMap<String, Vec<String>> = std::collections::HashMap::new();
        for row in rows {
            let (file_path, tag) = row?;
            map.entry(file_path).or_default().push(tag);
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
    fn test_overlap_tracking() {
        let (store, _f) = temp_store();

        // Store an injection
        let files = vec!["src/main.rs".to_string(), "src/lib.rs".to_string()];
        store.store_injection("inj-1", Some("sess-1"), Some("test"), "query", &files, 3, 300).unwrap();

        // Record overlaps
        store.store_overlap("inj-1", "src/main.rs", "Edit", Some("sess-1")).unwrap();
        store.store_overlap("inj-1", "src/main.rs", "Write", Some("sess-1")).unwrap();

        // Query session injections
        let session_inj = store.get_session_injections("sess-1", 10).unwrap();
        assert_eq!(session_inj.len(), 1);
        assert_eq!(session_inj[0].0, "inj-1");
        assert_eq!(session_inj[0].1, vec!["src/main.rs", "src/lib.rs"]);

        // Check overlap stats
        let stats = store.file_overlap_stats(10).unwrap();
        assert_eq!(stats.len(), 1);
        assert_eq!(stats[0].file_path, "src/main.rs");
        assert_eq!(stats[0].used_count, 1); // 1 unique injection_id
    }

    #[test]
    fn test_session_injections_empty() {
        let (store, _f) = temp_store();
        let result = store.get_session_injections("nonexistent", 10).unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn test_compute_autotags_hot() {
        let (store, _f) = temp_store();

        // Create injection and overlaps
        let files = vec!["src/hot.rs".to_string()];
        store.store_injection("inj-1", Some("s1"), None, "q", &files, 1, 100).unwrap();
        store.store_injection("inj-2", Some("s2"), None, "q", &files, 1, 100).unwrap();

        // 5 overlaps from different injections
        for i in 1..=5 {
            let inj = format!("inj-hot-{}", i);
            store.store_injection(&inj, Some("s"), None, "q", &files, 1, 100).unwrap();
            store.store_overlap(&inj, "src/hot.rs", "Edit", Some("s")).unwrap();
        }

        let result = store.compute_autotags(5, 10, 3).unwrap();
        assert!(result.hot_files.contains(&"src/hot.rs".to_string()));
    }

    #[test]
    fn test_apply_and_get_feedback_tags() {
        let (store, _f) = temp_store();
        let result = AutoTagResult {
            hot_files: vec!["src/hot.rs".to_string()],
            cold_files: vec!["src/cold.rs".to_string()],
        };
        store.apply_autotags(&result).unwrap();

        let tags = store.get_feedback_tags().unwrap();
        assert_eq!(tags.get("src/hot.rs").unwrap(), &vec!["feedback:hot".to_string()]);
        assert_eq!(tags.get("src/cold.rs").unwrap(), &vec!["feedback:cold".to_string()]);

        // Re-apply with different data should replace
        let result2 = AutoTagResult {
            hot_files: vec!["src/new_hot.rs".to_string()],
            cold_files: vec![],
        };
        store.apply_autotags(&result2).unwrap();
        let tags2 = store.get_feedback_tags().unwrap();
        assert!(tags2.get("src/hot.rs").is_none()); // old hot removed
        assert!(tags2.get("src/cold.rs").is_none()); // old cold removed
        assert_eq!(tags2.get("src/new_hot.rs").unwrap(), &vec!["feedback:hot".to_string()]);
    }
}
