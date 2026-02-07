# Task: `bobbin refs` CLI Command + MCP Tool

## Summary

Wire symbol reference resolution into a CLI subcommand and MCP tool. Supports finding definitions, usages, and listing symbols in a file.

## Files

- `src/cli/refs.rs` (new) -- CLI command
- `src/cli/mod.rs` (modify) -- register subcommand
- `src/mcp/server.rs` (modify) -- add `refs` MCP tool

## CLI Interface

```bash
# Find all references to a symbol
bobbin refs login_handler
bobbin refs LoginRequest --type struct

# Find the definition of a symbol
bobbin refs login_handler --definition

# List all symbols in a file
bobbin refs --file src/auth.rs
```

### Flags

| Flag | Short | Default | Description |
|------|-------|---------|-------------|
| `--type` | `-t` | any | Filter by symbol type (function, struct, trait, etc.) |
| `--definition` | `-D` | `false` | Find definition instead of usages |
| `--file` | `-f` | | List all symbols in a file |
| `--limit` | `-n` | `20` | Max results |
| `--repo` | `-r` | all | Filter to repository |
| `--json` | | `false` | JSON output |

### Modes (mutually exclusive)

1. **Default (usages):** `bobbin refs <symbol>` -- find definition + usages
2. **Definition only:** `bobbin refs <symbol> --definition` -- just the definition
3. **File listing:** `bobbin refs --file <path>` -- all symbols in file

## Implementation

### Output Format

**Usages mode:**
```
References to login_handler:

  Definition:
    src/auth.rs:45  fn login_handler(req: LoginRequest) -> Result<Token>

  Usages (7 found):
    src/api/routes.rs:23      .route("/login", post(login_handler))
    src/middleware/auth.rs:67  let token = login_handler(req).await?;
    tests/auth_test.rs:12     use crate::auth::login_handler;
```

**Definition mode:**
```
Definition of login_handler:
  src/auth.rs:45  fn login_handler(req: LoginRequest) -> Result<Token>
```

**File listing mode:**
```
Symbols in src/auth.rs:

  login_handler (function), line 45
  AuthService (struct), line 12
  validate_token (function), line 90
```

### MCP Tool

Add `refs` tool. Accepts symbol_name (or file_path for listing), type filter, definition flag, limit, repo. Returns JSON with definition and usages.

## Dependencies

- Requires `refs-1-fts-lookup`

## Tests

- Verify usages mode shows definition + usages
- Verify `--definition` shows only definition
- Verify `--file` lists symbols
- Verify `--type` filters by symbol type
- Verify `--json` produces valid JSON

## Acceptance Criteria

- [ ] `bobbin refs <symbol>` shows definition and usages
- [ ] `bobbin refs --definition` shows only definition
- [ ] `bobbin refs --file <path>` lists file symbols
- [ ] `--type` filtering works
- [ ] `--json` produces valid JSON
- [ ] MCP `refs` tool registered and functional
- [ ] Help text is clear
