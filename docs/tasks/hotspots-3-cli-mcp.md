# Task: `bobbin hotspots` CLI Command + MCP Tool

## Summary

Wire the churn and complexity signals into a CLI subcommand that surfaces high-risk code. Combines `GitAnalyzer::get_file_churn()` with `ComplexityAnalyzer` results and produces a ranked table.

## Files

- `src/cli/hotspots.rs` (new) -- CLI command
- `src/cli/mod.rs` (modify) -- register subcommand
- `src/mcp/server.rs` (modify) -- add `hotspots` MCP tool

## CLI Interface

```bash
bobbin hotspots
bobbin hotspots --limit 20
bobbin hotspots --since "6 months ago"
bobbin hotspots --repo backend
bobbin hotspots --sort complexity
```

### Flags

| Flag | Short | Default | Description |
|------|-------|---------|-------------|
| `--limit` | `-n` | `10` | Number of hotspots to show |
| `--since` | | `"1 year ago"` | Git history lookback |
| `--sort` | `-s` | `combined` | Sort: `combined`, `churn`, `complexity` |
| `--repo` | `-r` | all | Filter to repository |
| `--detailed` | | `false` | Show per-chunk breakdown |
| `--json` | | `false` | JSON output |

## Implementation

### Core Logic

```rust
pub async fn compute_hotspots(
    git: &GitAnalyzer,
    complexity: &ComplexityAnalyzer,
    vector_store: &VectorStore,
    config: &HotspotsConfig,
) -> Result<Vec<Hotspot>>
```

1. Get churn map: `git.get_file_churn(config.since)`
2. Get all indexed file paths: `vector_store.get_all_file_paths(config.repo)`
3. For each file in both sets:
   - Get chunks: `vector_store.get_chunks_for_file(path, config.repo)`
   - Compute complexity: `complexity.analyze_file(path, content, language)`
   - Combine: `score = normalized_churn * normalized_complexity`
4. Sort by selected field, take top N

### Output Types

```rust
pub struct Hotspot {
    pub path: String,
    pub churn: u32,
    pub complexity: f32,
    pub score: f32,            // churn_norm * complexity_norm
    pub chunks: Vec<ChunkComplexity>, // only populated with --detailed
}
```

### Display

```
Code Hotspots (sorted by risk score):

  #  File                        Churn  Complexity  Score
  1. src/storage/lance.rs         47      8.2       386
  2. src/search/context.rs        31      7.1       220
  ...

  Hotspot score = churn * complexity
  Churn = commits in last 12 months
```

### MCP Tool

Add `hotspots` tool following existing MCP patterns. Returns JSON array of hotspot objects.

## Dependencies

- Requires `hotspots-1-churn-batch` and `hotspots-2-complexity-metrics`

## Tests

- Verify ranking: file with high churn + high complexity ranks first
- Verify `--sort` changes ordering
- Verify `--since` parameter passed through
- Verify `--json` produces valid JSON

## Acceptance Criteria

- [ ] `bobbin hotspots` produces ranked table
- [ ] Scoring combines churn and complexity correctly
- [ ] `--detailed` shows per-chunk breakdown
- [ ] `--sort`, `--since`, `--limit`, `--repo` flags work
- [ ] `--json` produces valid JSON
- [ ] MCP tool registered and functional
- [ ] Help text is clear
