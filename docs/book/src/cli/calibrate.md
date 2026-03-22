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

1. Samples N recent commits from git history
2. For each commit, extracts changed files and builds a search query from the diff
3. Grid-sweeps parameter combinations (semantic_weight, search_limit, budget)
4. Scores each combination by how well search results match the actual changed files
5. Reports the best configuration with recall metrics

With `--apply`, writes the best params to `.bobbin/calibration.json`, which takes precedence over config.toml values at search time.

## Output

Shows a ranked table of parameter combinations with recall scores. The top result is the recommended configuration. Use `--json` for machine-readable output.
