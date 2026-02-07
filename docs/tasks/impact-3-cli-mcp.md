# Task: `bobbin impact` CLI Command + MCP Tool

## Summary

Wire impact analysis into a CLI subcommand and MCP tool. Displays a ranked table of files likely affected by changes to a target, with signal attribution.

## Files

- `src/cli/impact.rs` (new) -- CLI command
- `src/cli/mod.rs` (modify) -- register subcommand
- `src/mcp/server.rs` (modify) -- add `impact` MCP tool

## CLI Interface

```bash
bobbin impact src/auth.rs
bobbin impact src/auth.rs:login_handler
bobbin impact src/auth.rs --depth 2
bobbin impact src/auth.rs --mode coupling
bobbin impact src/auth.rs --mode semantic
```

### Flags

| Flag | Short | Default | Description |
|------|-------|---------|-------------|
| `--depth` | `-d` | `1` | Transitive impact depth |
| `--mode` | `-m` | `combined` | Signal: combined, coupling, semantic, deps |
| `--limit` | `-n` | `15` | Max results |
| `--threshold` | `-t` | `0.1` | Min impact score |
| `--repo` | `-r` | all | Filter to repository |
| `--json` | | `false` | JSON output |

### Positional Argument

Required: file path or `file:function` reference.

## Implementation

### Command Flow

1. Parse target from positional arg
2. Build `ImpactConfig` from flags
3. Call `impact_analyzer.analyze(target, &config, depth, repo)`
4. Format and display results

### Output Format

```
Impact analysis for login_handler (src/auth.rs:45-82):

  #  File                          Signal      Score  Reason
  1. src/session.rs                coupling    0.82   Co-changed 47 times
  2. src/api/auth.rs               semantic    0.91   Similar auth logic
  3. src/middleware/auth.rs         deps        1.00   Imports login_handler
  4. tests/auth_test.rs            coupling    0.71   Co-changed 33 times

  Combined score = max(coupling, semantic, deps)
```

When `--depth 2`:
```
  5. src/api/session.rs            semantic    0.43   Similar to src/session.rs (depth 2)
```

### MCP Tool

Add `impact` tool. Accepts target, depth, mode, threshold, limit, repo. Returns JSON array of impact results.

## Dependencies

- Requires `impact-1-signal-merger` and `impact-2-transitive`

## Tests

- Verify output format with known impact results
- Verify `--mode` restricts to single signal
- Verify `--depth` enables transitive results
- Verify `--json` produces valid JSON

## Acceptance Criteria

- [ ] `bobbin impact <target>` produces ranked table
- [ ] Signal column shows which signal contributed
- [ ] `--mode` filtering works
- [ ] `--depth` transitive expansion works
- [ ] `--json` produces valid JSON
- [ ] MCP `impact` tool registered and functional
- [ ] Help text is clear
