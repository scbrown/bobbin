# Task: Integrate import extraction into the index pipeline

## Summary

Wire import extraction + resolution into `bobbin index` so dependencies are stored alongside chunks and coupling data.

## Files

- `src/cli/index.rs` (modify)

## Implementation

In the indexing pipeline (after parsing chunks, before git coupling analysis):

```rust
// After parsing chunks for a file:
let chunks = parser.parse_file(file_path, &content)?;

// NEW: Extract and resolve imports
if config.dependencies.enabled {
    let imports = parser.extract_imports(file_path, &content)?;
    if !imports.is_empty() {
        // Clear old deps for this file (handles re-indexing)
        metadata_store.clear_file_dependencies(&rel_path)?;

        for import in imports {
            let resolved = resolver.resolve(&import, &file_path);
            let dep = ImportDependency {
                file_a: rel_path.clone(),
                file_b: resolved.unwrap_or_else(|| format!("unresolved:{}", import.path)),
                dep_type: import.dep_type,
                import_statement: import.statement,
                symbol: None,
                resolved: resolved.is_some(),
            };
            metadata_store.upsert_dependency(&dep)?;
        }
    }
}
```

The `ImportResolver` should be created once at the start of indexing (not per-file) since it needs the set of all indexed files.

### Incremental indexing

When a file is re-indexed (hash changed):
1. Clear its old dependencies: `clear_file_dependencies(path)`
2. Extract and store new dependencies

When a file is deleted from the index:
1. Clear its dependencies (as source)
2. Note: it may still appear as target of other files' deps - this is fine, those will be `resolved: false` on next index

### Statistics

Add to the index output stats:
- Total imports extracted
- Total resolved
- Total unresolved (external)

## Dependencies

- Requires Task 1 (extraction), Task 2 (storage), Task 3 (resolver)

## Tests

- Index a test fixture with known imports, verify deps stored correctly
- Re-index changed file, verify old deps cleared and new ones stored
- Verify stats output includes import counts

## Acceptance Criteria

- [ ] `bobbin index` extracts and stores imports
- [ ] Incremental indexing clears stale deps
- [ ] Stats show import counts
- [ ] Controlled by config flag
- [ ] No performance regression on repos without imports
