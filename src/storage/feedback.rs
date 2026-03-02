use anyhow::{Context, Result};
use rusqlite::{Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::Mutex;

/// Injection record — what bobbin injected into an agent session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InjectionRecord {
    pub injection_id: String,
    pub timestamp: String,
    pub session_id: String,
    pub agent: String,
    pub query: String,
    pub files_returned: Vec<String>,
    pub chunk_ids: Vec<String>,
    pub total_chunks: u32,
    pub budget_lines: u32,
}

/// Feedback record — an agent's assessment of an injection.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeedbackRecord {
    pub id: i64,
    pub injection_id: String,
    pub timestamp: String,
    pub agent: String,
    pub rating: String,
    pub reason: String,
    pub chunks_referenced: Vec<String>,
}

/// Input for creating feedback (no auto-generated fields).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeedbackInput {
    pub injection_id: String,
    pub agent: Option<String>,
    pub session_id: Option<String>,
    pub rating: String,
    pub reason: Option<String>,
    pub chunks_referenced: Option<Vec<String>>,
}

/// Query parameters for listing feedback.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct FeedbackQuery {
    pub injection_id: Option<String>,
    pub rating: Option<String>,
    pub agent: Option<String>,
    pub limit: Option<u32>,
    pub offset: Option<u32>,
}

/// Storage for injection records and feedback, backed by SQLite.
///
/// Thread-safe via Mutex — axum handlers share this across requests.
pub struct FeedbackStore {
    conn: Mutex<Connection>,
}

impl FeedbackStore {
    /// Open or create a feedback store at the given path.
    pub fn open(path: &Path) -> Result<Self> {
        let conn = Connection::open(path)
            .with_context(|| format!("Failed to open feedback DB: {}", path.display()))?;

        conn.pragma_update(None, "journal_mode", "WAL")
            .context("Failed to set WAL mode")?;
        conn.execute("PRAGMA foreign_keys = ON", [])
            .context("Failed to enable foreign keys")?;

        let store = Self {
            conn: Mutex::new(conn),
        };
        store.init_schema()?;
        Ok(store)
    }

    fn init_schema(&self) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS injections (
                injection_id TEXT PRIMARY KEY,
                timestamp TEXT NOT NULL,
                session_id TEXT NOT NULL DEFAULT '',
                agent TEXT NOT NULL DEFAULT '',
                query TEXT NOT NULL DEFAULT '',
                files_json TEXT NOT NULL DEFAULT '[]',
                chunks_json TEXT NOT NULL DEFAULT '[]',
                total_chunks INTEGER NOT NULL DEFAULT 0,
                budget_lines INTEGER NOT NULL DEFAULT 0
            );

            CREATE TABLE IF NOT EXISTS feedback (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                injection_id TEXT NOT NULL,
                timestamp TEXT NOT NULL,
                agent TEXT NOT NULL DEFAULT '',
                rating TEXT NOT NULL,
                reason TEXT NOT NULL DEFAULT '',
                chunks_referenced_json TEXT NOT NULL DEFAULT '[]',
                FOREIGN KEY (injection_id) REFERENCES injections(injection_id)
            );

            CREATE INDEX IF NOT EXISTS idx_feedback_injection ON feedback(injection_id);
            CREATE INDEX IF NOT EXISTS idx_feedback_rating ON feedback(rating);
            CREATE INDEX IF NOT EXISTS idx_feedback_timestamp ON feedback(timestamp DESC);
            CREATE INDEX IF NOT EXISTS idx_injections_timestamp ON injections(timestamp DESC);
            "#,
        )?;
        Ok(())
    }

    /// Store an injection record.
    pub fn store_injection(&self, record: &InjectionRecord) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            r#"INSERT OR REPLACE INTO injections
               (injection_id, timestamp, session_id, agent, query, files_json, chunks_json, total_chunks, budget_lines)
               VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)"#,
            (
                &record.injection_id,
                &record.timestamp,
                &record.session_id,
                &record.agent,
                &record.query,
                &serde_json::to_string(&record.files_returned).unwrap_or_default(),
                &serde_json::to_string(&record.chunk_ids).unwrap_or_default(),
                record.total_chunks,
                record.budget_lines,
            ),
        )?;
        Ok(())
    }

    /// Get an injection by ID.
    pub fn get_injection(&self, injection_id: &str) -> Result<Option<InjectionRecord>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            r#"SELECT injection_id, timestamp, session_id, agent, query, files_json, chunks_json, total_chunks, budget_lines
               FROM injections WHERE injection_id = ?1"#,
        )?;
        let result = stmt
            .query_row([injection_id], |row| {
                let files_json: String = row.get(5)?;
                let chunks_json: String = row.get(6)?;
                Ok(InjectionRecord {
                    injection_id: row.get(0)?,
                    timestamp: row.get(1)?,
                    session_id: row.get(2)?,
                    agent: row.get(3)?,
                    query: row.get(4)?,
                    files_returned: serde_json::from_str(&files_json).unwrap_or_default(),
                    chunk_ids: serde_json::from_str(&chunks_json).unwrap_or_default(),
                    total_chunks: row.get(7)?,
                    budget_lines: row.get(8)?,
                })
            })
            .optional()?;
        Ok(result)
    }

    /// Store a feedback record. Returns the auto-generated ID.
    pub fn store_feedback(&self, input: &FeedbackInput) -> Result<i64> {
        let conn = self.conn.lock().unwrap();
        let timestamp = chrono::Utc::now().to_rfc3339();
        let agent = input.agent.as_deref().unwrap_or("");
        let reason = input.reason.as_deref().unwrap_or("");
        let chunks_json =
            serde_json::to_string(input.chunks_referenced.as_deref().unwrap_or(&[])).unwrap_or_default();

        conn.execute(
            r#"INSERT INTO feedback (injection_id, timestamp, agent, rating, reason, chunks_referenced_json)
               VALUES (?1, ?2, ?3, ?4, ?5, ?6)"#,
            (
                &input.injection_id,
                &timestamp,
                agent,
                &input.rating,
                reason,
                &chunks_json,
            ),
        )?;
        Ok(conn.last_insert_rowid())
    }

    /// List feedback with optional filters.
    pub fn list_feedback(&self, query: &FeedbackQuery) -> Result<Vec<FeedbackRecord>> {
        let conn = self.conn.lock().unwrap();
        let mut sql = String::from(
            "SELECT id, injection_id, timestamp, agent, rating, reason, chunks_referenced_json FROM feedback WHERE 1=1",
        );
        let mut params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
        let mut param_idx = 1;

        if let Some(ref inj_id) = query.injection_id {
            sql.push_str(&format!(" AND injection_id = ?{param_idx}"));
            params.push(Box::new(inj_id.clone()));
            param_idx += 1;
        }
        if let Some(ref rating) = query.rating {
            sql.push_str(&format!(" AND rating = ?{param_idx}"));
            params.push(Box::new(rating.clone()));
            param_idx += 1;
        }
        if let Some(ref agent) = query.agent {
            sql.push_str(&format!(" AND agent = ?{param_idx}"));
            params.push(Box::new(agent.clone()));
            param_idx += 1;
        }

        sql.push_str(" ORDER BY timestamp DESC");

        let limit = query.limit.unwrap_or(50).min(500);
        let offset = query.offset.unwrap_or(0);
        sql.push_str(&format!(" LIMIT ?{param_idx} OFFSET ?{}", param_idx + 1));
        params.push(Box::new(limit));
        params.push(Box::new(offset));

        let param_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt
            .query_map(param_refs.as_slice(), |row| {
                let chunks_json: String = row.get(6)?;
                Ok(FeedbackRecord {
                    id: row.get(0)?,
                    injection_id: row.get(1)?,
                    timestamp: row.get(2)?,
                    agent: row.get(3)?,
                    rating: row.get(4)?,
                    reason: row.get(5)?,
                    chunks_referenced: serde_json::from_str(&chunks_json).unwrap_or_default(),
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// Get aggregate stats.
    pub fn stats(&self) -> Result<FeedbackStats> {
        let conn = self.conn.lock().unwrap();
        let total_injections: u64 =
            conn.query_row("SELECT COUNT(*) FROM injections", [], |r| r.get(0))?;
        let total_feedback: u64 =
            conn.query_row("SELECT COUNT(*) FROM feedback", [], |r| r.get(0))?;
        let useful: u64 = conn.query_row(
            "SELECT COUNT(*) FROM feedback WHERE rating = 'useful'",
            [],
            |r| r.get(0),
        )?;
        let noise: u64 = conn.query_row(
            "SELECT COUNT(*) FROM feedback WHERE rating = 'noise'",
            [],
            |r| r.get(0),
        )?;
        let harmful: u64 = conn.query_row(
            "SELECT COUNT(*) FROM feedback WHERE rating = 'harmful'",
            [],
            |r| r.get(0),
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

#[derive(Debug, Serialize, Deserialize)]
pub struct FeedbackStats {
    pub total_injections: u64,
    pub total_feedback: u64,
    pub useful: u64,
    pub noise: u64,
    pub harmful: u64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn create_test_store() -> (FeedbackStore, tempfile::TempDir) {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("feedback.db");
        let store = FeedbackStore::open(&db_path).unwrap();
        (store, dir)
    }

    #[test]
    fn test_store_and_get_injection() {
        let (store, _dir) = create_test_store();
        let record = InjectionRecord {
            injection_id: "inj-test123".to_string(),
            timestamp: "2026-03-02T14:30:00Z".to_string(),
            session_id: "sess-xyz".to_string(),
            agent: "aegis/crew/ian".to_string(),
            query: "how does auth work".to_string(),
            files_returned: vec!["src/auth.rs".to_string(), "src/middleware.rs".to_string()],
            chunk_ids: vec!["c1".to_string(), "c2".to_string()],
            total_chunks: 2,
            budget_lines: 300,
        };
        store.store_injection(&record).unwrap();

        let retrieved = store.get_injection("inj-test123").unwrap().unwrap();
        assert_eq!(retrieved.injection_id, "inj-test123");
        assert_eq!(retrieved.agent, "aegis/crew/ian");
        assert_eq!(retrieved.files_returned.len(), 2);
        assert_eq!(retrieved.files_returned[0], "src/auth.rs");
    }

    #[test]
    fn test_store_and_list_feedback() {
        let (store, _dir) = create_test_store();

        // Must create injection first (FK constraint)
        store
            .store_injection(&InjectionRecord {
                injection_id: "inj-001".to_string(),
                timestamp: "2026-03-02T14:30:00Z".to_string(),
                session_id: "sess-1".to_string(),
                agent: "test".to_string(),
                query: "test query".to_string(),
                files_returned: vec![],
                chunk_ids: vec![],
                total_chunks: 0,
                budget_lines: 0,
            })
            .unwrap();

        let id = store
            .store_feedback(&FeedbackInput {
                injection_id: "inj-001".to_string(),
                agent: Some("aegis/crew/ian".to_string()),
                session_id: None,
                rating: "useful".to_string(),
                reason: Some("exactly what I needed".to_string()),
                chunks_referenced: Some(vec!["c1".to_string()]),
            })
            .unwrap();
        assert!(id > 0);

        store
            .store_feedback(&FeedbackInput {
                injection_id: "inj-001".to_string(),
                agent: Some("aegis/crew/ellie".to_string()),
                session_id: None,
                rating: "noise".to_string(),
                reason: Some("irrelevant".to_string()),
                chunks_referenced: None,
            })
            .unwrap();

        // List all
        let all = store
            .list_feedback(&FeedbackQuery::default())
            .unwrap();
        assert_eq!(all.len(), 2);

        // Filter by rating
        let useful_only = store
            .list_feedback(&FeedbackQuery {
                rating: Some("useful".to_string()),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(useful_only.len(), 1);
        assert_eq!(useful_only[0].rating, "useful");

        // Filter by injection_id
        let by_inj = store
            .list_feedback(&FeedbackQuery {
                injection_id: Some("inj-001".to_string()),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(by_inj.len(), 2);
    }

    #[test]
    fn test_stats() {
        let (store, _dir) = create_test_store();

        store
            .store_injection(&InjectionRecord {
                injection_id: "inj-s1".to_string(),
                timestamp: "2026-03-02T14:30:00Z".to_string(),
                session_id: "s1".to_string(),
                agent: "test".to_string(),
                query: "q".to_string(),
                files_returned: vec![],
                chunk_ids: vec![],
                total_chunks: 0,
                budget_lines: 0,
            })
            .unwrap();

        store
            .store_feedback(&FeedbackInput {
                injection_id: "inj-s1".to_string(),
                agent: None,
                session_id: None,
                rating: "useful".to_string(),
                reason: None,
                chunks_referenced: None,
            })
            .unwrap();

        store
            .store_feedback(&FeedbackInput {
                injection_id: "inj-s1".to_string(),
                agent: None,
                session_id: None,
                rating: "noise".to_string(),
                reason: None,
                chunks_referenced: None,
            })
            .unwrap();

        let stats = store.stats().unwrap();
        assert_eq!(stats.total_injections, 1);
        assert_eq!(stats.total_feedback, 2);
        assert_eq!(stats.useful, 1);
        assert_eq!(stats.noise, 1);
        assert_eq!(stats.harmful, 0);
    }

    #[test]
    fn test_missing_injection_returns_none() {
        let (store, _dir) = create_test_store();
        assert!(store.get_injection("nonexistent").unwrap().is_none());
    }
}
