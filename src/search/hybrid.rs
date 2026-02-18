use anyhow::Result;
use std::collections::HashMap;

use crate::index::Embedder;
use crate::storage::VectorStore;
use crate::types::{MatchType, SearchResult};

/// Calculate recency boost factor using exponential decay.
///
/// Returns a value in [0.0, 1.0] where 1.0 means "just indexed" and values
/// decay toward 0.0 as age increases. The half_life_days parameter controls
/// how fast the decay happens (after half_life_days, the factor is 0.5).
///
/// Returns 1.0 if recency boosting is disabled (half_life <= 0) or if
/// the indexed_at timestamp is missing.
pub fn recency_factor(indexed_at: Option<i64>, half_life_days: f32) -> f32 {
    if half_life_days <= 0.0 {
        return 1.0;
    }
    let ts = match indexed_at {
        Some(t) if t > 0 => t,
        _ => return 1.0,
    };
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let age_days = ((now - ts) as f64 / 86400.0).max(0.0);
    let decay = (0.5_f64).powf(age_days / half_life_days as f64);
    decay as f32
}

/// Apply recency boost to a raw score.
///
/// `weight` controls how much recency affects the final score:
///   final = score * (1.0 - weight + weight * decay)
///
/// At weight=0.0 recency has no effect. At weight=0.3 (default), the maximum
/// penalty for very old items is 30% of the original score.
pub fn apply_recency_boost(score: f32, indexed_at: Option<i64>, half_life_days: f32, weight: f32) -> f32 {
    if weight <= 0.0 || half_life_days <= 0.0 {
        return score;
    }
    let decay = recency_factor(indexed_at, half_life_days);
    score * (1.0 - weight + weight * decay)
}

/// Combines semantic and keyword search results using Reciprocal Rank Fusion (RRF)
pub struct HybridSearch {
    embedder: Embedder,
    vector_store: VectorStore,
    semantic_weight: f32,
    recency_half_life_days: f32,
    recency_weight: f32,
    rrf_k: f32,
}

impl HybridSearch {
    /// Create a new hybrid search engine
    pub fn new(
        embedder: Embedder,
        vector_store: VectorStore,
        semantic_weight: f32,
    ) -> Self {
        Self {
            embedder,
            vector_store,
            semantic_weight,
            recency_half_life_days: 30.0,
            recency_weight: 0.3,
            rrf_k: 60.0,
        }
    }

    /// Configure recency boosting parameters
    pub fn with_recency(mut self, half_life_days: f32, weight: f32) -> Self {
        self.recency_half_life_days = half_life_days;
        self.recency_weight = weight;
        self
    }

    /// Configure the RRF constant k
    pub fn with_rrf_k(mut self, k: f32) -> Self {
        self.rrf_k = k;
        self
    }

    /// Perform hybrid search combining semantic and keyword results, optionally filtered by repo.
    ///
    /// The raw query is used for semantic search (embeddings handle natural language well).
    /// A preprocessed version (stopwords removed, prefixes stripped) is used for keyword
    /// search (BM25), improving relevance for conversational prompts.
    pub async fn search(&mut self, query: &str, limit: usize, repo: Option<&str>) -> Result<Vec<SearchResult>> {
        // Request more results from each source to have a good pool for fusion
        let fetch_limit = limit * 2;

        // Run semantic search with raw query (embeddings handle natural language)
        let query_embedding = self.embedder.embed(query).await?;
        let semantic_results = self.vector_store.search(&query_embedding, fetch_limit, repo).await?;

        // Preprocess query for keyword search (remove stopwords, strip prefixes)
        let keyword_query = super::preprocess::preprocess_for_keywords(query);
        let keyword_results = self.vector_store.search_fts(&keyword_query, fetch_limit, repo).await?;

        // Combine using RRF with recency boosting
        Self::combine_with_recency(
            semantic_results,
            keyword_results,
            self.semantic_weight,
            limit,
            self.recency_half_life_days,
            self.recency_weight,
            self.rrf_k,
        )
    }

    /// Combine semantic and keyword results using reciprocal rank fusion
    pub fn combine(
        semantic_results: Vec<SearchResult>,
        keyword_results: Vec<SearchResult>,
        semantic_weight: f32,
        limit: usize,
    ) -> Result<Vec<SearchResult>> {
        Self::combine_with_recency(semantic_results, keyword_results, semantic_weight, limit, 0.0, 0.0, 60.0)
    }

    /// Combine semantic and keyword results using RRF with optional recency boosting
    pub fn combine_with_recency(
        semantic_results: Vec<SearchResult>,
        keyword_results: Vec<SearchResult>,
        semantic_weight: f32,
        limit: usize,
        recency_half_life_days: f32,
        recency_weight: f32,
        rrf_k: f32,
    ) -> Result<Vec<SearchResult>> {
        let keyword_weight = 1.0 - semantic_weight;
        let k = rrf_k;

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

        // Apply recency boost and sort
        let mut combined: Vec<_> = scores
            .into_values()
            .map(|(result, score)| {
                let boosted = apply_recency_boost(
                    score,
                    result.indexed_at,
                    recency_half_life_days,
                    recency_weight,
                );
                (result, boosted)
            })
            .collect();
        combined.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());

        let results = combined
            .into_iter()
            .take(limit)
            .map(|(mut result, score)| {
                result.score = score;
                if result.match_type.is_none() {
                    result.match_type = Some(MatchType::Semantic);
                }
                result
            })
            .collect();

        Ok(results)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_recency_factor_disabled() {
        assert_eq!(recency_factor(Some(1000000), 0.0), 1.0);
        assert_eq!(recency_factor(None, 30.0), 1.0);
        assert_eq!(recency_factor(Some(0), 30.0), 1.0);
    }

    #[test]
    fn test_recency_factor_recent() {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;
        // Just indexed: should be ~1.0
        let factor = recency_factor(Some(now), 30.0);
        assert!(factor > 0.99, "Recent factor should be ~1.0, got {}", factor);
    }

    #[test]
    fn test_recency_factor_at_half_life() {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;
        // 30 days ago with 30-day half-life: should be ~0.5
        let thirty_days_ago = now - 30 * 86400;
        let factor = recency_factor(Some(thirty_days_ago), 30.0);
        assert!((factor - 0.5).abs() < 0.01, "Factor at half-life should be ~0.5, got {}", factor);
    }

    #[test]
    fn test_apply_recency_boost_no_effect() {
        assert_eq!(apply_recency_boost(0.8, None, 30.0, 0.3), 0.8);
        assert_eq!(apply_recency_boost(0.8, Some(100), 0.0, 0.3), 0.8);
        assert_eq!(apply_recency_boost(0.8, Some(100), 30.0, 0.0), 0.8);
    }

    #[test]
    fn test_apply_recency_boost_recent() {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;
        // Recent item should keep ~full score
        let boosted = apply_recency_boost(1.0, Some(now), 30.0, 0.3);
        assert!(boosted > 0.99, "Recent item should keep full score, got {}", boosted);
    }

    #[test]
    fn test_apply_recency_boost_old() {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;
        // 90 days old (3 half-lives): decay = 0.125, boost = 1.0 - 0.3 + 0.3*0.125 = 0.7375
        let old = now - 90 * 86400;
        let boosted = apply_recency_boost(1.0, Some(old), 30.0, 0.3);
        assert!(boosted < 0.75, "Old item should lose some score, got {}", boosted);
        assert!(boosted > 0.70, "Old item shouldn't lose too much, got {}", boosted);
    }
}
