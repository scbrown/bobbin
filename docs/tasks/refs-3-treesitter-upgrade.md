# Task: Tree-Sitter Based Reference Accuracy Upgrade

## Summary

Upgrade the FTS-based reference resolution (refs-1) with Tree-sitter identifier scanning for higher accuracy. This is Approach A from the design doc: at index time, scan each chunk for identifier tokens that match known definitions, building a proper reference table.

**Note:** This is a follow-up enhancement. The FTS-based approach (refs-1) should ship first to provide immediate value.

## Files

- `src/analysis/refs.rs` (modify) -- add tree-sitter scanning option
- `src/storage/lance.rs` (modify) -- add `refs` table (optional, for pre-computed refs)
- `src/index/mod.rs` (modify) -- add ref extraction to index pipeline

## Design

### Index-Time Process

1. **First pass (existing):** Extract definitions (named chunks). Build symbol table: `HashMap<String, ChunkId>`.

2. **Second pass (new):** For each chunk, walk the Tree-sitter AST looking for `identifier` nodes. For each identifier that matches a key in the symbol table, record a reference:
   ```rust
   pub struct IndexedRef {
       pub symbol_name: String,
       pub definition_chunk_id: String,
       pub usage_file: String,
       pub usage_line: u32,
       pub usage_context: String,
   }
   ```

3. **Storage:** Either:
   - New `refs` table in LanceDB with the above columns
   - Or extend the SQLite metadata store

### Query-Time Improvement

When `RefAnalyzer::find_refs()` runs:
- If indexed refs exist → query the refs table (fast, accurate)
- If not → fall back to FTS approach (Approach B)

This graceful degradation means the upgrade is backwards-compatible.

### Tree-sitter Identifier Nodes

Per language:
| Language | Identifier node type |
|----------|---------------------|
| Rust | `identifier` |
| TypeScript/JS | `identifier`, `property_identifier` |
| Python | `identifier` |
| Go | `identifier`, `field_identifier` |
| Java | `identifier` |
| C/C++ | `identifier`, `field_identifier` |

**Filtering:** Only match identifiers that:
- Are NOT inside the definition itself (avoid self-reference)
- Are NOT inside string literals or comments
- Match a known definition name exactly

## Dependencies

- Requires `refs-1-fts-lookup` to be complete
- Should ship after FTS approach proves valuable

## Complexity

High. The Tree-sitter scanning is straightforward per-chunk, but:
- Re-indexing all chunks adds time to `bobbin index`
- The refs table needs schema design
- False positive filtering requires understanding AST context (inside string? comment?)

## Acceptance Criteria

- [ ] Index-time identifier scanning works for Rust, TS, Python, Go
- [ ] Refs stored in new table
- [ ] Query falls back to FTS when refs table is empty
- [ ] False positives reduced vs FTS approach
- [ ] Indexing time increase is acceptable (< 2x)
- [ ] Tests pass for each supported language
