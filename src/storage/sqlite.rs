use anyhow::{Context, Result};
use rusqlite::{Connection, OptionalExtension};
use std::path::Path;

use crate::types::{FileCoupling, ImportEdge};

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

            -- Import/dependency graph edges
            CREATE TABLE IF NOT EXISTS dependencies (
                source_file TEXT NOT NULL,
                import_specifier TEXT NOT NULL,
                resolved_path TEXT,
                language TEXT NOT NULL,
                PRIMARY KEY (source_file, import_specifier)
            );

            -- Indexes
            CREATE INDEX IF NOT EXISTS idx_coupling_score ON coupling(score DESC);
            CREATE INDEX IF NOT EXISTS idx_deps_source ON dependencies(source_file);
            CREATE INDEX IF NOT EXISTS idx_deps_resolved ON dependencies(resolved_path);
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

    /// Insert an import edge
    pub fn upsert_dependency(&self, edge: &ImportEdge) -> Result<()> {
        self.conn.execute(
            r#"INSERT INTO dependencies (source_file, import_specifier, resolved_path, language)
               VALUES (?1, ?2, ?3, ?4)
               ON CONFLICT(source_file, import_specifier) DO UPDATE SET
                   resolved_path = excluded.resolved_path,
                   language = excluded.language"#,
            (
                &edge.source_file,
                &edge.import_specifier,
                &edge.resolved_path,
                &edge.language,
            ),
        )?;
        Ok(())
    }

    /// Get imports from a file (what does this file depend on?)
    pub fn get_imports(&self, file_path: &str) -> Result<Vec<ImportEdge>> {
        let mut stmt = self.conn.prepare(
            r#"SELECT source_file, import_specifier, resolved_path, language
               FROM dependencies
               WHERE source_file = ?1
               ORDER BY import_specifier"#,
        )?;

        let results = stmt
            .query_map([file_path], |row| {
                Ok(ImportEdge {
                    source_file: row.get(0)?,
                    import_specifier: row.get(1)?,
                    resolved_path: row.get(2)?,
                    language: row.get(3)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(results)
    }

    /// Get reverse dependencies (what files import this file?)
    pub fn get_dependents(&self, file_path: &str) -> Result<Vec<ImportEdge>> {
        let mut stmt = self.conn.prepare(
            r#"SELECT source_file, import_specifier, resolved_path, language
               FROM dependencies
               WHERE resolved_path = ?1
               ORDER BY source_file"#,
        )?;

        let results = stmt
            .query_map([file_path], |row| {
                Ok(ImportEdge {
                    source_file: row.get(0)?,
                    import_specifier: row.get(1)?,
                    resolved_path: row.get(2)?,
                    language: row.get(3)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(results)
    }

    /// Clear all dependency data for a set of files
    pub fn clear_dependencies_for_files(&self, files: &[String]) -> Result<()> {
        if files.is_empty() {
            return Ok(());
        }
        let placeholders: Vec<String> = (1..=files.len()).map(|i| format!("?{}", i)).collect();
        let sql = format!(
            "DELETE FROM dependencies WHERE source_file IN ({})",
            placeholders.join(", ")
        );
        let params: Vec<&dyn rusqlite::types::ToSql> = files.iter().map(|f| f as &dyn rusqlite::types::ToSql).collect();
        self.conn.execute(&sql, params.as_slice())?;
        Ok(())
    }

    /// Get dependency statistics
    pub fn get_dependency_stats(&self) -> Result<(u64, u64)> {
        let total_edges: u64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM dependencies", [], |row| row.get(0))?;
        let resolved: u64 = self.conn.query_row(
            "SELECT COUNT(*) FROM dependencies WHERE resolved_path IS NOT NULL",
            [],
            |row| row.get(0),
        )?;
        Ok((total_edges, resolved))
    }

    /// Clear all coupling data
    pub fn clear_coupling(&self) -> Result<()> {
        self.conn.execute("DELETE FROM coupling", [])?;
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
    fn test_dependencies_insert_and_query() {
        let (store, _dir) = create_test_store();

        let edge = ImportEdge {
            source_file: "src/main.rs".to_string(),
            import_specifier: "crate::types::Chunk".to_string(),
            resolved_path: Some("src/types.rs".to_string()),
            language: "rust".to_string(),
        };
        store.upsert_dependency(&edge).unwrap();

        let imports = store.get_imports("src/main.rs").unwrap();
        assert_eq!(imports.len(), 1);
        assert_eq!(imports[0].import_specifier, "crate::types::Chunk");
        assert_eq!(imports[0].resolved_path, Some("src/types.rs".to_string()));
    }

    #[test]
    fn test_dependencies_reverse_lookup() {
        let (store, _dir) = create_test_store();

        store
            .upsert_dependency(&ImportEdge {
                source_file: "src/main.rs".to_string(),
                import_specifier: "crate::types".to_string(),
                resolved_path: Some("src/types.rs".to_string()),
                language: "rust".to_string(),
            })
            .unwrap();
        store
            .upsert_dependency(&ImportEdge {
                source_file: "src/cli/search.rs".to_string(),
                import_specifier: "crate::types::SearchResult".to_string(),
                resolved_path: Some("src/types.rs".to_string()),
                language: "rust".to_string(),
            })
            .unwrap();

        let dependents = store.get_dependents("src/types.rs").unwrap();
        assert_eq!(dependents.len(), 2);
    }

    #[test]
    fn test_dependencies_clear_for_files() {
        let (store, _dir) = create_test_store();

        store
            .upsert_dependency(&ImportEdge {
                source_file: "src/a.rs".to_string(),
                import_specifier: "crate::b".to_string(),
                resolved_path: Some("src/b.rs".to_string()),
                language: "rust".to_string(),
            })
            .unwrap();
        store
            .upsert_dependency(&ImportEdge {
                source_file: "src/c.rs".to_string(),
                import_specifier: "crate::b".to_string(),
                resolved_path: Some("src/b.rs".to_string()),
                language: "rust".to_string(),
            })
            .unwrap();

        // Clear only a.rs deps
        store
            .clear_dependencies_for_files(&["src/a.rs".to_string()])
            .unwrap();

        assert_eq!(store.get_imports("src/a.rs").unwrap().len(), 0);
        assert_eq!(store.get_imports("src/c.rs").unwrap().len(), 1);
    }

    #[test]
    fn test_dependency_stats() {
        let (store, _dir) = create_test_store();

        store
            .upsert_dependency(&ImportEdge {
                source_file: "src/a.rs".to_string(),
                import_specifier: "crate::b".to_string(),
                resolved_path: Some("src/b.rs".to_string()),
                language: "rust".to_string(),
            })
            .unwrap();
        store
            .upsert_dependency(&ImportEdge {
                source_file: "src/a.rs".to_string(),
                import_specifier: "anyhow::Result".to_string(),
                resolved_path: None,
                language: "rust".to_string(),
            })
            .unwrap();

        let (total, resolved) = store.get_dependency_stats().unwrap();
        assert_eq!(total, 2);
        assert_eq!(resolved, 1);
    }
}
