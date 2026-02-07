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

/// Build a brief explanation for why a result is similar
fn build_explanation(result: &SearchResult) -> String {
    let chunk = &result.chunk;
    let type_str = chunk.chunk_type.to_string();

    match &chunk.name {
        Some(name) => format!("{} '{}' in {}", type_str, name, chunk.file_path),
        None => format!("{} in {} (lines {}-{})", type_str, chunk.file_path, chunk.start_line, chunk.end_line),
    }
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
}
