---
title: calibrate
description: Auto-tune search parameters against git history
tags: [cli, calibrate]
status: draft
category: cli-reference
related: [cli/search.md, guides/searching.md, config/reference.md]
commands: [calibrate]
feature: calibrate
source_files: [src/cli/calibrate.rs]
---

# calibrate

Auto-tune search parameters by probing your git history. Samples recent commits, builds queries from their diffs, and grid-sweeps search params to find the combination that best retrieves the changed files.

## Usage

```bash
bobbin calibrate [OPTIONS] [PATH]
```

## Examples

```bash
bobbin calibrate                          # Quick calibration (20 samples)
bobbin calibrate --apply                  # Apply best config to .bobbin/calibration.json
bobbin calibrate --full                   # Extended sweep (recency + coupling params)
bobbin calibrate --full --resume          # Resume interrupted full sweep
bobbin calibrate --bridge-sweep           # Sweep bridge params using calibrated core
bobbin calibrate -n 50 --since "1 year"   # More samples, wider time range
bobbin calibrate --repo myproject         # Calibrate a specific repo in multi-repo setup
```

## Options

| Flag | Short | Description |
|------|-------|-------------|
| `--samples <N>` | `-n` | Number of commits to sample (default: 20) |
| `--since <RANGE>` | | Time range to sample from, git format (default: "6 months ago") |
| `--search-limit <N>` | | Max results per probe. Omit to sweep [10, 20, 30, 40] |
| `--budget <N>` | | Budget lines per probe. Omit to sweep [150, 300, 500] |
| `--apply` | | Write best config to `.bobbin/calibration.json` |
| `--full` | | Extended sweep: also tunes recency and coupling parameters |
| `--resume` | | Resume an interrupted `--full` sweep from cache |
| `--bridge-sweep` | | Sweep bridge_mode + bridge_boost_factor only |
| `--repo <NAME>` | | Repo to calibrate (for multi-repo setups) |
| `--source <DIR>` | | Override source path for git sampling |
| `--verbose` | | Show detailed per-commit results |

## How It Works

1. **Sample** N recent commits from git history (stratified across the time range)
2. **Build queries** from commit messages — each message becomes a search probe
3. **Grid-sweep** parameter combinations across all configured dimensions
4. **Score** each combination by precision, recall, and F1 against ground truth (modified files)
5. **Rank** by F1 and report the best configuration

With `--apply`, writes the best params to `.bobbin/calibration.json`, which takes precedence over config.toml values at search time.

### Sample selection

Commits are filtered before sampling:

- **Included**: Commits with 2–30 changed files within the `--since` window
- **Excluded**: Merge commits, reverts, and noise commits (prefixes: `chore:`, `ci:`, `docs:`, `style:`, `build:`, `release:`, `bump `, `auto-merge`, `update dependency`)
- **Sampling**: Evenly-spaced picks across filtered candidates (stratified, not random)

If >50% of sampled commits have very short messages (<20 chars) or generic text ("fix", "wip", "temp"), calibration warns that accuracy may be reduced.

### Scoring

Each probe scores the context bundle returned for a commit message query against the files actually modified in that commit:

```
precision = |injected ∩ truth| / |injected|
recall    = |injected ∩ truth| / |truth|
f1        = 2 × precision × recall / (precision + recall)
```

Configs are ranked by average F1 across all sampled commits.

## Sweep Modes

### Core sweep (default)

Sweeps 5 core parameter dimensions:

| Parameter | Values |
|-----------|--------|
| `semantic_weight` | 0.0, 0.3, 0.5, 0.7, 0.9 |
| `doc_demotion` | 0.1, 0.3, 0.5 |
| `search_limit` | 10, 20, 30, 40 (or CLI override) |
| `budget_lines` | 150, 300, 500 (or CLI override) |
| `rrf_k` | 60.0 (fixed) |

Total: 180 configs × N commits = ~3,600 probes at default 20 samples. Takes a few minutes.

### Full sweep (`--full`)

Extends the core sweep with recency, coupling depth, and bridge parameters:

| Additional parameter | Values |
|---------------------|--------|
| `recency_half_life_days` | 7, 14, 30, 90 |
| `recency_weight` | 0.0, 0.15, 0.30, 0.50 |
| `coupling_depth` | 500, 2000, 5000, 20000 |
| `bridge_mode` | Off, Inject, Boost, BoostInject |
| `bridge_boost_factor` | 0.15, 0.3, 0.5 |

Total: ~960 configs × 4 coupling depths × N commits. Significantly longer (~15-30 min). Re-indexes coupling data per depth, so each depth is a separate probe run.

### Bridge sweep (`--bridge-sweep`)

Requires an existing `calibration.json` from a prior core or full sweep. Uses the calibrated core params and only sweeps bridge mode + boost factor (7 configs). Very fast.

## calibration.json

The output file contains:

```json
{
  "calibrated_at": "2026-03-22T12:00:00Z",
  "snapshot": {
    "chunk_count": 5103,
    "file_count": 312,
    "primary_language": "rust",
    "repo_age_days": 180,
    "recent_commit_rate": 2.5
  },
  "best_config": {
    "semantic_weight": 0.3,
    "doc_demotion": 0.1,
    "rrf_k": 60.0,
    "budget_lines": 300,
    "search_limit": 40,
    "bridge_mode": "inject"
  },
  "top_results": [ ... ],
  "sample_count": 20,
  "probe_count": 3600
}
```

**Precedence**: calibration.json > config.toml > compiled defaults. All search and context operations read calibration.json if present.

## Auto-recalibration

The `CalibrationGuard` triggers automatic recalibration during indexing when:

- **First run**: No prior calibration exists
- **Chunk count changed >20%**: Significant codebase growth or shrinkage
- **Primary language changed**: Project shifted languages
- **>30 days since last calibration**: Stale calibration

## Cache and `--resume`

Full sweeps can be interrupted. The cache is saved after each coupling depth completes to `.bobbin/calibration_cache.json`. Use `--resume` to pick up where you left off — previously completed depths are restored from cache, and only remaining depths are re-run. Cache is cleared on successful completion.

## Output

Shows a ranked table of parameter combinations with recall scores. The top result is the recommended configuration. Use `--json` for machine-readable output.
