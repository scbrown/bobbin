# Task: FTS-Based Symbol Reference Resolution

## Summary

Implement symbol cross-reference resolution using full-text search (Approach B from the design doc). For a given symbol name, find its definition chunk and all chunks that reference it via FTS. This is the fast, good-enough approach that covers 80% of use cases.

## Files

- `src/analysis/refs.rs` (new) -- reference resolution logic
- `src/analysis/mod.rs` (modify) -- add `pub mod refs;`

## Types

```rust
pub struct SymbolDefinition {
    pub name: String,
    pub chunk_type: ChunkType,     // function, struct, trait, etc.
    pub file_path: String,
    pub start_line: u32,
    pub end_line: u32,
    pub signature: String,         // First line of the chunk content
}

pub struct SymbolUsage {
    pub file_path: String,
    pub line: u32,
    pub context: String,           // The line of code containing the usage
}

pub struct SymbolRefs {
    pub definition: Option<SymbolDefinition>,
    pub usages: Vec<SymbolUsage>,
}

pub struct FileSymbols {
    pub path: String,
    pub symbols: Vec<SymbolDefinition>,
}
```

## Implementation

Add `RefAnalyzer`:

```rust
pub struct RefAnalyzer<'a> {
    vector_store: &'a VectorStore,
}

impl RefAnalyzer<'_> {
    /// Find the definition and usages of a symbol.
    pub async fn find_refs(
        &self,
        symbol_name: &str,
        symbol_type: Option<&str>,  // filter by chunk_type
        limit: usize,
        repo: Option<&str>,
    ) -> Result<SymbolRefs>

    /// Find the definition of a symbol.
    pub async fn find_definition(
        &self,
        symbol_name: &str,
        symbol_type: Option<&str>,
        repo: Option<&str>,
    ) -> Result<Option<SymbolDefinition>>

    /// List all symbols defined in a file.
    pub async fn list_symbols(
        &self,
        file_path: &str,
        repo: Option<&str>,
    ) -> Result<FileSymbols>
}
```

### `find_definition()`

1. Query indexed chunks where `name == symbol_name` (exact match on chunk name column).
2. If `symbol_type` provided, also filter by `chunk_type`.
3. If multiple definitions found (e.g., same name in different files), return all and let the caller disambiguate.

### `find_refs()` (FTS-based, Approach B)

1. Find definition(s) using `find_definition()`.
2. Run FTS search: `vector_store.search_fts(symbol_name, limit, repo)`.
3. Filter out the definition chunk itself.
4. For each matching chunk, extract the specific line(s) containing the symbol name.
5. Build `SymbolUsage` entries with file path, line number, and line content.

**Known limitations of FTS approach:**
- May produce false positives (symbol name in comments, strings, or unrelated contexts)
- Won't find usages that rename the symbol (e.g., `use foo as bar`)
- Case-sensitive matching may miss some languages

These are acceptable for v1. The Tree-sitter upgrade (refs-3) addresses accuracy.

### `list_symbols()`

1. Call `vector_store.get_chunks_for_file(file_path, repo)`.
2. Filter to chunks that have a `name` (named definitions).
3. Return as `FileSymbols`.

## Dependencies

None -- uses existing VectorStore queries.

## Tests

- Find definition of a known function, verify correct file/line
- Find usages via FTS, verify at least some real usages found
- List symbols in a file, verify all named chunks returned
- Verify definition chunk excluded from usages
- Verify `symbol_type` filtering works

## Acceptance Criteria

- [ ] `find_definition()` locates symbol definitions by name
- [ ] `find_refs()` returns FTS-based usages
- [ ] `list_symbols()` returns all named chunks in a file
- [ ] Definition chunk excluded from usage results
- [ ] Type filtering works
- [ ] Tests pass
