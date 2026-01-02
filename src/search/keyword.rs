use anyhow::Result;

use crate::storage::MetadataStore;
use crate::types::SearchResult;

/// Performs keyword (FTS) search
pub struct KeywordSearch<'a> {
    metadata_store: &'a MetadataStore,
}

impl<'a> KeywordSearch<'a> {
    /// Create a new keyword search engine
    pub fn new(metadata_store: &'a MetadataStore) -> Self {
        Self { metadata_store }
    }

    /// Search for code matching a keyword pattern
    pub fn search(&self, query: &str, limit: usize) -> Result<Vec<SearchResult>> {
        self.metadata_store.search_fts(query, limit)
    }
}
