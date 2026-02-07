# Task: Surface dependency graph in CLI commands

## Summary

Add `--deps` flag to `bobbin related` and expose dependency data in MCP and status.

## Files

- `src/cli/related.rs` (modify)
- `src/cli/status.rs` (modify)
- `src/mcp/server.rs` (modify)
- `src/mcp/tools.rs` (modify)

## Changes to `bobbin related`

Add flags:
```rust
/// Show import-based relationships (what this file imports and what imports it)
#[arg(long)]
deps: bool,

/// Show both temporal coupling AND import dependencies
#[arg(long)]
all: bool,
```

When `--deps` is set:
- Query `get_dependencies(file)` for forward deps (what this file imports)
- Query `get_dependents(file)` for reverse deps (what imports this file)
- Output with direction labels: "imports" / "imported by"

When `--all` is set:
- Show both coupling AND dependency relationships
- Group by relationship type in output

### JSON output for deps mode

```json
{
  "file": "src/auth/middleware.rs",
  "imports": [
    { "path": "src/auth/token.rs", "dep_type": "use", "symbol": "TokenValidator" }
  ],
  "imported_by": [
    { "path": "src/api/routes.rs", "dep_type": "use" }
  ],
  "unresolved": [
    { "path": "serde::Serialize", "dep_type": "use" }
  ]
}
```

### Human output for deps mode

```
Dependencies for src/auth/middleware.rs:

  Imports:
    src/auth/token.rs        (use TokenValidator)
    src/config/auth.rs       (use AuthConfig)
    [external] serde         (use serde::Serialize)

  Imported by:
    src/api/routes.rs        (use)
    src/api/admin.rs         (use)
    tests/auth_test.rs       (use)
```

## Changes to `bobbin status`

Add dependency stats to `--detailed` output:
- Total import relationships
- Resolved vs unresolved count
- Top files by import count (most dependencies)
- Top files by dependent count (most imported)

## Changes to MCP

Update `related` tool to accept `deps` parameter.
Optionally add a `dependencies` tool for direct dep queries.

## Dependencies

- Requires Task 4 (index integration - data must exist to query)

## Acceptance Criteria

- [ ] `bobbin related --deps src/main.rs` shows imports and importers
- [ ] `bobbin related --all src/main.rs` shows both coupling + deps
- [ ] JSON output includes imports, imported_by, unresolved sections
- [ ] `bobbin status --detailed` shows dep stats
- [ ] MCP related tool supports deps parameter
