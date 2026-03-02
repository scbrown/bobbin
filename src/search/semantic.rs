use anyhow::Result;

use crate::index::Embedder;
use crate::storage::VectorStore;
use crate::types::SearchResult;

/// Performs semantic (vector) search
pub struct SemanticSearch {
    embedder: Embedder,
    vector_store: VectorStore,
}

impl SemanticSearch {
    /// Create a new semantic search engine
    pub fn new(embedder: Embedder, vector_store: VectorStore) -> Self {
        Self {
            embedder,
            vector_store,
        }
    }

    /// Search for semantically similar code, optionally filtered by repo
    pub async fn search(&mut self, query: &str, limit: usize, repo: Option<&str>) -> Result<Vec<SearchResult>> {
        self.search_filtered(query, limit, repo, None).await
    }

    /// Search with an additional SQL filter clause
    pub async fn search_filtered(&mut self, query: &str, limit: usize, repo: Option<&str>, filter: Option<&str>) -> Result<Vec<SearchResult>> {
        let query_embedding = self.embedder.embed(query).await?;
        let mut results = self.vector_store.search_filtered(&query_embedding, limit, repo, filter).await?;
        for result in &mut results {
            result.match_type = Some(crate::types::MatchType::Semantic);
        }
        Ok(results)
    }
}
