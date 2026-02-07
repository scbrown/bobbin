# Task: Create Context Assembler Module

## Summary

Create the core context assembly engine that combines hybrid search + temporal coupling into a budget-aware context bundle. This is the heart of `bobbin context`.

## Files

- `src/search/context.rs` (new)
- `src/search/mod.rs` (add `pub mod context;`)

## Design

### Three-Phase Pipeline

**Phase 1 - Seed (Hybrid Search):**
- Run `HybridSearch::search()` with the query
- Use `limit` arg (default 20) as search limit
- Collect results as seed chunks

**Phase 2 - Expand (Temporal Coupling):**
- For each unique file in seed results, call `MetadataStore::get_coupling(file, max_coupled)`
- Filter by `coupling_threshold`
- For newly discovered files (not already in seed), call `VectorStore::get_chunks_for_file()`
- Track which seed file led to each coupled file (for `coupled_to` field)
- If `depth` is 0, skip this phase entirely

**Phase 3 - Assemble (Budget-Aware Merge):**
- Deduplicate chunks by `id`
- Apply line budget with priority ordering:
  1. Direct search hit chunks (by score desc)
  2. Coupled file chunks (by coupling score desc)
- Cap individual chunks at 50% of total budget
- Group results by file, order chunks within file by `start_line`
- Order files by highest-scoring chunk (direct hits first)

### Types

```rust
pub struct ContextAssembler { ... }

pub struct ContextConfig {
    pub budget_lines: usize,
    pub depth: u32,
    pub max_coupled: usize,
    pub coupling_threshold: f32,
    pub semantic_weight: f32,
    pub content_mode: ContentMode,
}

pub struct ContextBundle {
    pub query: String,
    pub files: Vec<ContextFile>,
    pub budget: BudgetInfo,
    pub summary: ContextSummary,
}

pub struct ContextFile {
    pub path: String,
    pub language: String,
    pub relevance: FileRelevance,  // Direct | Coupled
    pub score: f32,
    pub coupled_to: Vec<String>,   // which direct-hit files led here
    pub chunks: Vec<ContextChunk>,
}

pub struct ContextChunk {
    pub name: Option<String>,
    pub chunk_type: ChunkType,
    pub start_line: u32,
    pub end_line: u32,
    pub score: f32,
    pub match_type: Option<MatchType>,
    pub content: Option<String>,  // None if ContentMode::None
}

pub struct BudgetInfo {
    pub max_lines: usize,
    pub used_lines: usize,
}

pub struct ContextSummary {
    pub total_files: usize,
    pub total_chunks: usize,
    pub direct_hits: usize,
    pub coupled_additions: usize,
}

pub enum FileRelevance { Direct, Coupled }
pub enum ContentMode { Full, Preview, None }
```

### Content Mode Handling

- `Full`: include full `chunk.content` in output
- `Preview`: truncate to first 3 lines + "..."
- `None`: set `content` to None (paths/metadata only)

## Dependencies

- Requires Task 1 (`get_chunks_for_file`) to be done first
- Uses existing `HybridSearch`, `MetadataStore::get_coupling()`, `VectorStore`

## Tests

- Test budget enforcement: given chunks totaling 1000 lines and budget of 500, output respects budget
- Test deduplication: same chunk from search and coupling appears once
- Test depth=0: no coupling expansion
- Test content modes: Full includes content, None has None
- Test file ordering: direct hits before coupled files

## Acceptance Criteria

- [ ] `ContextAssembler::assemble()` returns `ContextBundle`
- [ ] Three-phase pipeline works correctly
- [ ] Budget is respected
- [ ] Chunks are deduplicated
- [ ] Content modes work
- [ ] All tests pass
