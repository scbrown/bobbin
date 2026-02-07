# Task: Single-Target Similarity Search

## Summary

Add core logic to find chunks semantically similar to a given target chunk or text query. This reuses the existing vector search infrastructure -- the key difference from `bobbin search` is that the query is a pre-existing chunk's embedding rather than user text.

## Files

- `src/analysis/mod.rs` (new) -- create the analysis module
- `src/analysis/similar.rs` (new) -- similarity logic

## Types

```rust
pub struct SimilarResult {
    pub chunk: Chunk,
    pub similarity: f32,       // Cosine similarity score
    pub explanation: String,   // Brief reason (derived from chunk names/types)
}

pub enum SimilarTarget {
    ChunkRef(String),          // "file.rs:function_name" syntax
    Text(String),              // Free-text query
}
```

## Implementation

Add `find_similar()` to a new `SimilarityAnalyzer` struct:

```rust
pub async fn find_similar(
    &self,
    target: &SimilarTarget,
    threshold: f32,     // min cosine similarity (default 0.85)
    limit: usize,       // max results (default 10)
    repo: Option<&str>,
) -> Result<Vec<SimilarResult>>
```

**Steps:**

1. **Resolve target to embedding:**
   - `ChunkRef`: Parse `file:name` syntax. Use `VectorStore::get_chunks_for_file()` to find the chunk, then extract its stored embedding from LanceDB.
   - `Text`: Use `Embedder::embed()` to generate an embedding from the query string.

2. **Vector search:** Call `VectorStore::search()` (or a new lower-level method that takes a raw embedding vector instead of text). Filter results by threshold. Exclude the target chunk itself.

3. **Build results:** For each match above threshold, construct a `SimilarResult` with the chunk data, similarity score, and a generated explanation line (e.g., "Both are authentication handlers").

**Key detail -- embedding extraction:** The current `VectorStore::search()` takes a text query and embeds it internally. We need either:
- A new method `search_by_vector(embedding: &[f32], ...)` that skips the embedding step, OR
- Access to the stored embedding for a chunk (query the `vector` column from LanceDB)

The cleaner approach is `search_by_vector()` since it avoids double-embedding and is also needed by scan mode.

## Pattern Reference

Follow `VectorStore::search()` for the LanceDB query pattern. The `flat_index` search call accepts a query vector directly -- we just need to expose this without the embedding step.

## Dependencies

None -- this is the first task in the similar feature.

## Tests

- Resolve a `ChunkRef` target and find similar chunks
- Resolve a `Text` target and find similar chunks
- Verify threshold filtering (similarity < threshold excluded)
- Verify self-exclusion (target chunk not in results)
- Verify results ordered by similarity descending

## Acceptance Criteria

- [ ] `src/analysis/mod.rs` created with `pub mod similar;`
- [ ] `SimilarityAnalyzer::find_similar()` implemented
- [ ] `search_by_vector()` (or equivalent) added to VectorStore
- [ ] `file:name` chunk reference syntax parsed correctly
- [ ] Threshold filtering works
- [ ] Target chunk excluded from results
- [ ] Unit tests pass
