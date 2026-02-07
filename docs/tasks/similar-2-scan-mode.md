# Task: Scan Mode -- Codebase-Wide Duplicate Detection

## Summary

Add scan mode to the similarity analyzer: iterate all chunks, find near-duplicate pairs, and cluster them using union-find. This is the expensive but high-value operation that surfaces all semantic clones across a codebase.

## Files

- `src/analysis/similar.rs` (modify) -- add `scan_duplicates()`

## Types

```rust
pub struct DuplicateCluster {
    pub representative: Chunk,     // Highest-scored chunk in cluster
    pub members: Vec<SimilarResult>, // Other chunks in cluster
    pub avg_similarity: f32,       // Average pairwise similarity
}
```

## Implementation

Add `scan_duplicates()` to `SimilarityAnalyzer`:

```rust
pub async fn scan_duplicates(
    &self,
    threshold: f32,        // min similarity (default 0.90 for scan)
    max_clusters: usize,   // max clusters to return (default 10)
    repo: Option<&str>,
    cross_repo: bool,      // compare across repos
) -> Result<Vec<DuplicateCluster>>
```

**Steps:**

1. **Load all chunk embeddings:** Query LanceDB for all chunks (optionally filtered by repo). Extract chunk metadata + embedding vectors.

2. **Batched self-join:** For each chunk, run `search_by_vector()` (from task similar-1) against all other chunks. Filter pairs above threshold. Deduplicate: only keep pair (A, B) where A.id < B.id to avoid counting (B, A).

3. **Union-find clustering:** Build a union-find structure over chunk IDs. For each duplicate pair, union them. Extract connected components as clusters.

4. **Rank and limit:** Sort clusters by size (largest first) or avg similarity. Return top `max_clusters`.

**Performance considerations:**
- For large codebases (10k+ chunks), batched self-join is O(n * k) where k is the vector search limit. Set k reasonably (e.g., 50).
- Consider adding a progress indicator since this can take seconds.
- If `cross_repo` is false, only compare chunks within the same repo.

## Dependencies

- Requires `similar-1-single-target` (for `search_by_vector()`)

## Tests

- Scan a small set of chunks with known duplicates, verify clusters formed
- Verify cross-repo filtering (same-repo only when `cross_repo=false`)
- Verify deduplication (pair appears once, not twice)
- Verify cluster size ordering

## Acceptance Criteria

- [ ] `scan_duplicates()` implemented
- [ ] Union-find clustering produces correct groups
- [ ] Cross-repo filtering works
- [ ] Results limited to `max_clusters`
- [ ] No duplicate pairs (A,B) and (B,A)
- [ ] Tests pass
