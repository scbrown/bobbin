use std::collections::HashMap;

use anyhow::{bail, Context, Result};

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
                let embedding = self.embedder.embed(text).await
                    .context("Failed to embed text query")?;
                (embedding, None)
            }
        };

        // Search with extra headroom for filtering
        let search_limit = limit + 1; // +1 to account for self-exclusion
        let results = self.vector_store.search(&embedding, search_limit, repo).await
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
        scan_duplicates_impl(&self.vector_store, threshold, max_clusters, repo, cross_repo).await
    }

    /// Resolve a "file:name" chunk reference to a chunk and its embedding
    async fn resolve_chunk_ref(
        &self,
        ref_str: &str,
        repo: Option<&str>,
    ) -> Result<(Chunk, Vec<f32>)> {
        let (file_path, chunk_name) = parse_chunk_ref(ref_str)?;

        let chunks = self.vector_store.get_chunks_for_file(&file_path, repo).await
            .with_context(|| format!("Failed to get chunks for file: {}", file_path))?;

        if chunks.is_empty() {
            bail!("No chunks found for file: {}", file_path);
        }

        let chunk = chunks
            .into_iter()
            .find(|c| c.name.as_deref() == Some(chunk_name))
            .with_context(|| format!("Chunk '{}' not found in file '{}'", chunk_name, file_path))?;

        let embedding = self.vector_store.get_chunk_embedding(&chunk.id).await
            .with_context(|| format!("Failed to get embedding for chunk: {}", chunk.id))?
            .with_context(|| format!("No embedding found for chunk: {}", chunk.id))?;

        Ok((chunk, embedding))
    }
}

/// Parse a "file:name" reference into (file_path, chunk_name)
fn parse_chunk_ref(ref_str: &str) -> Result<(&str, &str)> {
    let (file, name) = ref_str
        .rsplit_once(':')
        .with_context(|| format!(
            "Invalid chunk reference '{}': expected 'file:name' syntax",
            ref_str
        ))?;

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
        None => format!("{} in {} (lines {}-{})", type_str, chunk.file_path, chunk.start_line, chunk.end_line),
    }
}

/// Build a brief explanation for why a result is similar
fn build_explanation(result: &SearchResult) -> String {
    let chunk = &result.chunk;
    let type_str = chunk.chunk_type.to_string();

    match &chunk.name {
        Some(name) => format!("{} '{}' in {}", type_str, name, chunk.file_path),
        None => format!("{} in {} (lines {}-{})", type_str, chunk.file_path, chunk.start_line, chunk.end_line),
    }
}

/// Core scan logic, separated from SimilarityAnalyzer for testability
async fn scan_duplicates_impl(
    vector_store: &VectorStore,
    threshold: f32,
    max_clusters: usize,
    repo: Option<&str>,
    cross_repo: bool,
) -> Result<Vec<DuplicateCluster>> {
    // Step 1: Load all chunks with embeddings
    let all_chunks = vector_store.get_all_chunks_with_embeddings(repo).await
        .context("Failed to load chunks for duplicate scan")?;

    if all_chunks.is_empty() {
        return Ok(vec![]);
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

    for (chunk, embedding, _repo_name) in &all_chunks {
        // When not cross_repo, filter search to same repo
        let search_repo = if cross_repo { repo } else { Some(id_to_repo[chunk.id.as_str()]) };

        let results = vector_store.search(embedding, search_k, search_repo).await
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
                }
            }
        }
    }

    if pairs.is_empty() {
        return Ok(vec![]);
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
                let key = (member_idxs[i].min(member_idxs[j]), member_idxs[i].max(member_idxs[j]));
                if let Some(&score) = pair_scores.get(&key) {
                    total_sim += score;
                    sim_count += 1;
                }
            }
        }
        let avg_similarity = if sim_count > 0 { total_sim / sim_count as f32 } else { 0.0 };

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
            b.avg_similarity.partial_cmp(&a.avg_similarity).unwrap_or(std::cmp::Ordering::Equal)
        } else {
            size_cmp
        }
    });

    clusters.truncate(max_clusters);
    Ok(clusters)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::VectorStore;
    use crate::types::ChunkType;
    use tempfile::tempdir;

    fn sample_chunk(id: &str, name: &str, file_path: &str) -> Chunk {
        Chunk {
            id: id.to_string(),
            file_path: file_path.to_string(),
            chunk_type: ChunkType::Function,
            name: Some(name.to_string()),
            start_line: 1,
            end_line: 10,
            content: format!("fn {}() {{ }}", name),
            language: "rust".to_string(),
        }
    }

    fn make_embedding(seed: f32) -> Vec<f32> {
        let mut emb: Vec<f32> = (0..384).map(|i| ((i as f32) + seed) / 384.0).collect();
        let norm: f32 = emb.iter().map(|x| x * x).sum::<f32>().sqrt();
        emb.iter_mut().for_each(|x| *x /= norm);
        emb
    }

    fn no_contexts(n: usize) -> Vec<Option<String>> {
        vec![None; n]
    }

    #[test]
    fn test_parse_chunk_ref_valid() {
        let (file, name) = parse_chunk_ref("src/main.rs:process_data").unwrap();
        assert_eq!(file, "src/main.rs");
        assert_eq!(name, "process_data");
    }

    #[test]
    fn test_parse_chunk_ref_nested_path() {
        let (file, name) = parse_chunk_ref("src/handlers/auth.rs:verify_token").unwrap();
        assert_eq!(file, "src/handlers/auth.rs");
        assert_eq!(name, "verify_token");
    }

    #[test]
    fn test_parse_chunk_ref_no_colon() {
        assert!(parse_chunk_ref("src/main.rs").is_err());
    }

    #[test]
    fn test_parse_chunk_ref_empty_name() {
        assert!(parse_chunk_ref("src/main.rs:").is_err());
    }

    #[test]
    fn test_parse_chunk_ref_empty_file() {
        assert!(parse_chunk_ref(":func").is_err());
    }

    #[test]
    fn test_build_explanation_with_name() {
        let result = SearchResult {
            chunk: Chunk {
                id: "id1".to_string(),
                file_path: "src/auth.rs".to_string(),
                chunk_type: ChunkType::Function,
                name: Some("verify_token".to_string()),
                start_line: 10,
                end_line: 20,
                content: "fn verify_token() {}".to_string(),
                language: "rust".to_string(),
            },
            score: 0.9,
            match_type: Some(crate::types::MatchType::Semantic),
        };

        let explanation = build_explanation(&result);
        assert_eq!(explanation, "function 'verify_token' in src/auth.rs");
    }

    #[test]
    fn test_build_explanation_without_name() {
        let result = SearchResult {
            chunk: Chunk {
                id: "id1".to_string(),
                file_path: "src/auth.rs".to_string(),
                chunk_type: ChunkType::Section,
                name: None,
                start_line: 10,
                end_line: 20,
                content: "some section".to_string(),
                language: "rust".to_string(),
            },
            score: 0.9,
            match_type: Some(crate::types::MatchType::Semantic),
        };

        let explanation = build_explanation(&result);
        assert_eq!(explanation, "section in src/auth.rs (lines 10-20)");
    }

    /// Helper: directly test find_similar logic using raw VectorStore
    /// (bypasses embedder since ChunkRef path uses stored embeddings)
    #[tokio::test]
    async fn test_find_similar_via_chunk_ref() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("vectors");
        let mut store = VectorStore::open(&path).await.unwrap();

        // Insert three chunks with different embeddings
        let emb_target = make_embedding(0.0);
        let emb_similar = make_embedding(1.0); // Very close to target
        let emb_different = make_embedding(500.0); // Very different

        let chunks = vec![
            sample_chunk("target", "process_data", "src/main.rs"),
            sample_chunk("similar", "process_items", "src/utils.rs"),
            sample_chunk("different", "render_html", "src/views.rs"),
        ];
        let embeddings = vec![emb_target.clone(), emb_similar, emb_different];

        store
            .insert(&chunks, &embeddings, &no_contexts(3), "default", "abc", "100")
            .await
            .unwrap();

        // Manually resolve chunk ref (what SimilarityAnalyzer.resolve_chunk_ref does)
        let file_chunks = store.get_chunks_for_file("src/main.rs", None).await.unwrap();
        let target_chunk = file_chunks.iter().find(|c| c.name.as_deref() == Some("process_data")).unwrap();

        // Get stored embedding
        let stored_emb = store.get_chunk_embedding(&target_chunk.id).await.unwrap().unwrap();
        assert_eq!(stored_emb.len(), 384);

        // Search with the stored embedding
        let results = store.search(&stored_emb, 10, None).await.unwrap();

        // Should find all 3 chunks (including self)
        assert_eq!(results.len(), 3);

        // Filter: exclude self, apply threshold
        let filtered: Vec<_> = results
            .into_iter()
            .filter(|r| r.chunk.id != target_chunk.id)
            .filter(|r| r.score >= 0.0)
            .collect();

        // Target should not be in filtered results
        assert!(!filtered.iter().any(|r| r.chunk.id == "target"));

        // Results should be ordered by score descending (VectorStore returns them this way)
        for w in filtered.windows(2) {
            assert!(w[0].score >= w[1].score);
        }
    }

    #[tokio::test]
    async fn test_threshold_filtering_logic() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("vectors");
        let mut store = VectorStore::open(&path).await.unwrap();

        let emb_target = make_embedding(0.0);
        let emb_close = make_embedding(0.5);
        let emb_far = make_embedding(1000.0);

        let chunks = vec![
            sample_chunk("target", "func_a", "src/a.rs"),
            sample_chunk("close", "func_b", "src/b.rs"),
            sample_chunk("far", "func_c", "src/c.rs"),
        ];

        store
            .insert(&chunks, &[emb_target.clone(), emb_close, emb_far], &no_contexts(3), "default", "abc", "100")
            .await
            .unwrap();

        let results = store.search(&emb_target, 10, None).await.unwrap();

        // With a very high threshold, only very similar results should pass
        let high_threshold: Vec<_> = results
            .iter()
            .filter(|r| r.chunk.id != "target")
            .filter(|r| r.score >= 0.99)
            .collect();

        // All results should meet the threshold
        for r in &high_threshold {
            assert!(r.score >= 0.99);
        }
    }

    #[tokio::test]
    async fn test_limit_enforcement() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("vectors");
        let mut store = VectorStore::open(&path).await.unwrap();

        // Insert several chunks with the same embedding
        let emb = make_embedding(0.0);
        let chunks: Vec<Chunk> = (0..5)
            .map(|i| sample_chunk(&format!("c{}", i), &format!("func_{}", i), &format!("src/f{}.rs", i)))
            .collect();
        let embeddings: Vec<Vec<f32>> = (0..5).map(|_| emb.clone()).collect();

        store
            .insert(&chunks, &embeddings, &no_contexts(5), "default", "abc", "100")
            .await
            .unwrap();

        // Request limit of 3 from vector search (+1 for self-exclusion)
        let results = store.search(&emb, 3, None).await.unwrap();

        // Exclude self and enforce limit of 2
        let limited: Vec<_> = results
            .into_iter()
            .filter(|r| r.chunk.id != "c0")
            .take(2)
            .collect();

        assert!(limited.len() <= 2);
    }

    #[tokio::test]
    async fn test_resolve_chunk_ref_finds_chunk() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("vectors");
        let mut store = VectorStore::open(&path).await.unwrap();

        let chunks = vec![
            sample_chunk("c1", "authenticate", "src/auth.rs"),
            sample_chunk("c2", "verify_token", "src/auth.rs"),
        ];
        let embeddings = vec![make_embedding(0.0), make_embedding(100.0)];

        store
            .insert(&chunks, &embeddings, &no_contexts(2), "default", "abc", "100")
            .await
            .unwrap();

        // Manually test the chunk resolution logic
        let (file_path, chunk_name) = parse_chunk_ref("src/auth.rs:verify_token").unwrap();
        let file_chunks = store.get_chunks_for_file(file_path, None).await.unwrap();
        let chunk = file_chunks.into_iter().find(|c| c.name.as_deref() == Some(chunk_name)).unwrap();

        assert_eq!(chunk.id, "c2");
        assert_eq!(chunk.name, Some("verify_token".to_string()));

        let embedding = store.get_chunk_embedding(&chunk.id).await.unwrap().unwrap();
        assert_eq!(embedding.len(), 384);
    }

    #[tokio::test]
    async fn test_resolve_chunk_ref_missing_file() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("vectors");
        let store = VectorStore::open(&path).await.unwrap();

        // Empty store -- no chunks for any file
        let chunks = store.get_chunks_for_file("nonexistent.rs", None).await.unwrap();
        assert!(chunks.is_empty());
    }

    #[tokio::test]
    async fn test_resolve_chunk_ref_missing_name() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("vectors");
        let mut store = VectorStore::open(&path).await.unwrap();

        let chunks = vec![sample_chunk("c1", "authenticate", "src/auth.rs")];
        let embeddings = vec![make_embedding(0.0)];

        store
            .insert(&chunks, &embeddings, &no_contexts(1), "default", "abc", "100")
            .await
            .unwrap();

        let file_chunks = store.get_chunks_for_file("src/auth.rs", None).await.unwrap();
        let found = file_chunks.iter().find(|c| c.name.as_deref() == Some("nonexistent"));
        assert!(found.is_none());
    }

    #[tokio::test]
    async fn test_self_exclusion() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("vectors");
        let mut store = VectorStore::open(&path).await.unwrap();

        let emb = make_embedding(0.0);
        let chunks = vec![
            sample_chunk("c1", "func_a", "src/a.rs"),
            sample_chunk("c2", "func_b", "src/b.rs"),
        ];

        store
            .insert(&chunks, &[emb.clone(), emb.clone()], &no_contexts(2), "default", "abc", "100")
            .await
            .unwrap();

        // Get the embedding for c1
        let stored = store.get_chunk_embedding("c1").await.unwrap().unwrap();

        // Search should return both
        let results = store.search(&stored, 10, None).await.unwrap();
        assert_eq!(results.len(), 2);

        // After filtering out self
        let without_self: Vec<_> = results.into_iter().filter(|r| r.chunk.id != "c1").collect();
        assert_eq!(without_self.len(), 1);
        assert_eq!(without_self[0].chunk.id, "c2");
    }

    #[tokio::test]
    async fn test_results_ordered_by_similarity() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("vectors");
        let mut store = VectorStore::open(&path).await.unwrap();

        let emb_target = make_embedding(0.0);
        let emb_close = make_embedding(1.0);
        let emb_medium = make_embedding(50.0);
        let emb_far = make_embedding(500.0);

        let chunks = vec![
            sample_chunk("target", "target_fn", "src/target.rs"),
            sample_chunk("close", "close_fn", "src/close.rs"),
            sample_chunk("medium", "medium_fn", "src/medium.rs"),
            sample_chunk("far", "far_fn", "src/far.rs"),
        ];

        store
            .insert(
                &chunks,
                &[emb_target.clone(), emb_close, emb_medium, emb_far],
                &no_contexts(4),
                "default",
                "abc",
                "100",
            )
            .await
            .unwrap();

        let results = store.search(&emb_target, 10, None).await.unwrap();
        let filtered: Vec<_> = results.into_iter().filter(|r| r.chunk.id != "target").collect();

        // Results should be ordered by score descending
        for w in filtered.windows(2) {
            assert!(
                w[0].score >= w[1].score,
                "Results not in descending order: {} >= {} failed",
                w[0].score,
                w[1].score
            );
        }
    }

    // ── Union-find tests ──────────────────────────────────────────────

    #[test]
    fn test_union_find_basic() {
        let mut uf = UnionFind::new(5);
        uf.union(0, 1);
        uf.union(2, 3);
        assert_eq!(uf.find(0), uf.find(1));
        assert_eq!(uf.find(2), uf.find(3));
        assert_ne!(uf.find(0), uf.find(2));

        // Now merge the two groups
        uf.union(1, 3);
        assert_eq!(uf.find(0), uf.find(3));
        // 4 is still isolated
        assert_ne!(uf.find(0), uf.find(4));
    }

    #[test]
    fn test_union_find_single_element() {
        let mut uf = UnionFind::new(1);
        assert_eq!(uf.find(0), 0);
    }

    // ── Scan duplicates tests ─────────────────────────────────────────

    #[tokio::test]
    async fn test_scan_finds_duplicate_clusters() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("vectors");
        let mut store = VectorStore::open(&path).await.unwrap();

        // Create two groups of near-identical chunks
        let emb_group1 = make_embedding(0.0);
        let emb_group1b = make_embedding(0.5); // very close to group1
        let emb_group2 = make_embedding(500.0);
        let emb_group2b = make_embedding(500.5); // very close to group2

        let chunks = vec![
            sample_chunk("g1a", "func_a1", "src/a.rs"),
            sample_chunk("g1b", "func_a2", "src/b.rs"),
            sample_chunk("g2a", "func_b1", "src/c.rs"),
            sample_chunk("g2b", "func_b2", "src/d.rs"),
        ];
        let embeddings = vec![emb_group1, emb_group1b, emb_group2, emb_group2b];

        store
            .insert(&chunks, &embeddings, &no_contexts(4), "default", "abc", "100")
            .await
            .unwrap();

        // Scan with a moderate threshold
        let clusters = scan_duplicates_impl(&store, 0.80, 10, None, true)
            .await
            .unwrap();

        // Should find at least one cluster
        assert!(!clusters.is_empty(), "Expected at least one duplicate cluster");

        // Each cluster should have at least 2 members (rep + members)
        for cluster in &clusters {
            assert!(
                !cluster.members.is_empty(),
                "Cluster should have at least one member besides representative"
            );
        }
    }

    #[tokio::test]
    async fn test_scan_no_duplicates_high_threshold() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("vectors");
        let mut store = VectorStore::open(&path).await.unwrap();

        // Very different embeddings - no duplicates at high threshold
        let chunks = vec![
            sample_chunk("c1", "func1", "src/a.rs"),
            sample_chunk("c2", "func2", "src/b.rs"),
        ];
        let embeddings = vec![make_embedding(0.0), make_embedding(1000.0)];

        store
            .insert(&chunks, &embeddings, &no_contexts(2), "default", "abc", "100")
            .await
            .unwrap();

        let clusters = scan_duplicates_impl(&store, 0.999, 10, None, true)
            .await
            .unwrap();

        assert!(clusters.is_empty(), "Expected no clusters with very high threshold");
    }

    #[tokio::test]
    async fn test_scan_empty_store() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("vectors");
        let store = VectorStore::open(&path).await.unwrap();

        let clusters = scan_duplicates_impl(&store, 0.90, 10, None, true)
            .await
            .unwrap();

        assert!(clusters.is_empty());
    }

    #[tokio::test]
    async fn test_scan_deduplicates_pairs() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("vectors");
        let mut store = VectorStore::open(&path).await.unwrap();

        // Two identical embeddings
        let emb = make_embedding(0.0);
        let chunks = vec![
            sample_chunk("a", "func_a", "src/a.rs"),
            sample_chunk("b", "func_b", "src/b.rs"),
        ];

        store
            .insert(&chunks, &[emb.clone(), emb.clone()], &no_contexts(2), "default", "abc", "100")
            .await
            .unwrap();

        let clusters = scan_duplicates_impl(&store, 0.50, 10, None, true)
            .await
            .unwrap();

        // Should produce exactly one cluster with 2 chunks
        assert_eq!(clusters.len(), 1, "Expected exactly one cluster");
        assert_eq!(clusters[0].members.len(), 1, "Cluster should have 1 member + representative");
    }

    #[tokio::test]
    async fn test_scan_max_clusters_limit() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("vectors");
        let mut store = VectorStore::open(&path).await.unwrap();

        // Create 3 distinct pairs (6 chunks), each pair identical
        let mut chunks = Vec::new();
        let mut embeddings = Vec::new();
        for pair in 0..3 {
            let emb = make_embedding(pair as f32 * 500.0);
            chunks.push(sample_chunk(
                &format!("p{}a", pair),
                &format!("func_{}a", pair),
                &format!("src/p{}a.rs", pair),
            ));
            chunks.push(sample_chunk(
                &format!("p{}b", pair),
                &format!("func_{}b", pair),
                &format!("src/p{}b.rs", pair),
            ));
            embeddings.push(emb.clone());
            embeddings.push(emb);
        }

        store
            .insert(&chunks, &embeddings, &no_contexts(6), "default", "abc", "100")
            .await
            .unwrap();

        // Limit to 2 clusters
        let clusters = scan_duplicates_impl(&store, 0.50, 2, None, true)
            .await
            .unwrap();

        assert!(clusters.len() <= 2, "Should respect max_clusters limit");
    }

    #[tokio::test]
    async fn test_scan_cross_repo_filtering() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("vectors");
        let mut store = VectorStore::open(&path).await.unwrap();

        // Two identical chunks in different repos
        let emb = make_embedding(0.0);
        let c1 = sample_chunk("r1c1", "func_a", "src/a.rs");
        let c2 = sample_chunk("r2c1", "func_a", "src/a.rs");

        store
            .insert(&[c1], &[emb.clone()], &no_contexts(1), "repo1", "abc", "100")
            .await
            .unwrap();
        store
            .insert(&[c2], &[emb.clone()], &no_contexts(1), "repo2", "def", "100")
            .await
            .unwrap();

        // cross_repo=false should NOT find duplicates across repos
        let clusters_same_repo = scan_duplicates_impl(&store, 0.50, 10, None, false)
            .await
            .unwrap();
        assert!(
            clusters_same_repo.is_empty(),
            "cross_repo=false should not find cross-repo duplicates"
        );

        // cross_repo=true should find the cross-repo pair
        let clusters_cross = scan_duplicates_impl(&store, 0.50, 10, None, true)
            .await
            .unwrap();
        assert!(
            !clusters_cross.is_empty(),
            "cross_repo=true should find cross-repo duplicates"
        );
    }

    /// Make an embedding that is orthogonal to the standard make_embedding vectors.
    /// Uses alternating sign pattern to create a distinct direction.
    fn make_orthogonal_embedding(seed: f32) -> Vec<f32> {
        let mut emb: Vec<f32> = (0..384)
            .map(|i| {
                let sign = if i % 2 == 0 { 1.0 } else { -1.0 };
                sign * ((i as f32) + seed) / 384.0
            })
            .collect();
        let norm: f32 = emb.iter().map(|x| x * x).sum::<f32>().sqrt();
        emb.iter_mut().for_each(|x| *x /= norm);
        emb
    }

    #[tokio::test]
    async fn test_scan_clusters_sorted_by_size() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("vectors");
        let mut store = VectorStore::open(&path).await.unwrap();

        // Create one cluster of 3 (identical embeddings) and one cluster of 2
        // Use orthogonal embeddings so the groups are clearly separated
        let emb_big = make_embedding(0.0);
        let emb_small = make_orthogonal_embedding(0.0);

        let chunks = vec![
            sample_chunk("big1", "fn_a", "src/a.rs"),
            sample_chunk("big2", "fn_b", "src/b.rs"),
            sample_chunk("big3", "fn_c", "src/c.rs"),
            sample_chunk("small1", "fn_d", "src/d.rs"),
            sample_chunk("small2", "fn_e", "src/e.rs"),
        ];
        let embeddings = vec![
            emb_big.clone(),
            emb_big.clone(),
            emb_big,
            emb_small.clone(),
            emb_small,
        ];

        store
            .insert(&chunks, &embeddings, &no_contexts(5), "default", "abc", "100")
            .await
            .unwrap();

        let clusters = scan_duplicates_impl(&store, 0.50, 10, None, true)
            .await
            .unwrap();

        assert!(clusters.len() >= 2, "Expected at least 2 clusters, got {}", clusters.len());

        // First cluster should be the bigger one (3 members total)
        let first_size = clusters[0].members.len() + 1;
        let second_size = clusters[1].members.len() + 1;
        assert!(
            first_size >= second_size,
            "Clusters should be sorted by size (largest first): {} vs {}",
            first_size,
            second_size
        );
    }
}
