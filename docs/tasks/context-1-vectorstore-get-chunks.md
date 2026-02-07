# Task: Add `get_chunks_for_file` to VectorStore

## Summary

Add a method to VectorStore that retrieves all chunks for a given file path, ordered by start_line. This is needed by the context command's coupling expansion phase.

## File

`src/storage/lance.rs`

## Implementation

Add to `impl VectorStore`:

```rust
/// Get all chunks for a specific file path, ordered by start_line
pub async fn get_chunks_for_file(
    &self,
    file_path: &str,
    repo: Option<&str>,
) -> Result<Vec<Chunk>>
```

- Filter the `chunks` table using `only_if` on the `file_path` column
- If `repo` is Some, also filter on `repo` column
- Convert rows to `Chunk` structs using the same pattern as `search()` result parsing
- Sort by `start_line` ascending
- Return empty vec if file not found (don't error)

## Pattern Reference

Follow the existing `get_file()` method which already filters on `file_path`. The difference is `get_file()` returns metadata while this returns full `Chunk` objects.

Also reference `search()` for how RecordBatch rows are converted to `Chunk` structs.

## Tests

Add unit test that:
1. Inserts chunks for two files
2. Calls `get_chunks_for_file` for one file
3. Asserts only that file's chunks are returned, in start_line order

## Acceptance Criteria

- [ ] Method exists and compiles
- [ ] Returns chunks sorted by start_line
- [ ] Respects repo filter
- [ ] Returns empty vec for unknown files
- [ ] Unit test passes
