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

    /// Search for semantically similar code
    pub async fn search(&mut self, query: &str, limit: usize) -> Result<Vec<SearchResult>> {
        // Embed the query
        let query_embedding = self.embedder.embed(query)?;

        // Search the vector store
        let mut results = self.vector_store.search(&query_embedding, limit).await?;

        // Mark results as semantic matches
        for result in &mut results {
            result.match_type = Some(crate::types::MatchType::Semantic);
        }

        Ok(results)
    }
}
