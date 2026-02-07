# Task: Extract raw imports from tree-sitter AST

## Summary

Add import/use/require statement extraction to the parser. This is the easy part - just walking tree-sitter nodes and collecting import statements. No resolution yet.

## Files

- `src/index/parser.rs` (modify) - add `extract_imports()` method
- `src/types.rs` (modify) - add `RawImport` type

## Types

```rust
pub struct RawImport {
    pub statement: String,  // "use crate::auth::middleware;"
    pub path: String,       // "crate::auth::middleware"
    pub dep_type: String,   // "use" | "import" | "require" | "from" | "include"
}
```

## Implementation

Add `extract_imports(path, content) -> Result<Vec<RawImport>>` to `CodeParser`.

Tree-sitter node types per language:

| Language | Node type | How to get path |
|----------|-----------|-----------------|
| Rust | `use_declaration` | Strip `use ` prefix and `;` suffix |
| TypeScript/JS | `import_statement` | Child node with kind `string` (the source) |
| TypeScript/JS | `call_expression` where function is `require` | Argument string |
| Python | `import_statement` | Text after `import ` |
| Python | `import_from_statement` | Module between `from` and `import` |
| Go | `import_spec` (within `import_declaration`) | String literal child |
| Java | `import_declaration` | Strip `import ` and `;` |
| C/C++ | `preproc_include` | Path between `<>` or `""` |

Walk the tree root's children (not recursive into functions - imports are top-level). For each matching node kind, extract the raw statement text and parse out the path.

## Pattern reference

Follow the same tree-sitter pattern as `extract_chunks()` and `node_to_chunk_type()` in parser.rs. The dispatch-by-language pattern already exists.

## Tests

- Parse a Rust file with `use` statements, verify RawImport fields
- Parse a TypeScript file with `import` and `require`, verify both detected
- Parse a Python file with `import` and `from...import`
- Verify no false positives (function calls named `import`, etc.)

## Acceptance Criteria

- [ ] `extract_imports()` returns imports for all 6 language families
- [ ] Statement text preserved verbatim
- [ ] Path correctly extracted (no `use`/`import` keywords, no semicolons)
- [ ] dep_type correctly categorized
- [ ] Unit tests for each language
