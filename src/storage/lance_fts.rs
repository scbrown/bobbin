//! FTS index lifecycle for [`VectorStore`]: detect an existing index, build a
//! missing one, force a rebuild. Split from `lance.rs` for the file-size ratchet
//! (aegis-mgpp28), and because this is the path whose cost is a whole-corpus
//! training job, so it deserves one place to read.

use std::sync::atomic::Ordering;

use anyhow::{Context, Result};
use lancedb::index::scalar::FtsIndexBuilder;
use lancedb::index::Index;
use lancedb::Table;

use super::VectorStore;

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
