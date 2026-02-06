use anyhow::Result;

use crate::storage::VectorStore;
use crate::types::SearchResult;

/// Performs keyword (FTS) search via LanceDB
pub struct KeywordSearch<'a> {
    vector_store: &'a mut VectorStore,
}

impl<'a> KeywordSearch<'a> {
    /// Create a new keyword search engine
    pub fn new(vector_store: &'a mut VectorStore) -> Self {
        Self { vector_store }
    }

    /// Search for code matching a keyword pattern
    pub async fn search(&mut self, query: &str, limit: usize) -> Result<Vec<SearchResult>> {
        self.vector_store.search_fts(query, limit).await
    }
}
