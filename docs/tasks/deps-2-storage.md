# Task: Add dependencies table to MetadataStore

## Summary

Add SQLite storage for import dependency edges, mirroring the existing coupling table pattern.

## Files

- `src/storage/sqlite.rs` (modify) - add table, types, and methods
- `src/types.rs` (modify) - add `ImportDependency` type
- `src/config.rs` (modify) - add `DependencyConfig`

## Schema

```sql
CREATE TABLE IF NOT EXISTS dependencies (
    file_a TEXT NOT NULL,           -- Importer (source file)
    file_b TEXT NOT NULL,           -- Imported (target file or "unresolved:<path>")
    dep_type TEXT NOT NULL,         -- "use", "import", "require", "from", "include"
    import_statement TEXT,          -- Raw: "use crate::auth::middleware;"
    symbol TEXT,                    -- What's imported: "middleware" (nullable)
    resolved BOOLEAN DEFAULT 0,    -- True if file_b is a real file path
    PRIMARY KEY (file_a, file_b, dep_type)
);

CREATE INDEX IF NOT EXISTS idx_deps_target ON dependencies(file_b);
CREATE INDEX IF NOT EXISTS idx_deps_source ON dependencies(file_a);
```

## Types

```rust
pub struct ImportDependency {
    pub file_a: String,
    pub file_b: String,
    pub dep_type: String,
    pub import_statement: String,
    pub symbol: Option<String>,
    pub resolved: bool,
}
```

## Methods to add to MetadataStore

```rust
pub fn upsert_dependency(&self, dep: &ImportDependency) -> Result<()>
pub fn get_dependencies(&self, file_path: &str) -> Result<Vec<ImportDependency>>  // what this file imports
pub fn get_dependents(&self, file_path: &str) -> Result<Vec<ImportDependency>>    // what imports this file
pub fn clear_dependencies(&self) -> Result<()>
pub fn clear_file_dependencies(&self, file_path: &str) -> Result<()>              // for re-indexing
```

## Config

Add to `config.rs`:
```rust
pub struct DependencyConfig {
    pub enabled: bool,           // default: true
    pub resolve_imports: bool,   // default: true
}
```

Add `pub dependencies: DependencyConfig` to `Config`.

## Pattern reference

Follow exactly the pattern of `upsert_coupling()` / `get_coupling()` / `clear_coupling()` in sqlite.rs.

## Tests

- Insert dependencies, query by source file, verify correct results
- Query by target file (reverse lookup), verify dependents found
- Clear file dependencies, verify only that file's deps removed
- Test upsert (update existing dep)

## Acceptance Criteria

- [ ] Table created on MetadataStore::open
- [ ] All CRUD methods work
- [ ] Config flag exists to enable/disable
- [ ] Unit tests pass
