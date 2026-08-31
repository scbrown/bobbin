//! Persist `similar_to` chunk edges from the near-duplicate scan.
//!
//! `bobbin similar --scan` finds threshold-gated near-duplicate pairs and,
//! historically, threw them away after printing clusters. This module turns
//! those pairs into `ChunkEdgeType::SimilarTo` rows in the existing
//! `chunk_edges` Lance table (opt-in via `--persist`), so the semantic
//! neighbors become queryable through the same `chunk_neighbors` surface as
//! the structural edges.
//!
//! Lifecycle mirrors the structural edges with one difference: structural
//! edges are replaced per file on re-index, while `similar_to` edges are
//! global to a scan, so a persist run replaces the whole `similar_to` set
//! within its repo scope. Re-running the same scan is therefore idempotent —
//! it converges to the same rows instead of accumulating duplicates.

use std::collections::HashMap;

use anyhow::{Context, Result};

use crate::storage::VectorStore;
use crate::types::{Chunk, ChunkEdge, ChunkEdgeType};

/// A threshold-gated near-duplicate pair, ready for persistence.
#[derive(Debug, Clone)]
pub struct SimilarEdgeCandidate {
    pub edge: ChunkEdge,
    /// Cosine similarity at scan time (informational; the `chunk_edges`
    /// table stores edge presence, not weight — presence implies the score
    /// cleared the scan threshold).
    pub similarity: f32,
    /// Repo the edge is stored under (the source chunk's repo — the
    /// `chunk_edges` table keys every row by one repo).
    pub repo: String,
}

/// Build a candidate from one scan pair.
///
/// Direction carries no meaning for similarity, so it is normalized: the
/// lexically smaller chunk ID becomes the source. Re-running a scan then
/// produces byte-identical rows, which is what makes replace-on-persist
/// converge.
pub fn edge_candidate(
    a: &Chunk,
    a_repo: &str,
    b: &Chunk,
    b_repo: &str,
    similarity: f32,
) -> SimilarEdgeCandidate {
    let (src, src_repo, dst) = if a.id <= b.id {
        (a, a_repo, b)
    } else {
        (b, b_repo, a)
    };
    SimilarEdgeCandidate {
        edge: ChunkEdge {
            source_chunk: src.id.clone(),
            target_chunk: dst.id.clone(),
            source_name: src.name.clone().unwrap_or_default(),
            target_name: dst.name.clone().unwrap_or_default(),
            edge_type: ChunkEdgeType::SimilarTo,
            file_path: src.file_path.clone(),
        },
        similarity,
        repo: src_repo.to_string(),
    }
}

/// Replace the persisted `similar_to` edge set with `candidates`.
///
/// Clears existing `similar_to` edges within `repo_scope` first (None =
/// every repo), then writes the new set grouped by repo. Returns the number
/// of edges written.
pub async fn persist_similar_edges(
    store: &mut VectorStore,
    candidates: &[SimilarEdgeCandidate],
    repo_scope: Option<&str>,
) -> Result<usize> {
    store
        .clear_chunk_edges_by_type(ChunkEdgeType::SimilarTo, repo_scope)
        .await
        .context("Failed to clear previous similar_to edges")?;

    let mut by_repo: HashMap<&str, Vec<ChunkEdge>> = HashMap::new();
    for c in candidates {
        by_repo
            .entry(c.repo.as_str())
            .or_default()
            .push(c.edge.clone());
    }

    let mut written = 0;
    for (repo, edges) in by_repo {
        store
            .upsert_chunk_edges(&edges, repo)
            .await
            .with_context(|| format!("Failed to persist similar_to edges for repo '{repo}'"))?;
        written += edges.len();
    }
    Ok(written)
}

#[cfg(test)]
#[path = "similar_edges_tests.rs"]
mod tests;
