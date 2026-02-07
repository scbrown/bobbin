# Task: `bobbin similar` CLI Command + MCP Tool

## Summary

Wire the similarity analyzer into a CLI subcommand and MCP tool. Supports both single-target and scan modes with standard output formatting and `--json` flag.

## Files

- `src/cli/similar.rs` (new) -- CLI command
- `src/cli/mod.rs` (modify) -- register subcommand
- `src/mcp/server.rs` (modify) -- add `similar` MCP tool
- `src/main.rs` (modify if needed) -- wire command

## CLI Interface

```bash
# Single-target: find chunks similar to a specific function
bobbin similar src/auth.rs:login_handler
bobbin similar src/auth.rs:login_handler --threshold 0.85

# Single-target: find chunks similar to a text query
bobbin similar "error handling with retry logic"

# Scan mode: find all near-duplicate pairs
bobbin similar --scan
bobbin similar --scan --threshold 0.90 --repo backend
bobbin similar --scan --cross-repo
```

### Flags

| Flag | Short | Default | Description |
|------|-------|---------|-------------|
| `--threshold` | `-t` | `0.85` | Minimum cosine similarity |
| `--scan` | | `false` | Scan entire codebase for duplicates |
| `--limit` | `-n` | `10` | Max results/clusters |
| `--repo` | `-r` | all | Filter to specific repository |
| `--cross-repo` | | `false` | In scan mode, compare across repos |
| `--json` | | `false` | JSON output |

## Implementation

### CLI

Use clap `#[derive(Args)]` following the pattern in existing commands (e.g., `src/cli/search.rs`).

**Single-target output:**
```
Similar to login_handler (src/auth.rs:45-82):

  1. session_handler (src/session.rs:23-58)     [0.94 similarity]
  2. api_login (src/api/auth.rs:12-45)           [0.91 similarity]
```

**Scan output:**
```
Duplicate clusters (threshold: 0.90):

  Cluster 1 (3 chunks, avg similarity: 0.93):
    - login_handler (src/auth.rs:45-82)
    - session_handler (src/session.rs:23-58)
    - api_login (src/api/auth.rs:12-45)
```

### MCP Tool

Add a `similar` tool to the MCP server that accepts the same parameters and returns JSON. Follow the pattern of existing MCP tools in `src/mcp/server.rs`.

## Dependencies

- Requires `similar-1-single-target` and `similar-2-scan-mode`

## Tests

- CLI integration test: run `bobbin similar` with a known target
- Verify `--json` produces valid JSON
- Verify `--scan` triggers scan mode

## Acceptance Criteria

- [ ] `bobbin similar <target>` works for chunk refs and text
- [ ] `bobbin similar --scan` works
- [ ] `--threshold`, `--limit`, `--repo` flags work
- [ ] `--json` produces valid JSON output
- [ ] MCP `similar` tool registered and functional
- [ ] Help text is clear
