//! Per-repo file content hashes, for incremental indexing.
//!
//! Split out of `src/storage/sqlite/mod.rs` (bobbin-aoz). Rust lets one
//! type's inherent methods live in several `impl` blocks across modules, so
//! this is a real split rather than a re-export shim.

use anyhow::Result;
use rusqlite::OptionalExtension;

use super::MetadataStore;
use super::SQLITE_MAX_BIND_VARS;

impl MetadataStore {
    /// Get the stored hash for a file (for incremental indexing). Scoped to
    /// `repo`: the same relative path in another repo is a different file
    ///.
    pub fn get_file_hash(&self, repo: &str, file_path: &str) -> Result<Option<String>> {
        let mut stmt = self
            .conn
            .prepare("SELECT hash FROM file_hashes WHERE repo = ?1 AND file_path = ?2")?;
        let result = stmt
            .query_row([repo, file_path], |row| row.get(0))
            .optional()?;
        Ok(result)
    }

    /// Store the hash for a file after successful indexing
    pub fn set_file_hash(&self, repo: &str, file_path: &str, hash: &str) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO file_hashes (repo, file_path, hash) VALUES (?1, ?2, ?3)",
            [repo, file_path, hash],
        )?;
        Ok(())
    }

    /// Store hashes for multiple files in a single transaction
    pub fn set_file_hashes_bulk(&self, repo: &str, entries: &[(&str, &str)]) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        {
            let mut stmt = tx.prepare(
                "INSERT OR REPLACE INTO file_hashes (repo, file_path, hash) VALUES (?1, ?2, ?3)",
            )?;
            for (path, hash) in entries {
                stmt.execute([repo, path, hash])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// Delete hash entries for removed files.
    ///
    /// The deletes are chunked so a single statement never binds more than
    /// [`SQLITE_MAX_BIND_VARS`] parameters. An unbatched `IN (?1, …, ?N)` past
    /// SQLite's `SQLITE_MAX_VARIABLE_NUMBER` (32766) fails, aborting the prune
    /// and leaving the index inconsistent (bobbin #43). All chunks run in one
    /// transaction so the prune is atomic.
    /// `repo = None` deletes the paths in EVERY repo — for callers (the
    /// watcher's delete path) that cannot attribute a vanished path to one
    /// repo; over-deleting a hash row only costs a re-hash on the next run,
    /// while under-deleting risks a stale hash silently skipping a file.
    pub fn delete_file_hashes(&self, repo: Option<&str>, file_paths: &[String]) -> Result<()> {
        if file_paths.is_empty() {
            return Ok(());
        }
        let tx = self.conn.unchecked_transaction()?;
        for chunk in file_paths.chunks(SQLITE_MAX_BIND_VARS) {
            let placeholders: Vec<String> = (1..=chunk.len()).map(|i| format!("?{i}")).collect();
            let sql = match repo {
                Some(_) => format!(
                    "DELETE FROM file_hashes WHERE repo = ?{} AND file_path IN ({})",
                    chunk.len() + 1,
                    placeholders.join(", ")
                ),
                None => format!(
                    "DELETE FROM file_hashes WHERE file_path IN ({})",
                    placeholders.join(", ")
                ),
            };
            let mut params: Vec<&dyn rusqlite::ToSql> =
                chunk.iter().map(|p| p as &dyn rusqlite::ToSql).collect();
            if let Some(ref r) = repo {
                params.push(r as &dyn rusqlite::ToSql);
            }
            tx.execute(&sql, params.as_slice())?;
        }
        tx.commit()?;
        Ok(())
    }

    /// Get all file paths that have been indexed for one repo
    pub fn get_all_indexed_files(&self, repo: &str) -> Result<std::collections::HashSet<String>> {
        let mut stmt = self
            .conn
            .prepare("SELECT file_path FROM file_hashes WHERE repo = ?1")?;
        let rows = stmt.query_map([repo], |row| row.get::<_, String>(0))?;
        let mut result = std::collections::HashSet::new();
        for row in rows {
            result.insert(row?);
        }
        Ok(result)
    }

    /// Clear one repo's file hashes (used by --force to rebuild from scratch).
    /// Scoped so `--force` on repo A no longer wipes every other repo's
    /// incremental state with it.
    pub fn clear_file_hashes(&self, repo: &str) -> Result<()> {
        self.conn
            .execute("DELETE FROM file_hashes WHERE repo = ?1", [repo])?;
        Ok(())
    }
}
