# Task: Make embedding dimensions dynamic

## Summary

Remove the hardcoded `EMBEDDING_DIM = 384` constant and derive dimensions from the model config. This is the foundation for supporting models with different output dimensions.

## Files

- `src/storage/lance.rs` (modify) - parameterize dimension
- `src/index/embedder.rs` (modify) - expose dimension from model
- `src/storage/sqlite.rs` (modify) - store dimension in metadata

## Changes

### `src/index/embedder.rs`

The `ModelConfig` already has a `dim` field and `dimension()` method (both marked `#[allow(dead_code)]`). Remove the dead_code suppression and make them the source of truth.

The `Embedder` struct should expose its dimension:
```rust
impl Embedder {
    pub fn dimension(&self) -> usize {
        self.config.dim
    }
}
```

### `src/storage/lance.rs`

Replace `const EMBEDDING_DIM: i32 = 384;` with a parameter:

1. `VectorStore::open()` should accept or detect the dimension
2. `schema()` becomes `schema(dim: i32)` - takes dimension as parameter
3. `to_record_batch()` takes dimension parameter
4. When opening an existing table, detect dimension from the schema:
   ```rust
   // Read the vector column's FixedSizeList size from existing schema
   fn detect_dimension(table: &Table) -> Result<i32>
   ```
5. When creating a new table, use the embedder's dimension

### `src/storage/sqlite.rs`

Store the dimension in metadata alongside the model name:
```rust
metadata_store.set_meta("embedding_dimension", &dim.to_string())?;
```

### Model change detection

In `src/cli/index.rs`, the current flow already detects model name changes and wipes the vector store. Extend this to also detect dimension changes:
```rust
let stored_dim = metadata_store.get_meta("embedding_dimension")?;
if stored_dim != Some(current_dim.to_string()) {
    // Must re-index - dimension mismatch
}
```

In `src/cli/search.rs`, add dimension validation:
```rust
let stored_dim = metadata_store.get_meta("embedding_dimension")?;
// Verify query embedding dimension matches stored dimension
```

## Tests

- Update all test code that uses hardcoded 384 dimension
- Test creating a store with dim=384, verify schema correct
- Test creating a store with dim=768, verify schema correct
- Test detecting dimension from existing table
- Test model change detection catches dimension mismatches

## Acceptance Criteria

- [ ] No hardcoded 384 anywhere in storage code
- [ ] Dimension flows from Embedder → VectorStore → schema
- [ ] Dimension stored in SQLite metadata
- [ ] Model change detection includes dimension check
- [ ] Existing 384-dim indexes still work (backward compatible)
- [ ] All tests updated and passing
