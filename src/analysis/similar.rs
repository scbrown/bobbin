use std::collections::HashMap;

use anyhow::{bail, Context, Result};

use crate::analysis::similar_edges::{edge_candidate, SimilarEdgeCandidate};
use crate::index::Embedder;
use crate::storage::VectorStore;
use crate::types::{Chunk, SearchResult};

/// Result from a similarity search with explanation
#[derive(Debug, Clone)]
pub struct SimilarResult {
    pub chunk: Chunk,
    pub similarity: f32,
    pub explanation: String,
}

/// A cluster of semantically duplicate chunks
#[derive(Debug, Clone)]
pub struct DuplicateCluster {
    pub representative: Chunk,
    pub members: Vec<SimilarResult>,
    pub avg_similarity: f32,
}

/// What to search for similar code to
#[derive(Debug, Clone)]
pub enum SimilarTarget {
    /// A chunk reference in "file:name" syntax
    ChunkRef(String),
    /// Free-text query
    Text(String),
}

/// Finds chunks semantically similar to a given target
pub struct SimilarityAnalyzer {
    embedder: Embedder,
    vector_store: VectorStore,
}

impl SimilarityAnalyzer {
    pub fn new(embedder: Embedder, vector_store: VectorStore) -> Self {
        Self {
            embedder,
            vector_store,
        }
    }

    /// Find chunks similar to the given target
    ///
    /// - `threshold`: minimum cosine similarity (default 0.85)
    /// - `limit`: maximum results (default 10)
    /// - `repo`: optional repo filter
    pub async fn find_similar(
        &mut self,
        target: &SimilarTarget,
        threshold: f32,
        limit: usize,
        repo: Option<&str>,
    ) -> Result<Vec<SimilarResult>> {
        let (embedding, target_chunk_id) = match target {
            SimilarTarget::ChunkRef(ref_str) => {
                let (chunk, embedding) = self.resolve_chunk_ref(ref_str, repo).await?;
                (embedding, Some(chunk.id))
            }
            SimilarTarget::Text(text) => {
                let embedding = self
                    .embedder
                    .embed(text)
                    .await
                    .context("Failed to embed text query")?;
                (embedding, None)
            }
        };

        // Search with extra headroom for filtering
        let search_limit = limit + 1; // +1 to account for self-exclusion
        let results = self
            .vector_store
            .search(&embedding, search_limit, repo)
            .await
            .context("Failed to search for similar chunks")?;

        let mut similar_results = Vec::new();
        for result in results {
            // Exclude the target chunk itself
            if let Some(ref target_id) = target_chunk_id {
                if &result.chunk.id == target_id {
                    continue;
                }
            }

            // Filter by threshold
            if result.score < threshold {
                continue;
            }

            if similar_results.len() >= limit {
                break;
            }

            let explanation = build_explanation(&result);
            similar_results.push(SimilarResult {
                chunk: result.chunk,
                similarity: result.score,
                explanation,
            });
        }

        Ok(similar_results)
    }

    /// Scan all chunks for near-duplicate clusters
    ///
    /// - `threshold`: minimum cosine similarity to consider a pair duplicate (default 0.90)
    /// - `max_clusters`: maximum number of clusters to return (default 10)
    /// - `repo`: optional repo filter (only scan chunks in this repo)
    /// - `cross_repo`: if false, only compare chunks within the same repo
    pub async fn scan_duplicates(
        &self,
        threshold: f32,
        max_clusters: usize,
        repo: Option<&str>,
        cross_repo: bool,
    ) -> Result<Vec<DuplicateCluster>> {
        let (clusters, _) = scan_duplicates_impl(
            &self.vector_store,
            threshold,
            max_clusters,
            repo,
            cross_repo,
        )
        .await?;
        Ok(clusters)
    }

    /// Like [`scan_duplicates`](Self::scan_duplicates), but also returns the
    /// threshold-gated near-duplicate pairs as persistable edge candidates.
    /// Candidates cover every gated pair, not just the truncated cluster
    /// display.
    pub async fn scan_duplicates_with_edges(
        &self,
        threshold: f32,
        max_clusters: usize,
        repo: Option<&str>,
        cross_repo: bool,
    ) -> Result<(Vec<DuplicateCluster>, Vec<SimilarEdgeCandidate>)> {
        scan_duplicates_impl(
            &self.vector_store,
            threshold,
            max_clusters,
            repo,
            cross_repo,
        )
        .await
    }

    /// Replace the persisted `similar_to` edge set (within `repo_scope`)
    /// with the given candidates. See [`crate::analysis::similar_edges`].
    pub async fn persist_similar_edges(
        &mut self,
        candidates: &[SimilarEdgeCandidate],
        repo_scope: Option<&str>,
    ) -> Result<usize> {
        crate::analysis::similar_edges::persist_similar_edges(
            &mut self.vector_store,
            candidates,
            repo_scope,
        )
        .await
    }

    /// Resolve a "file:name" chunk reference to a chunk and its embedding
    async fn resolve_chunk_ref(
        &self,
        ref_str: &str,
        repo: Option<&str>,
    ) -> Result<(Chunk, Vec<f32>)> {
        let (file_path, chunk_name) = parse_chunk_ref(ref_str)?;

        let chunks = self
            .vector_store
            .get_chunks_for_file(&file_path, repo)
            .await
            .with_context(|| format!("Failed to get chunks for file: {}", file_path))?;

        if chunks.is_empty() {
            bail!("No chunks found for file: {}", file_path);
        }

        let chunk = chunks
            .into_iter()
            .find(|c| c.name.as_deref() == Some(chunk_name))
            .with_context(|| format!("Chunk '{}' not found in file '{}'", chunk_name, file_path))?;

        let embedding = self
            .vector_store
            .get_chunk_embedding(&chunk.id)
            .await
            .with_context(|| format!("Failed to get embedding for chunk: {}", chunk.id))?
            .with_context(|| format!("No embedding found for chunk: {}", chunk.id))?;

        Ok((chunk, embedding))
    }
}

/// Parse a "file:name" reference into (file_path, chunk_name)
fn parse_chunk_ref(ref_str: &str) -> Result<(&str, &str)> {
    let (file, name) = ref_str.rsplit_once(':').with_context(|| {
        format!(
            "Invalid chunk reference '{}': expected 'file:name' syntax",
            ref_str
        )
    })?;

    if file.is_empty() || name.is_empty() {
        bail!(
            "Invalid chunk reference '{}': both file and name must be non-empty",
            ref_str
        );
    }

    Ok((file, name))
}

/// Simple union-find (disjoint set) for clustering
struct UnionFind {
    parent: Vec<usize>,
    rank: Vec<usize>,
}

impl UnionFind {
    fn new(n: usize) -> Self {
        Self {
            parent: (0..n).collect(),
            rank: vec![0; n],
        }
    }

    fn find(&mut self, x: usize) -> usize {
        if self.parent[x] != x {
            self.parent[x] = self.find(self.parent[x]); // path compression
        }
        self.parent[x]
    }

    fn union(&mut self, x: usize, y: usize) {
        let rx = self.find(x);
        let ry = self.find(y);
        if rx == ry {
            return;
        }
        // union by rank
        match self.rank[rx].cmp(&self.rank[ry]) {
            std::cmp::Ordering::Less => self.parent[rx] = ry,
            std::cmp::Ordering::Greater => self.parent[ry] = rx,
            std::cmp::Ordering::Equal => {
                self.parent[ry] = rx;
                self.rank[rx] += 1;
            }
        }
    }
}

/// Build a brief explanation from a Chunk (for scan results where we don't have a SearchResult)
fn build_explanation_from_chunk(chunk: &Chunk) -> String {
    let type_str = chunk.chunk_type.to_string();
    match &chunk.name {
        Some(name) => format!("{} '{}' in {}", type_str, name, chunk.file_path),
        None => format!(
            "{} in {} (lines {}-{})",
            type_str, chunk.file_path, chunk.start_line, chunk.end_line
        ),
    }
}

/// Build a brief explanation for why a result is similar
fn build_explanation(result: &SearchResult) -> String {
    let chunk = &result.chunk;
    let type_str = chunk.chunk_type.to_string();

    match &chunk.name {
        Some(name) => format!("{} '{}' in {}", type_str, name, chunk.file_path),
        None => format!(
            "{} in {} (lines {}-{})",
            type_str, chunk.file_path, chunk.start_line, chunk.end_line
        ),
    }
}

/// Core scan logic, separated from SimilarityAnalyzer for testability
async fn scan_duplicates_impl(
    vector_store: &VectorStore,
    threshold: f32,
    max_clusters: usize,
    repo: Option<&str>,
    cross_repo: bool,
) -> Result<(Vec<DuplicateCluster>, Vec<SimilarEdgeCandidate>)> {
    // Step 1: Load all chunks with embeddings
    let all_chunks = vector_store
        .get_all_chunks_with_embeddings(repo)
        .await
        .context("Failed to load chunks for duplicate scan")?;

    if all_chunks.is_empty() {
        return Ok((vec![], vec![]));
    }

    // Build lookup: chunk_id -> index
    let id_to_idx: HashMap<String, usize> = all_chunks
        .iter()
        .enumerate()
        .map(|(i, (chunk, _, _))| (chunk.id.clone(), i))
        .collect();

    // Build repo lookup for cross_repo filtering
    let id_to_repo: HashMap<&str, &str> = all_chunks
        .iter()
        .map(|(chunk, _, repo_name)| (chunk.id.as_str(), repo_name.as_str()))
        .collect();

    // Step 2: Batched self-join - find duplicate pairs
    let search_k = 50; // Max neighbors to check per chunk
    let mut pairs: Vec<(usize, usize, f32)> = Vec::new();
    let mut edge_candidates: Vec<SimilarEdgeCandidate> = Vec::new();

    for (chunk, embedding, _repo_name) in &all_chunks {
        // When not cross_repo, filter search to same repo
        let search_repo = if cross_repo {
            repo
        } else {
            Some(id_to_repo[chunk.id.as_str()])
        };

        let results = vector_store
            .search(embedding, search_k, search_repo)
            .await
            .with_context(|| format!("Failed to search neighbors for chunk {}", chunk.id))?;

        for result in &results {
            if result.chunk.id == chunk.id {
                continue;
            }
            if result.score < threshold {
                continue;
            }
            // Deduplicate: only keep pair where A.id < B.id
            if chunk.id < result.chunk.id {
                if let (Some(&idx_a), Some(&idx_b)) =
                    (id_to_idx.get(&chunk.id), id_to_idx.get(&result.chunk.id))
                {
                    pairs.push((idx_a, idx_b, result.score));
                    edge_candidates.push(edge_candidate(
                        chunk,
                        id_to_repo[chunk.id.as_str()],
                        &result.chunk,
                        id_to_repo[result.chunk.id.as_str()],
                        result.score,
                    ));
                }
            }
        }
    }

    if pairs.is_empty() {
        return Ok((vec![], vec![]));
    }

    // Step 3: Union-find clustering
    let n = all_chunks.len();
    let mut uf = UnionFind::new(n);
    let mut pair_scores: HashMap<(usize, usize), f32> = HashMap::new();

    for &(a, b, score) in &pairs {
        uf.union(a, b);
        let key = (a.min(b), a.max(b));
        pair_scores.insert(key, score);
    }

    // Extract connected components
    let mut clusters_map: HashMap<usize, Vec<usize>> = HashMap::new();
    for i in 0..n {
        let root = uf.find(i);
        clusters_map.entry(root).or_default().push(i);
    }

    // Step 4: Build DuplicateCluster results (only clusters with 2+ members)
    let mut clusters: Vec<DuplicateCluster> = Vec::new();

    for (_root, member_idxs) in &clusters_map {
        if member_idxs.len() < 2 {
            continue;
        }

        // Collect pairwise similarities within this cluster
        let mut total_sim = 0.0f32;
        let mut sim_count = 0u32;
        for i in 0..member_idxs.len() {
            for j in (i + 1)..member_idxs.len() {
                let key = (
                    member_idxs[i].min(member_idxs[j]),
                    member_idxs[i].max(member_idxs[j]),
                );
                if let Some(&score) = pair_scores.get(&key) {
                    total_sim += score;
                    sim_count += 1;
                }
            }
        }
        let avg_similarity = if sim_count > 0 {
            total_sim / sim_count as f32
        } else {
            0.0
        };

        let rep_idx = member_idxs[0];
        let representative = all_chunks[rep_idx].0.clone();

        let members: Vec<SimilarResult> = member_idxs[1..]
            .iter()
            .map(|&idx| {
                let (chunk, _, _) = &all_chunks[idx];
                let key = (rep_idx.min(idx), rep_idx.max(idx));
                let similarity = pair_scores.get(&key).copied().unwrap_or(avg_similarity);
                SimilarResult {
                    chunk: chunk.clone(),
                    similarity,
                    explanation: build_explanation_from_chunk(chunk),
                }
            })
            .collect();

        clusters.push(DuplicateCluster {
            representative,
            members,
            avg_similarity,
        });
    }

    // Sort by cluster size (largest first), then by avg similarity
    clusters.sort_by(|a, b| {
        let size_cmp = (b.members.len() + 1).cmp(&(a.members.len() + 1));
        if size_cmp == std::cmp::Ordering::Equal {
            b.avg_similarity
                .partial_cmp(&a.avg_similarity)
                .unwrap_or(std::cmp::Ordering::Equal)
        } else {
            size_cmp
        }
    });

    clusters.truncate(max_clusters);
    Ok((clusters, edge_candidates))
}

#[cfg(test)]
#[path = "similar_tests.rs"]
mod tests;
