use anyhow::{Context, Result};
use rusqlite::{Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::path::Path;

use crate::types::FileCoupling;

/// Git coupling and metadata storage using SQLite
///
/// After the LanceDB consolidation, SQLite only stores:
/// - Temporal coupling relationships (git co-change data)
/// - Global metadata key-value pairs (e.g., embedding model)
pub struct MetadataStore {
    conn: Connection,
}

impl MetadataStore {
    /// Open or create a metadata store at the given path
    pub fn open(path: &Path) -> Result<Self> {
        let conn = Connection::open(path)
            .with_context(|| format!("Failed to open database: {}", path.display()))?;

        conn.execute("PRAGMA foreign_keys = ON", [])
            .context("Failed to enable foreign keys")?;

        let store = Self { conn };
        store.init_schema()?;
        Ok(store)
    }

    /// Initialize the database schema
    fn init_schema(&self) -> Result<()> {
        self.conn.execute_batch(
            r#"
            -- Temporal coupling (git co-change relationships)
            CREATE TABLE IF NOT EXISTS coupling (
                file_a TEXT NOT NULL,
                file_b TEXT NOT NULL,
                score REAL,
                co_changes INTEGER,
                last_co_change INTEGER,
                PRIMARY KEY (file_a, file_b)
            );

            -- Global metadata
            CREATE TABLE IF NOT EXISTS meta (
                key TEXT PRIMARY KEY,
                value TEXT
            );

            -- File hash tracking for incremental indexing
            CREATE TABLE IF NOT EXISTS file_hashes (
                file_path TEXT PRIMARY KEY,
                hash TEXT NOT NULL
            );

            -- Bead → bundle → commit workflow telemetry (GH#9, Layer 1: logging).
            -- Each row records that a bead was linked to a commit / changeset, so
            -- later layers can mine which files matter for which kinds of work.
            CREATE TABLE IF NOT EXISTS bead_lineage (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
                bead_id TEXT NOT NULL,
                bead_type TEXT,
                commit_sha TEXT,
                bundle_slugs TEXT,
                touched_files TEXT,
                action_type TEXT
            );

            -- Indexes
            CREATE INDEX IF NOT EXISTS idx_coupling_score ON coupling(score DESC);
            CREATE INDEX IF NOT EXISTS idx_bead_lineage_bead ON bead_lineage(bead_id);
            CREATE INDEX IF NOT EXISTS idx_bead_lineage_commit ON bead_lineage(commit_sha);
        "#,
        )?;

        self.migrate_bead_lineage()?;

        Ok(())
    }

    /// Idempotently add columns introduced after the initial bead_lineage schema
    /// (telemetry Phase 0, bo-xrsy). SQLite has no `ADD COLUMN IF NOT EXISTS`, so
    /// we inspect `PRAGMA table_info` and only ALTER for genuinely-missing
    /// columns. Errors other than the additions themselves propagate — we do not
    /// blind-try-and-ignore. `bundle_slugs` already exists in the base schema and
    /// is intentionally absent here (this migration only adds new columns).
    fn migrate_bead_lineage(&self) -> Result<()> {
        let mut existing = std::collections::HashSet::new();
        {
            let mut stmt = self.conn.prepare("PRAGMA table_info(bead_lineage)")?;
            let rows = stmt.query_map([], |row| row.get::<_, String>(1))?;
            for r in rows {
                existing.insert(r?);
            }
        }
        // (column, SQL type) — additive only.
        let additions = [
            ("feature_id", "TEXT"),
            ("lines_added", "INTEGER"),
            ("lines_deleted", "INTEGER"),
            ("touched_symbols", "TEXT"),
        ];
        for (col, ty) in additions {
            if !existing.contains(col) {
                self.conn.execute(
                    &format!("ALTER TABLE bead_lineage ADD COLUMN {} {}", col, ty),
                    [],
                )?;
            }
        }
        Ok(())
    }

    /// Get file coupling data
    pub fn get_coupling(&self, file_path: &str, limit: usize) -> Result<Vec<FileCoupling>> {
        let mut stmt = self.conn.prepare(
            r#"SELECT file_a, file_b, score, co_changes, last_co_change
               FROM coupling
               WHERE file_a = ?1 OR file_b = ?1
               ORDER BY score DESC
               LIMIT ?2"#,
        )?;

        let results = stmt
            .query_map([file_path, &limit.to_string()], |row| {
                Ok(FileCoupling {
                    file_a: row.get(0)?,
                    file_b: row.get(1)?,
                    score: row.get(2)?,
                    co_changes: row.get(3)?,
                    last_co_change: row.get(4)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(results)
    }

    /// Update coupling data
    pub fn upsert_coupling(&self, coupling: &FileCoupling) -> Result<()> {
        self.conn.execute(
            r#"INSERT INTO coupling (file_a, file_b, score, co_changes, last_co_change)
               VALUES (?1, ?2, ?3, ?4, ?5)
               ON CONFLICT(file_a, file_b) DO UPDATE SET
                   score = excluded.score,
                   co_changes = excluded.co_changes,
                   last_co_change = excluded.last_co_change"#,
            (
                &coupling.file_a,
                &coupling.file_b,
                coupling.score,
                coupling.co_changes,
                coupling.last_co_change,
            ),
        )?;
        Ok(())
    }

    /// Begin a transaction for batch operations
    pub fn begin_transaction(&self) -> Result<()> {
        self.conn.execute("BEGIN TRANSACTION", [])?;
        Ok(())
    }

    /// Commit a transaction
    pub fn commit(&self) -> Result<()> {
        self.conn.execute("COMMIT", [])?;
        Ok(())
    }

    /// Get global metadata value
    pub fn get_meta(&self, key: &str) -> Result<Option<String>> {
        let mut stmt = self.conn.prepare("SELECT value FROM meta WHERE key = ?1")?;
        let result = stmt.query_row([key], |row| row.get(0)).optional()?;
        Ok(result)
    }

    /// Set global metadata value
    pub fn set_meta(&self, key: &str, value: &str) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO meta (key, value) VALUES (?1, ?2)",
            [key, value],
        )?;
        Ok(())
    }

    /// Get all metadata entries matching a key prefix (e.g., "repo_source:")
    pub fn get_meta_by_prefix(&self, prefix: &str) -> Result<Vec<(String, String)>> {
        let mut stmt = self.conn.prepare(
            "SELECT key, value FROM meta WHERE key LIKE ?1"
        )?;
        let pattern = format!("{}%", prefix);
        let rows = stmt.query_map([&pattern], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        let mut results = Vec::new();
        for row in rows {
            results.push(row?);
        }
        Ok(results)
    }

    /// Get all coupling edges above a minimum score threshold.
    pub fn all_coupling(&self, min_score: f32, limit: usize) -> Result<Vec<FileCoupling>> {
        let mut stmt = self.conn.prepare(
            r#"SELECT file_a, file_b, score, co_changes, last_co_change
               FROM coupling
               WHERE score >= ?1
               ORDER BY score DESC
               LIMIT ?2"#,
        )?;
        let results = stmt
            .query_map(rusqlite::params![min_score, limit], |row| {
                Ok(FileCoupling {
                    file_a: row.get(0)?,
                    file_b: row.get(1)?,
                    score: row.get(2)?,
                    co_changes: row.get(3)?,
                    last_co_change: row.get(4)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(results)
    }

    /// Clear all coupling data
    pub fn clear_coupling(&self) -> Result<()> {
        self.conn.execute("DELETE FROM coupling", [])?;
        Ok(())
    }

    /// Get the stored hash for a file (for incremental indexing)
    pub fn get_file_hash(&self, file_path: &str) -> Result<Option<String>> {
        let mut stmt = self
            .conn
            .prepare("SELECT hash FROM file_hashes WHERE file_path = ?1")?;
        let result = stmt.query_row([file_path], |row| row.get(0)).optional()?;
        Ok(result)
    }

    /// Store the hash for a file after successful indexing
    pub fn set_file_hash(&self, file_path: &str, hash: &str) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO file_hashes (file_path, hash) VALUES (?1, ?2)",
            [file_path, hash],
        )?;
        Ok(())
    }

    /// Store hashes for multiple files in a single transaction
    pub fn set_file_hashes_bulk(&self, entries: &[(&str, &str)]) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        {
            let mut stmt = tx.prepare(
                "INSERT OR REPLACE INTO file_hashes (file_path, hash) VALUES (?1, ?2)",
            )?;
            for (path, hash) in entries {
                stmt.execute([path, hash])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// Delete hash entries for removed files
    pub fn delete_file_hashes(&self, file_paths: &[String]) -> Result<()> {
        if file_paths.is_empty() {
            return Ok(());
        }
        let placeholders: Vec<String> = (1..=file_paths.len()).map(|i| format!("?{i}")).collect();
        let sql = format!(
            "DELETE FROM file_hashes WHERE file_path IN ({})",
            placeholders.join(", ")
        );
        let params: Vec<&dyn rusqlite::ToSql> = file_paths
            .iter()
            .map(|p| p as &dyn rusqlite::ToSql)
            .collect();
        self.conn.execute(&sql, params.as_slice())?;
        Ok(())
    }

    /// Get all file paths that have been indexed
    pub fn get_all_indexed_files(&self) -> Result<std::collections::HashSet<String>> {
        let mut stmt = self.conn.prepare("SELECT file_path FROM file_hashes")?;
        let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
        let mut result = std::collections::HashSet::new();
        for row in rows {
            result.insert(row?);
        }
        Ok(result)
    }

    /// Clear all file hashes (used by --force to rebuild from scratch)
    pub fn clear_file_hashes(&self) -> Result<()> {
        self.conn.execute("DELETE FROM file_hashes", [])?;
        Ok(())
    }

    /// Record a bead→commit lineage entry (GH#9 Layer 1 logging).
    ///
    /// Returns the row id of the inserted record. `touched_files` is stored as
    /// a JSON array so later layers can aggregate over changesets.
    pub fn record_bead_lineage(&self, rec: &NewBeadLineage) -> Result<i64> {
        let touched_files_json = serde_json::to_string(&rec.touched_files)
            .unwrap_or_else(|_| "[]".to_string());
        let touched_symbols_json = serde_json::to_string(&rec.touched_symbols)
            .unwrap_or_else(|_| "[]".to_string());
        self.conn.execute(
            r#"INSERT INTO bead_lineage
                   (bead_id, bead_type, commit_sha, bundle_slugs, touched_files, action_type,
                    feature_id, lines_added, lines_deleted, touched_symbols)
               VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)"#,
            rusqlite::params![
                rec.bead_id,
                rec.bead_type,
                rec.commit_sha,
                rec.bundle_slugs,
                touched_files_json,
                rec.action_type,
                rec.feature_id,
                rec.lines_added,
                rec.lines_deleted,
                touched_symbols_json,
            ],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    /// List bead lineage records, optionally filtered by bead id and/or commit.
    /// Most recent first.
    pub fn list_bead_lineage(
        &self,
        bead_id: Option<&str>,
        commit_sha: Option<&str>,
        limit: usize,
    ) -> Result<Vec<BeadLineageRecord>> {
        let mut sql = String::from(
            "SELECT id, created_at, bead_id, bead_type, commit_sha, bundle_slugs, touched_files, action_type,
                    feature_id, lines_added, lines_deleted, touched_symbols
             FROM bead_lineage",
        );
        let mut conditions: Vec<String> = Vec::new();
        let mut params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
        if let Some(b) = bead_id {
            conditions.push(format!("bead_id = ?{}", params.len() + 1));
            params.push(Box::new(b.to_string()));
        }
        if let Some(c) = commit_sha {
            conditions.push(format!("commit_sha = ?{}", params.len() + 1));
            params.push(Box::new(c.to_string()));
        }
        if !conditions.is_empty() {
            sql.push_str(" WHERE ");
            sql.push_str(&conditions.join(" AND "));
        }
        sql.push_str(&format!(" ORDER BY id DESC LIMIT ?{}", params.len() + 1));
        params.push(Box::new(limit as i64));

        let param_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt
            .query_map(param_refs.as_slice(), |row| {
                let touched_json: Option<String> = row.get(6)?;
                let touched_files = touched_json
                    .and_then(|j| serde_json::from_str::<Vec<String>>(&j).ok())
                    .unwrap_or_default();
                let symbols_json: Option<String> = row.get(11)?;
                let touched_symbols = symbols_json
                    .and_then(|j| serde_json::from_str::<Vec<TouchedSymbol>>(&j).ok())
                    .unwrap_or_default();
                Ok(BeadLineageRecord {
                    id: row.get(0)?,
                    created_at: row.get(1)?,
                    bead_id: row.get(2)?,
                    bead_type: row.get(3)?,
                    commit_sha: row.get(4)?,
                    bundle_slugs: row.get(5)?,
                    touched_files,
                    action_type: row.get(7)?,
                    feature_id: row.get(8)?,
                    lines_added: row.get(9)?,
                    lines_deleted: row.get(10)?,
                    touched_symbols,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }
}

/// A symbol touched by a commit's changeset (telemetry Phase 0, bo-xrsy).
/// Carries file attribution so the reconcile/predict loops (bo-mu4m/bo-6i55) can
/// map a bead to the named entities it changed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TouchedSymbol {
    pub file: String,
    pub symbol: String,
    /// Chunk kind (function | method | struct | ...), from the parser.
    pub kind: String,
}

/// Input for recording a bead lineage entry. `id`/`created_at` are assigned by
/// the database.
#[derive(Debug, Clone, Default)]
pub struct NewBeadLineage {
    pub bead_id: String,
    pub bead_type: Option<String>,
    pub commit_sha: Option<String>,
    pub bundle_slugs: Option<String>,
    pub touched_files: Vec<String>,
    pub action_type: Option<String>,
    /// Feature ancestor resolved via bd dep-walk (edge E1 'implements').
    pub feature_id: Option<String>,
    /// Aggregate lines added across the changeset (numstat).
    pub lines_added: Option<i64>,
    /// Aggregate lines deleted across the changeset (numstat).
    pub lines_deleted: Option<i64>,
    /// Named symbols touched by the changeset (best-effort parse).
    pub touched_symbols: Vec<TouchedSymbol>,
}

/// A stored bead→commit lineage record.
#[derive(Debug, Clone)]
pub struct BeadLineageRecord {
    pub id: i64,
    pub created_at: String,
    pub bead_id: String,
    pub bead_type: Option<String>,
    pub commit_sha: Option<String>,
    pub bundle_slugs: Option<String>,
    pub touched_files: Vec<String>,
    pub action_type: Option<String>,
    pub feature_id: Option<String>,
    pub lines_added: Option<i64>,
    pub lines_deleted: Option<i64>,
    pub touched_symbols: Vec<TouchedSymbol>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn create_test_store() -> (MetadataStore, tempfile::TempDir) {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("test.db");
        let store = MetadataStore::open(&db_path).unwrap();
        (store, dir)
    }

    #[test]
    fn test_open_creates_schema() {
        let (_store, _dir) = create_test_store();
        // Schema created without error
    }

    #[test]
    fn test_coupling() {
        let (store, _dir) = create_test_store();

        let coupling = FileCoupling {
            file_a: "src/a.rs".to_string(),
            file_b: "src/b.rs".to_string(),
            score: 0.85,
            co_changes: 10,
            last_co_change: 1234567890,
        };

        store.upsert_coupling(&coupling).unwrap();

        let retrieved = store.get_coupling("src/a.rs", 10).unwrap();
        assert_eq!(retrieved.len(), 1);
        assert_eq!(retrieved[0].score, 0.85);
        assert_eq!(retrieved[0].co_changes, 10);
    }

    #[test]
    fn test_coupling_update() {
        let (store, _dir) = create_test_store();

        let coupling = FileCoupling {
            file_a: "src/a.rs".to_string(),
            file_b: "src/b.rs".to_string(),
            score: 0.5,
            co_changes: 5,
            last_co_change: 1234567890,
        };
        store.upsert_coupling(&coupling).unwrap();

        // Update with higher score
        let updated = FileCoupling {
            file_a: "src/a.rs".to_string(),
            file_b: "src/b.rs".to_string(),
            score: 0.9,
            co_changes: 15,
            last_co_change: 9999999999,
        };
        store.upsert_coupling(&updated).unwrap();

        let retrieved = store.get_coupling("src/a.rs", 10).unwrap();
        assert_eq!(retrieved.len(), 1);
        assert_eq!(retrieved[0].score, 0.9);
        assert_eq!(retrieved[0].co_changes, 15);
    }

    #[test]
    fn test_meta() {
        let (store, _dir) = create_test_store();

        assert!(store.get_meta("model").unwrap().is_none());

        store.set_meta("model", "all-MiniLM-L6-v2").unwrap();
        assert_eq!(
            store.get_meta("model").unwrap(),
            Some("all-MiniLM-L6-v2".to_string())
        );

        store.set_meta("model", "bge-small-en-v1.5").unwrap();
        assert_eq!(
            store.get_meta("model").unwrap(),
            Some("bge-small-en-v1.5".to_string())
        );
    }

    #[test]
    fn test_clear_coupling() {
        let (store, _dir) = create_test_store();

        store
            .upsert_coupling(&FileCoupling {
                file_a: "a.rs".to_string(),
                file_b: "b.rs".to_string(),
                score: 0.5,
                co_changes: 3,
                last_co_change: 0,
            })
            .unwrap();

        assert_eq!(store.get_coupling("a.rs", 10).unwrap().len(), 1);
        store.clear_coupling().unwrap();
        assert_eq!(store.get_coupling("a.rs", 10).unwrap().len(), 0);
    }

    #[test]
    fn test_transaction() {
        let (store, _dir) = create_test_store();

        store.begin_transaction().unwrap();
        store
            .upsert_coupling(&FileCoupling {
                file_a: "a.rs".to_string(),
                file_b: "b.rs".to_string(),
                score: 0.5,
                co_changes: 3,
                last_co_change: 0,
            })
            .unwrap();
        store.commit().unwrap();

        assert_eq!(store.get_coupling("a.rs", 10).unwrap().len(), 1);
    }

    #[test]
    fn test_file_hash_roundtrip() {
        let (store, _dir) = create_test_store();

        assert!(store.get_file_hash("src/main.rs").unwrap().is_none());

        store.set_file_hash("src/main.rs", "abc123").unwrap();
        assert_eq!(
            store.get_file_hash("src/main.rs").unwrap(),
            Some("abc123".to_string())
        );

        // Update hash
        store.set_file_hash("src/main.rs", "def456").unwrap();
        assert_eq!(
            store.get_file_hash("src/main.rs").unwrap(),
            Some("def456".to_string())
        );
    }

    #[test]
    fn test_file_hashes_bulk() {
        let (store, _dir) = create_test_store();

        let entries = vec![
            ("src/a.rs", "hash_a"),
            ("src/b.rs", "hash_b"),
            ("src/c.rs", "hash_c"),
        ];
        store.set_file_hashes_bulk(&entries).unwrap();

        assert_eq!(store.get_file_hash("src/a.rs").unwrap(), Some("hash_a".to_string()));
        assert_eq!(store.get_file_hash("src/b.rs").unwrap(), Some("hash_b".to_string()));
        assert_eq!(store.get_file_hash("src/c.rs").unwrap(), Some("hash_c".to_string()));
    }

    #[test]
    fn test_delete_file_hashes() {
        let (store, _dir) = create_test_store();

        store.set_file_hash("src/a.rs", "hash_a").unwrap();
        store.set_file_hash("src/b.rs", "hash_b").unwrap();
        store.set_file_hash("src/c.rs", "hash_c").unwrap();

        store.delete_file_hashes(&["src/a.rs".to_string(), "src/c.rs".to_string()]).unwrap();

        assert!(store.get_file_hash("src/a.rs").unwrap().is_none());
        assert_eq!(store.get_file_hash("src/b.rs").unwrap(), Some("hash_b".to_string()));
        assert!(store.get_file_hash("src/c.rs").unwrap().is_none());
    }

    #[test]
    fn test_clear_file_hashes() {
        let (store, _dir) = create_test_store();

        store.set_file_hash("src/a.rs", "hash_a").unwrap();
        store.set_file_hash("src/b.rs", "hash_b").unwrap();

        store.clear_file_hashes().unwrap();

        assert!(store.get_file_hash("src/a.rs").unwrap().is_none());
        assert!(store.get_file_hash("src/b.rs").unwrap().is_none());
    }

    #[test]
    fn test_bead_lineage_record_and_list() {
        let (store, _dir) = create_test_store();

        store
            .record_bead_lineage(&NewBeadLineage {
                bead_id: "bo-abc".to_string(),
                bead_type: Some("bug".to_string()),
                commit_sha: Some("deadbeef".to_string()),
                bundle_slugs: Some("search-reranking".to_string()),
                touched_files: vec!["src/search/weights.rs".to_string(), "src/a.rs".to_string()],
                action_type: Some("linked".to_string()),
                feature_id: Some("bo-feat".to_string()),
                lines_added: Some(42),
                lines_deleted: Some(7),
                touched_symbols: vec![TouchedSymbol {
                    file: "src/search/weights.rs".to_string(),
                    symbol: "rerank".to_string(),
                    kind: "function".to_string(),
                }],
            })
            .unwrap();

        let all = store.list_bead_lineage(None, None, 10).unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].bead_id, "bo-abc");
        assert_eq!(all[0].commit_sha.as_deref(), Some("deadbeef"));
        assert_eq!(all[0].touched_files.len(), 2);
        assert!(all[0].touched_files.contains(&"src/a.rs".to_string()));
        // New telemetry Phase 0 fields round-trip through record -> list.
        assert_eq!(all[0].feature_id.as_deref(), Some("bo-feat"));
        assert_eq!(all[0].lines_added, Some(42));
        assert_eq!(all[0].lines_deleted, Some(7));
        assert_eq!(all[0].touched_symbols.len(), 1);
        assert_eq!(all[0].touched_symbols[0].symbol, "rerank");
        assert_eq!(all[0].touched_symbols[0].kind, "function");

        // Filter by bead id
        let by_bead = store.list_bead_lineage(Some("bo-abc"), None, 10).unwrap();
        assert_eq!(by_bead.len(), 1);
        assert!(store.list_bead_lineage(Some("bo-zzz"), None, 10).unwrap().is_empty());

        // Filter by commit
        let by_commit = store.list_bead_lineage(None, Some("deadbeef"), 10).unwrap();
        assert_eq!(by_commit.len(), 1);
    }

    #[test]
    fn test_bead_lineage_migration_idempotent() {
        // Opening the same DB twice must not error or duplicate columns: the
        // second open re-runs migrate_bead_lineage against an already-migrated
        // table.
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("migrate.db");
        {
            let _store = MetadataStore::open(&db_path).unwrap();
        }
        let store = MetadataStore::open(&db_path).unwrap();

        // The new columns exist exactly once.
        let cols: Vec<String> = {
            let mut stmt = store
                .conn
                .prepare("PRAGMA table_info(bead_lineage)")
                .unwrap();
            let rows = stmt
                .query_map([], |row| row.get::<_, String>(1))
                .unwrap()
                .collect::<Result<Vec<_>, _>>()
                .unwrap();
            rows
        };
        for expected in ["feature_id", "lines_added", "lines_deleted", "touched_symbols"] {
            assert_eq!(
                cols.iter().filter(|c| c.as_str() == expected).count(),
                1,
                "column {expected} should exist exactly once"
            );
        }
    }

    #[test]
    fn test_bead_lineage_migrates_legacy_table() {
        // A DB created with only the original columns (pre-bo-xrsy) must gain the
        // new columns on next open, and existing rows survive with NULL telemetry.
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("legacy.db");
        {
            let conn = Connection::open(&db_path).unwrap();
            conn.execute_batch(
                r#"CREATE TABLE bead_lineage (
                       id INTEGER PRIMARY KEY AUTOINCREMENT,
                       created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
                       bead_id TEXT NOT NULL,
                       bead_type TEXT,
                       commit_sha TEXT,
                       bundle_slugs TEXT,
                       touched_files TEXT,
                       action_type TEXT
                   );
                   INSERT INTO bead_lineage (bead_id, commit_sha) VALUES ('bo-old', 'cafe');"#,
            )
            .unwrap();
        }
        let store = MetadataStore::open(&db_path).unwrap();
        let rows = store.list_bead_lineage(Some("bo-old"), None, 10).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].feature_id, None);
        assert_eq!(rows[0].lines_added, None);
        assert!(rows[0].touched_symbols.is_empty());
    }

    #[test]
    fn test_bead_lineage_ordering_and_limit() {
        let (store, _dir) = create_test_store();
        for i in 0..5 {
            store
                .record_bead_lineage(&NewBeadLineage {
                    bead_id: format!("bo-{i}"),
                    commit_sha: Some(format!("sha{i}")),
                    ..Default::default()
                })
                .unwrap();
        }
        let recent = store.list_bead_lineage(None, None, 3).unwrap();
        assert_eq!(recent.len(), 3);
        // Most recent (highest id) first
        assert_eq!(recent[0].bead_id, "bo-4");
    }

    #[test]
    fn test_get_all_indexed_files() {
        let (store, _dir) = create_test_store();

        store.set_file_hash("src/a.rs", "hash_a").unwrap();
        store.set_file_hash("src/b.rs", "hash_b").unwrap();

        let files = store.get_all_indexed_files().unwrap();
        assert_eq!(files.len(), 2);
        assert!(files.contains("src/a.rs"));
        assert!(files.contains("src/b.rs"));
    }
}
