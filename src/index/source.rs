//! The seam between "a thing that produces chunks" and the index pipeline.
//!
//! Five source integrations grew as bespoke blocks inside `cli/index.rs`
//! (files, git commits, beads, archives, PDFs-inside-files), each
//! re-implementing fetch/hash/delete/insert. This trait plus
//! [`index_hashed_source`] extracts the strongest of those patterns — the
//! beads content-hash incremental with removal sweep — so a new
//! non-filesystem source (SQL, logs, metrics) is one `ChunkSource` impl
//! and a config block, not another copy of the pipeline.
//!
//! The file source deliberately stays bespoke: it streams per-file, builds
//! contextual-embedding windows, and owns the deletion sweep against the
//! walk — a genuinely different lifecycle. Git commits use a watermark and
//! archives full-replace; retrofitting those two onto this seam is tracked
//! in the roadmap (W4.P1 follow-up).

use anyhow::Result;

use crate::index::Embedder;
use crate::storage::{MetadataStore, VectorStore};
use crate::types::Chunk;

/// A non-filesystem producer of chunks with stable per-item identity.
pub trait ChunkSource {
    /// Display name for progress output.
    fn name(&self) -> &str;

    /// Repo key that namespaces both the LanceDB rows and the hash
    /// bookkeeping — distinct from any source repo's name so the corpus
    /// is shared across index runs without touching repo incremental state.
    fn repo_key(&self) -> &str;

    /// Source label stamped on inserted rows (e.g. "beads", "sql").
    fn source_label(&self) -> &str;

    /// Fetch the CURRENT full corpus. Each chunk must carry a stable
    /// `id` and `file_path` (the per-item key hashes are stored under).
    fn fetch(&self) -> impl std::future::Future<Output = Result<Vec<Chunk>>> + Send;
}

/// Outcome of one incremental source-index pass, for the caller to report.
pub struct SourceIndexReport {
    /// Chunks (re)embedded and inserted this pass.
    pub indexed: usize,
    /// Chunks skipped because their content hash was unchanged.
    pub unchanged: usize,
    /// Previously indexed items that disappeared from the corpus.
    pub removed: usize,
}

/// Content-hash incremental indexing with a removal sweep — the beads
/// pattern, generalized:
///
/// 1. fetch the full current corpus;
/// 2. (re)embed only items whose content hash changed;
/// 3. delete items that vanished since the last pass;
/// 4. persist hashes only after a successful insert, so a failed run
///    retries rather than silently skipping.
pub async fn index_hashed_source<S: ChunkSource>(
    source: &S,
    vector_store: &mut VectorStore,
    metadata_store: &MetadataStore,
    embedder: &Embedder,
    force: bool,
) -> Result<SourceIndexReport> {
    let chunks = source.fetch().await?;
    let repo_key = source.repo_key();
    let now = chrono::Utc::now().timestamp().to_string();

    if force {
        metadata_store.clear_file_hashes(repo_key).ok();
    }

    let mut to_index = Vec::new();
    let mut current_paths: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut new_hashes: Vec<(String, String)> = Vec::new();
    for c in &chunks {
        current_paths.insert(c.file_path.clone());
        let h = content_hash(&c.content);
        let unchanged = metadata_store
            .get_file_hash(repo_key, &c.file_path)
            .ok()
            .flatten()
            .as_deref()
            == Some(h.as_str());
        if !unchanged {
            to_index.push(c.clone());
        }
        new_hashes.push((c.file_path.clone(), h));
    }

    let removed: Vec<String> = metadata_store
        .get_all_indexed_files(repo_key)
        .unwrap_or_default()
        .into_iter()
        .filter(|p| !current_paths.contains(p))
        .collect();
    if !removed.is_empty() {
        vector_store
            .delete_by_file(&removed, Some(repo_key))
            .await
            .ok();
        metadata_store
            .delete_file_hashes(Some(repo_key), &removed)
            .ok();
    }

    if to_index.is_empty() {
        return Ok(SourceIndexReport {
            indexed: 0,
            unchanged: chunks.len(),
            removed: removed.len(),
        });
    }

    let embed_texts: Vec<String> = to_index.iter().map(|c| c.content.clone()).collect();
    let embed_refs: Vec<&str> = embed_texts.iter().map(|s| s.as_str()).collect();
    let embeddings = embedder.embed_batch(&embed_refs).await?;
    let contexts = vec![None; to_index.len()];

    // Replace just the changed chunks.
    let ids: Vec<String> = to_index.iter().map(|c| c.id.clone()).collect();
    vector_store.delete(&ids).await.ok();
    vector_store
        .insert(
            &to_index,
            &embeddings,
            &contexts,
            repo_key,
            source.source_label(),
            &now,
        )
        .await?;

    // Persist hashes only after a successful insert.
    let hash_refs: Vec<(&str, &str)> = new_hashes
        .iter()
        .map(|(p, h)| (p.as_str(), h.as_str()))
        .collect();
    metadata_store
        .set_file_hashes_bulk(repo_key, &hash_refs)
        .ok();

    Ok(SourceIndexReport {
        indexed: to_index.len(),
        unchanged: chunks.len() - to_index.len(),
        removed: removed.len(),
    })
}

/// Stable content hash for change detection (shared by all hashed sources).
pub fn content_hash(content: &str) -> String {
    use sha2::{Digest, Sha256};
    hex::encode(Sha256::digest(content.as_bytes()))
}
