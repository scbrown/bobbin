use anyhow::{Context, Result};
use rusqlite::{Connection, OptionalExtension};
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

            -- Indexes
            CREATE INDEX IF NOT EXISTS idx_coupling_score ON coupling(score DESC);
        "#,
        )?;

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
