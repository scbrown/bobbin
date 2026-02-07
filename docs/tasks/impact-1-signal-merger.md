# Task: Multi-Signal Impact Analysis Core

## Summary

Create the impact analysis module that combines coupling, semantic similarity, and dependency signals to predict what code is affected when a target file or function changes.

## Files

- `src/analysis/impact.rs` (new) -- impact analysis logic
- `src/analysis/mod.rs` (modify) -- add `pub mod impact;`

## Types

```rust
pub struct ImpactResult {
    pub path: String,
    pub signal: ImpactSignal,
    pub score: f32,                // [0, 1]
    pub reason: String,            // Human-readable explanation
}

pub enum ImpactSignal {
    Coupling { co_changes: u32 },
    Semantic { similarity: f32 },
    Dependency,                    // Import graph (future)
    Combined,                      // Max of available signals
}

pub struct ImpactConfig {
    pub mode: ImpactMode,
    pub threshold: f32,            // min score to report (default 0.1)
    pub limit: usize,              // max results (default 15)
}

pub enum ImpactMode {
    Combined,
    Coupling,
    Semantic,
    Deps,                          // Gated on bobbin-graph
}
```

## Implementation

Add `ImpactAnalyzer`:

```rust
pub struct ImpactAnalyzer<'a> {
    metadata_store: &'a MetadataStore,
    vector_store: &'a VectorStore,
    // Future: dep_graph: Option<&'a DepGraph>,
}

impl ImpactAnalyzer<'_> {
    pub async fn analyze(
        &self,
        target: &str,           // file path or file:function
        config: &ImpactConfig,
        repo: Option<&str>,
    ) -> Result<Vec<ImpactResult>>
}
```

**Steps:**

1. **Resolve target:** If `file:function` syntax, find the specific chunk. Otherwise treat as file path.

2. **Gather signals** (based on `config.mode`):

   a. **Coupling signal:** Call `MetadataStore::get_coupling(file_path, limit)`. Normalize scores to [0, 1] (divide by max score).

   b. **Semantic signal:** Get the target chunk's embedding, run `search_by_vector()` (from similar-1). Filter to other files only. Use similarity score directly (already [0, 1]).

   c. **Dependency signal:** Return error with "dependency graph not yet available" message when `ImpactMode::Deps` requested. This will be enabled when `bobbin-graph` lands.

3. **Merge results:**
   - Build a `HashMap<String, Vec<(ImpactSignal, f32)>>` keyed by file path.
   - For `Combined` mode: take `max(score)` across all signals for each file.
   - For single-signal mode: only include that signal's results.

4. **Filter and sort:** Remove results below threshold, sort by score descending, limit.

## Dependencies

- Soft dependency on `similar-1-single-target` (for `search_by_vector()`). Can implement without it by using `VectorStore::search()` with the target chunk's content as text query (less precise but functional).

## Tests

- Verify coupling signal returns co-changed files
- Verify semantic signal returns similar files
- Verify combined mode takes max across signals
- Verify threshold filtering
- Verify `Deps` mode returns informative error

## Acceptance Criteria

- [ ] `ImpactAnalyzer::analyze()` implemented
- [ ] Coupling signal queries MetadataStore
- [ ] Semantic signal queries VectorStore
- [ ] Combined mode merges with max()
- [ ] Results filtered by threshold and limited
- [ ] `Deps` mode returns clear "not yet available" error
- [ ] Tests pass
