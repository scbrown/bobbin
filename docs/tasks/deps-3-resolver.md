# Task: Implement heuristic import resolver

## Summary

Map raw import paths to actual file paths in the repo. This is the hard part. Use language-specific heuristics that cover common patterns (~70% resolution rate is the target). Don't try to be perfect.

## Files

- `src/index/resolver.rs` (new)
- `src/index/mod.rs` (modify - add module)

## Design

```rust
pub struct ImportResolver {
    repo_root: PathBuf,
    indexed_files: HashSet<String>,  // all file paths in the index
}

impl ImportResolver {
    pub fn resolve(&self, import: &RawImport, from_file: &Path) -> Option<String> {
        // Returns relative path from repo root, or None if unresolved
    }
}
```

## Resolution strategies per language

### Rust
- `crate::x::y` → try `src/x/y.rs`, `src/x/y/mod.rs`
- `super::x` → parent dir + `x.rs` or `x/mod.rs`
- `self::x` → current dir + `x.rs` or `x/mod.rs`
- Bare name (no `::`) → likely external crate, mark unresolved

### TypeScript/JavaScript
- `./foo` or `../foo` → resolve relative, try `.ts`, `.tsx`, `.js`, `.jsx`, `/index.ts`
- `@/foo` → common alias for `src/foo` (check tsconfig if exists)
- Bare module name → external (node_modules), mark unresolved

### Python
- `from .foo import bar` → relative: current dir + `foo.py` or `foo/__init__.py`
- `from ..foo import bar` → parent dir
- `import foo.bar` → try `foo/bar.py`, `foo/bar/__init__.py` from repo root
- Standard lib / pip packages → mark unresolved

### Go
- Path containing repo module path → strip module prefix, resolve locally
- Standard lib (no dots in path) → mark unresolved
- Check `go.mod` for module path if available

### Java
- `com.example.foo.Bar` → try `src/main/java/com/example/foo/Bar.java`
- Standard lib (`java.*`, `javax.*`) → mark unresolved

### C/C++
- `"foo.h"` (quoted) → relative to current file, then include paths
- `<foo.h>` (angle brackets) → system header, mark unresolved

## Key principles

1. **Best-effort**: Return None for anything uncertain rather than guessing wrong
2. **Check against index**: Only resolve to files that are actually in the bobbin index (use `indexed_files` set)
3. **No config file parsing initially**: Don't parse tsconfig.json, Cargo.toml workspaces, etc. in v1. Just use filesystem heuristics
4. **Mark unresolved clearly**: Caller stores as `resolved: false` with original path

## Tests

- Rust: `use crate::auth::middleware` resolves to `src/auth/middleware.rs`
- TypeScript: `import from './utils'` resolves to `src/utils.ts`
- Python: `from ..models import User` resolves correctly with parent traversal
- External imports: `use serde::Serialize` returns None (unresolved)
- Test with actual bobbin source tree as fixture

## Acceptance Criteria

- [ ] Resolves common patterns for Rust, TypeScript, Python
- [ ] Go, Java, C/C++ have basic support
- [ ] External/stdlib imports correctly marked unresolved
- [ ] Only resolves to files that exist in the index
- [ ] Unit tests for each language
