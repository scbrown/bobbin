use anyhow::Result;
use std::collections::HashMap;

use crate::index::Embedder;
use crate::storage::{MetadataStore, VectorStore};
use crate::types::{MatchType, SearchResult};

/// Combines semantic and keyword search results using Reciprocal Rank Fusion (RRF)
pub struct HybridSearch<'a> {
    embedder: Embedder,
    vector_store: VectorStore,
    metadata_store: &'a MetadataStore,
    semantic_weight: f32,
}

impl<'a> HybridSearch<'a> {
    /// Create a new hybrid search engine
    pub fn new(
        embedder: Embedder,
        vector_store: VectorStore,
        metadata_store: &'a MetadataStore,
        semantic_weight: f32,
    ) -> Self {
        Self {
            embedder,
            vector_store,
            metadata_store,
            semantic_weight,
        }
    }

    /// Perform hybrid search combining semantic and keyword results
    pub async fn search(&mut self, query: &str, limit: usize) -> Result<Vec<SearchResult>> {
        // Request more results from each source to have a good pool for fusion
        let fetch_limit = limit * 2;

        // Run semantic search
        let query_embedding = self.embedder.embed(query)?;
        let semantic_results = self.vector_store.search(&query_embedding, fetch_limit).await?;

        // Run keyword search
        let keyword_results = self.metadata_store.search_fts(query, fetch_limit)?;

        // Combine using RRF
        Self::combine(
            semantic_results,
            keyword_results,
            self.semantic_weight,
            limit,
        )
    }

    /// Combine semantic and keyword results using reciprocal rank fusion
    pub fn combine(
        semantic_results: Vec<SearchResult>,
        keyword_results: Vec<SearchResult>,
        semantic_weight: f32,
        limit: usize,
    ) -> Result<Vec<SearchResult>> {
        let keyword_weight = 1.0 - semantic_weight;
        let k = 60.0; // RRF constant

        // Build a map of chunk_id -> (best_result, combined_score)
        let mut scores: HashMap<String, (SearchResult, f32)> = HashMap::new();

        // Add semantic results with RRF scoring
        for (rank, result) in semantic_results.into_iter().enumerate() {
            let rrf_score = semantic_weight / (k + rank as f32 + 1.0);
            scores.insert(result.chunk.id.clone(), (result, rrf_score));
        }

        // Add keyword results, combining scores if already present
        for (rank, result) in keyword_results.into_iter().enumerate() {
            let rrf_score = keyword_weight / (k + rank as f32 + 1.0);

            scores
                .entry(result.chunk.id.clone())
                .and_modify(|(existing, score)| {
                    *score += rrf_score;
                    existing.match_type = Some(MatchType::Hybrid);
                })
                .or_insert((result, rrf_score));
        }

        // Sort by combined score and take top limit
        let mut combined: Vec<_> = scores.into_values().collect();
        combined.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());

        let results = combined
            .into_iter()
            .take(limit)
            .map(|(mut result, score)| {
                result.score = score;
                if result.match_type.is_none() {
                    // Result only appeared in semantic search
                    result.match_type = Some(MatchType::Semantic);
                }
                result
            })
            .collect();

        Ok(results)
    }
}
