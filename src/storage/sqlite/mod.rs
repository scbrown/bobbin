use anyhow::{Context, Result};
use rusqlite::{Connection, OptionalExtension};
use std::path::Path;

use crate::types::FileCoupling;

/// Maximum bound parameters per statement. SQLite's compile-time default
/// `SQLITE_MAX_VARIABLE_NUMBER` is 32766; we stay comfortably under it so a
/// large `IN (…)` prune never aborts (bobbin #43).
const SQLITE_MAX_BIND_VARS: usize = 30_000;

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

    /// Roll back the active transaction (best-effort; used on a failed batch).
    pub fn rollback(&self) -> Result<()> {
        self.conn.execute("ROLLBACK", [])?;
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

    /// Delete a metadata key. Returns the number of rows removed (0 if absent).
    ///
    /// Used to reset a per-repo commit/coupling WATERMARK when that repo's
    /// chunks are bulk-deleted: a watermark that outlives its
    /// chunks makes the next incremental index ask `git log <watermark>..HEAD`
    /// and re-add only NEW commits, silently stranding the wiped history.
    /// Clearing the watermark forces the next index to rebuild from scratch
    /// (`since=None` → `git log` → all commits).
    pub fn delete_meta(&self, key: &str) -> Result<usize> {
        let n = self
            .conn
            .execute("DELETE FROM meta WHERE key = ?1", [key])?;
        Ok(n)
    }

    /// Get all metadata entries matching a key prefix (e.g., "repo_source:")
    pub fn get_meta_by_prefix(&self, prefix: &str) -> Result<Vec<(String, String)>> {
        let mut stmt = self
            .conn
            .prepare("SELECT key, value FROM meta WHERE key LIKE ?1")?;
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
}

mod coupling;
mod files;
mod lineage;
mod schema;
#[cfg(test)]
mod tests;

pub use lineage::{
    BeadLineageRecord, BugCausalityRecord, NewBeadLineage, NewBugCausality, PriorTouch,
    TouchedSymbol,
};
