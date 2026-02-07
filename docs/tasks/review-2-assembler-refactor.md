# Task: Refactor ContextAssembler to Accept External Seeds

## Summary

Extract the seed-to-bundle logic from `ContextAssembler` so that both `bobbin context` (search-based seeds) and `bobbin review` (diff-based seeds) can share the coupling expansion, budgeting, and output formatting code.

## Files

- `src/search/context.rs` (modify) -- extract `assemble_from_seeds()`
- `src/search/review.rs` (new) -- diff-to-seeds mapping

## Design

### Current Flow (context command)

```
query → hybrid_search() → seed chunks → expand (coupling) → budget → bundle
```

### Refactored Flow

```
# context command:
query → hybrid_search() → seed chunks ─┐
                                        ├→ assemble_from_seeds() → bundle
# review command:                       │
diff → map_diff_to_chunks() → seeds ───┘
```

### Changes to ContextAssembler

Add a new public method:

```rust
/// Assemble a context bundle from pre-computed seed chunks.
/// This is the shared pipeline used by both `context` and `review`.
pub async fn assemble_from_seeds(
    &self,
    seeds: Vec<SeedChunk>,
    config: &ContextConfig,
    repo: Option<&str>,
) -> Result<ContextBundle>
```

The existing `assemble()` method becomes a wrapper:
```rust
pub async fn assemble(&self, query: &str, ...) -> Result<ContextBundle> {
    let seeds = self.search_for_seeds(query, ...).await?;
    self.assemble_from_seeds(seeds, ...).await
}
```

### New Type

```rust
pub struct SeedChunk {
    pub chunk: Chunk,
    pub score: f32,
    pub source: SeedSource,
}

pub enum SeedSource {
    Search { match_type: MatchType },
    Diff { status: DiffStatus, added_lines: usize, removed_lines: usize },
}
```

### Diff-to-Seeds Mapping (review.rs)

```rust
/// Map git diff results to seed chunks by finding indexed chunks
/// that overlap with changed line ranges.
pub async fn map_diff_to_chunks(
    diff_files: &[DiffFile],
    vector_store: &VectorStore,
    repo: Option<&str>,
) -> Result<Vec<SeedChunk>>
```

For each `DiffFile`:
1. Get indexed chunks: `vector_store.get_chunks_for_file(path, repo)`
2. Find chunks whose `[start_line, end_line]` overlaps with any changed line
3. Assign score based on overlap ratio (more changed lines in chunk = higher score)
4. Wrap in `SeedChunk` with `SeedSource::Diff`

## Dependencies

- Requires `review-1-diff-parsing` (for `DiffFile` types)

## Tests

- Verify `assemble_from_seeds()` produces same output as old `assemble()` for search seeds
- Verify diff-to-seeds mapping: chunk overlapping changed lines is included
- Verify chunk NOT overlapping changed lines is excluded
- Verify overlap scoring: chunk with 100% overlap scores higher than 10% overlap

## Acceptance Criteria

- [ ] `assemble_from_seeds()` extracted and working
- [ ] Existing `assemble()` refactored as wrapper (no behavior change)
- [ ] `map_diff_to_chunks()` correctly maps diff hunks to indexed chunks
- [ ] Overlap-based scoring works
- [ ] Existing context command tests still pass
- [ ] New tests for seed-based assembly pass
