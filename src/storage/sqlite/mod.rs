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

            -- Cross-repo coupling (bo-oqny). Co-change inferred across repos via
            -- shared bead references; both sides carry their repo because paths
            -- are repo-relative and collide across repos. Additive: leaves the
            -- per-repo `coupling` table above untouched. Canonicalized so
            -- (repo_a, path_a) <= (repo_b, path_b).
            CREATE TABLE IF NOT EXISTS cross_repo_coupling (
                repo_a TEXT NOT NULL,
                path_a TEXT NOT NULL,
                repo_b TEXT NOT NULL,
                path_b TEXT NOT NULL,
                score REAL,
                co_changes INTEGER,
                last_co_change INTEGER,
                PRIMARY KEY (repo_a, path_a, repo_b, path_b)
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

            -- Bug causality (GH#9 telemetry Phase 0, bo-s1kb). The supervised
            -- signal for "risky change": reconstructs which prior commit most
            -- likely introduced the bug a later bead fixed, per file. One row per
            -- (bug, culprit_sha, file); UNIQUE makes the reconstruction job
            -- idempotent so periodic re-runs upsert rather than duplicate.
            CREATE TABLE IF NOT EXISTS bug_causality (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
                bug_id TEXT NOT NULL,
                culprit_sha TEXT,
                culprit_bead_id TEXT,
                file TEXT,
                confidence REAL,
                UNIQUE(bug_id, culprit_sha, file)
            );

            -- Indexes
            CREATE INDEX IF NOT EXISTS idx_coupling_score ON coupling(score DESC);
            CREATE INDEX IF NOT EXISTS idx_xrepo_coupling_a ON cross_repo_coupling(repo_a, path_a);
            CREATE INDEX IF NOT EXISTS idx_xrepo_coupling_b ON cross_repo_coupling(repo_b, path_b);
            CREATE INDEX IF NOT EXISTS idx_bead_lineage_bead ON bead_lineage(bead_id);
            CREATE INDEX IF NOT EXISTS idx_bead_lineage_commit ON bead_lineage(commit_sha);
            CREATE INDEX IF NOT EXISTS idx_bug_causality_bug ON bug_causality(bug_id);
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

    /// Get cross-repo coupling edges touching a seed file (bo-oqny).
    ///
    /// Matches the seed on either side of the canonical pair. When `repo` is
    /// `Some`, the seed must match that exact (repo, path); when `None` (caller
    /// cannot resolve the seed's repo, e.g. a single-repo CLI store), match on
    /// `path` alone. Ordered by score DESC. NOTE: this returns raw edges — the
    /// caller MUST apply `[access]` role filtering to the *other* side before
    /// surfacing results (see `crate::index::cross_repo::related_cross_repo`).
    pub fn get_cross_repo_coupling(
        &self,
        repo: Option<&str>,
        path: &str,
        limit: usize,
    ) -> Result<Vec<crate::types::CrossRepoCoupling>> {
        let map_row = |row: &rusqlite::Row| {
            Ok(crate::types::CrossRepoCoupling {
                repo_a: row.get(0)?,
                path_a: row.get(1)?,
                repo_b: row.get(2)?,
                path_b: row.get(3)?,
                score: row.get(4)?,
                co_changes: row.get(5)?,
                last_co_change: row.get(6)?,
            })
        };
        let results = match repo {
            Some(r) => {
                let mut stmt = self.conn.prepare(
                    r#"SELECT repo_a, path_a, repo_b, path_b, score, co_changes, last_co_change
                       FROM cross_repo_coupling
                       WHERE (repo_a = ?1 AND path_a = ?2) OR (repo_b = ?1 AND path_b = ?2)
                       ORDER BY score DESC
                       LIMIT ?3"#,
                )?;
                let rows = stmt.query_map(rusqlite::params![r, path, limit as i64], map_row)?;
                rows.collect::<Result<Vec<_>, _>>()?
            }
            None => {
                let mut stmt = self.conn.prepare(
                    r#"SELECT repo_a, path_a, repo_b, path_b, score, co_changes, last_co_change
                       FROM cross_repo_coupling
                       WHERE path_a = ?1 OR path_b = ?1
                       ORDER BY score DESC
                       LIMIT ?2"#,
                )?;
                let rows = stmt.query_map(rusqlite::params![path, limit as i64], map_row)?;
                rows.collect::<Result<Vec<_>, _>>()?
            }
        };
        Ok(results)
    }

    /// Upsert a cross-repo coupling edge (bo-oqny). Caller is responsible for
    /// canonical ordering of the pair (see `CrossRepoCoupling`).
    pub fn upsert_cross_repo_coupling(
        &self,
        c: &crate::types::CrossRepoCoupling,
    ) -> Result<()> {
        self.conn.execute(
            r#"INSERT INTO cross_repo_coupling
                   (repo_a, path_a, repo_b, path_b, score, co_changes, last_co_change)
               VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
               ON CONFLICT(repo_a, path_a, repo_b, path_b) DO UPDATE SET
                   score = excluded.score,
                   co_changes = excluded.co_changes,
                   last_co_change = excluded.last_co_change"#,
            (
                &c.repo_a,
                &c.path_a,
                &c.repo_b,
                &c.path_b,
                c.score,
                c.co_changes,
                c.last_co_change,
            ),
        )?;
        Ok(())
    }

    /// Clear all cross-repo coupling data (bo-oqny). Rebuilt wholesale by the
    /// compute pass, mirroring `clear_coupling`.
    pub fn clear_cross_repo_coupling(&self) -> Result<()> {
        self.conn.execute("DELETE FROM cross_repo_coupling", [])?;
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

    /// Delete hash entries for removed files.
    ///
    /// The deletes are chunked so a single statement never binds more than
    /// [`SQLITE_MAX_BIND_VARS`] parameters. An unbatched `IN (?1, …, ?N)` past
    /// SQLite's `SQLITE_MAX_VARIABLE_NUMBER` (32766) fails, aborting the prune
    /// and leaving the index inconsistent (bobbin #43). All chunks run in one
    /// transaction so the prune is atomic.
    pub fn delete_file_hashes(&self, file_paths: &[String]) -> Result<()> {
        if file_paths.is_empty() {
            return Ok(());
        }
        let tx = self.conn.unchecked_transaction()?;
        for chunk in file_paths.chunks(SQLITE_MAX_BIND_VARS) {
            let placeholders: Vec<String> = (1..=chunk.len()).map(|i| format!("?{i}")).collect();
            let sql = format!(
                "DELETE FROM file_hashes WHERE file_path IN ({})",
                placeholders.join(", ")
            );
            let params: Vec<&dyn rusqlite::ToSql> =
                chunk.iter().map(|p| p as &dyn rusqlite::ToSql).collect();
            tx.execute(&sql, params.as_slice())?;
        }
        tx.commit()?;
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

mod lineage;
#[cfg(test)]
mod tests;

pub use lineage::{
    BeadLineageRecord, BugCausalityRecord, NewBeadLineage, NewBugCausality, PriorTouch,
    TouchedSymbol,
};
