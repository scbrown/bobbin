//! FTS index lifecycle for [`VectorStore`]: detect an existing index, build a
//! missing one, force a rebuild. Split from `lance.rs` for the file-size ratchet
//! (aegis-mgpp28), and because this is the path whose cost is a whole-corpus
//! training job, so it deserves one place to read.

use std::sync::atomic::Ordering;

use anyhow::{Context, Result};
use lancedb::index::scalar::FtsIndexBuilder;
use lancedb::index::Index;
use lancedb::table::OptimizeAction;
use lancedb::Table;

use super::{is_commit_conflict, VectorStore};

impl VectorStore {
    /// Ensure FTS index exists on the content column.
    ///
    /// LanceDB 0.17 does not support multi-column (composite) FTS indexes,
    /// so we index only the `content` column which contains the actual code text.
    ///
    /// ASK FIRST, BUILD ONLY WHEN ABSENT. `create_index(..).replace(false)` is not a
    /// cheap probe: Lance trains the whole-corpus inverted index BEFORE it
    /// discovers one already exists, then fails the commit and discards the work.
    /// Measured on the production server (aegis-mgpp28): `Starting index training
    /// job ... Training index 0/204607` inside ordinary searches, which together
    /// with a fresh `VectorStore` per request (so `fts_indexed` never persists)
    /// drove memory bursts past the unit's MemoryHigh and hung the service. So an
    /// existing index is detected with `list_indices`, and only a genuinely
    /// missing one is built. If listing fails, fall through to the build path:
    /// that is the pre-fix behaviour, never worse.
    pub async fn ensure_fts_index(&self) -> Result<()> {
        if self.fts_indexed.load(Ordering::Relaxed) {
            return Ok(());
        }

        let table = match &self.table {
            Some(t) => t,
            None => return Ok(()),
        };

        if Self::has_content_fts_index(table).await {
            self.fts_indexed.store(true, Ordering::Relaxed);
            return Ok(());
        }

        // No FTS index on `content`: build one. An error here most likely means a
        // concurrent builder won the race; the existing index is then usable.
        crate::operational_metrics::record_fts_build_attempt();
        self.fts_build_attempts.fetch_add(1, Ordering::Relaxed);
        let result = table
            .create_index(&["content"], Index::FTS(FtsIndexBuilder::default()))
            .replace(false)
            .execute()
            .await;

        match result {
            Ok(()) => {}
            Err(_) => {
                // Index likely already exists. Verify by trying replace=true
                // only if the error isn't "index already exists".
                // For now, assume existing index is usable.
            }
        }

        self.fts_indexed.store(true, Ordering::Relaxed);
        Ok(())
    }

    /// Whole-corpus FTS builds this store has requested (tests read it; production
    /// reads the global `bobbin_fts_build_attempts_total`).
    #[cfg(test)]
    pub(crate) fn fts_build_attempts(&self) -> u64 {
        self.fts_build_attempts.load(Ordering::Relaxed)
    }

    /// Whether an FTS (inverted) index already covers the `content` column.
    /// Reads the table manifest only; trains nothing. `false` on a listing error,
    /// which sends the caller down the pre-fix build path.
    pub(super) async fn has_content_fts_index(table: &Table) -> bool {
        table
            .list_indices()
            .await
            .map(|indices| {
                indices.iter().any(|index| {
                    index.index_type == lancedb::index::IndexType::FTS
                        && index.columns.iter().any(|c| c == "content")
                })
            })
            .unwrap_or(false)
    }

    /// Rows the `content` FTS index covers, and rows it does not, as
    /// `(indexed, unindexed)`. Lance answers an FTS query for unindexed rows by
    /// scanning their text on every query, so the second number is a per-query
    /// cost (aegis-4h7zw8). `None` when there is no content FTS index or the
    /// stats cannot be read. Reads index metadata only; trains nothing.
    pub async fn fts_coverage(&self) -> Option<(usize, usize)> {
        let table = self.table.as_ref()?;
        let indices = table.list_indices().await.ok()?;
        let index = indices.iter().find(|index| {
            index.index_type == lancedb::index::IndexType::FTS
                && index.columns.iter().any(|c| c == "content")
        })?;
        let stats = table.index_stats(&index.name).await.ok()??;
        Some((stats.num_indexed_rows, stats.num_unindexed_rows))
    }

    /// Force-(re)build the FTS index over the `content` column, replacing any
    /// existing index. Used to self-heal a missing/stale index.
    pub async fn rebuild_fts_index(&self) -> Result<()> {
        let table = match &self.table {
            Some(t) => t,
            None => return Ok(()),
        };
        crate::operational_metrics::record_fts_build_attempt();
        self.fts_build_attempts.fetch_add(1, Ordering::Relaxed);
        table
            .create_index(&["content"], Index::FTS(FtsIndexBuilder::default()))
            .replace(true)
            .execute()
            .await
            .context("Failed to (re)build FTS index")?;
        crate::operational_metrics::record_fts_rebuild();
        self.fts_indexed.store(true, Ordering::Relaxed);
        Ok(())
    }
}

impl VectorStore {
    /// Add unindexed rows to every table's existing indices, without
    /// retraining them. CALLER MUST HOLD the maintenance lock.
    ///
    /// ALL indices of every maintained table, not only the content FTS index
    /// (wu, bobbin#164): today only `chunks` carries one (the FTS index; there
    /// is no ANN index), but a future index gets the same treatment rather
    /// than silently going stale the same way.
    pub(super) async fn optimize_indices_locked(
        &self,
        tables: &[(&'static str, &Table)],
    ) -> Result<()> {
        let mut first_err = None;
        for (name, table) in tables {
            let mut r = retry_on_conflict!(
                table,
                table.optimize(OptimizeAction::Index(
                    lancedb::table::OptimizeOptions::default()
                ))
            )
            .with_context(|| format!("Failed to optimize indices of {name} table"));
            // Optimizing the FTS index runs the same inverted-index builder
            // whose incremental path can panic during compaction (see
            // compact_locked). Same recovery: a full replacement build. It
            // indexes every row, so it also achieves what the optimize was for.
            if should_rebuild_fts_after(name, &r) {
                tracing::warn!(
                    error = %r.as_ref().expect_err("checked above"),
                    "FTS incremental optimize panicked; rebuilding the FTS index instead"
                );
                r = self
                    .rebuild_fts_index()
                    .await
                    .map(|()| lancedb::table::OptimizeStats::default())
                    .context("Failed to rebuild FTS index after incremental optimize panic");
            }
            if let Err(e) = r {
                first_err.get_or_insert(e);
            }
        }
        first_err.map_or(Ok(()), Err)
    }
}

/// Whether a failed chunks-table maintenance step should fall back to a full
/// FTS rebuild: only the inverted-builder panic, only on `chunks`, never an
/// unrelated error (I/O, schema, OOM), which a rebuild would not fix.
pub(super) fn should_rebuild_fts_after<T>(table: &str, r: &Result<T>) -> bool {
    table == "chunks"
        && r.as_ref()
            .err()
            .is_some_and(|e| super::is_fts_compaction_panic(e))
}
