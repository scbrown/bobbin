//! Bulk coupling reads and cross-repo coupling.
//!
//! Split out of `src/storage/sqlite/mod.rs` (bobbin-aoz). Rust lets one
//! type's inherent methods live in several `impl` blocks across modules, so
//! this is a real split rather than a re-export shim.

use anyhow::Result;

use super::MetadataStore;
use crate::types::FileCoupling;

impl MetadataStore {
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
            // rusqlite 0.40 dropped ToSql for usize; LIMIT fits i64.
            .query_map(rusqlite::params![min_score, limit as i64], |row| {
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
    pub fn upsert_cross_repo_coupling(&self, c: &crate::types::CrossRepoCoupling) -> Result<()> {
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
}
