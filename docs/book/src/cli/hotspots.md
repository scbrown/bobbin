---
title: "hotspots"
description: "Identify code hotspots with high churn and high complexity"
category: cli-reference
tags: [cli, hotspots]
commands: [hotspots]
feature: hotspots
source_files: [src/cli/hotspots.rs]
---

# hotspots

Identify code hotspots — files with both high git churn and high structural complexity.

## Synopsis

```bash
bobbin hotspots [OPTIONS]
```

## Description

The `hotspots` command combines git commit history with AST-based complexity analysis to surface files that are both frequently changed and structurally complex. These files are the most likely sources of bugs and the best candidates for refactoring.

The **hotspot score** is the geometric mean of normalized churn and complexity:

```
score = sqrt(churn_normalized * complexity)
```

Where:
- **churn** is the number of commits touching a file in the time window, normalized against the most-changed file.
- **complexity** is a weighted AST complexity score in the range \[0, 1\].

Non-code files (markdown, JSON, YAML, TOML) and unknown file types are automatically excluded.

## Options

| Option | Short | Default | Description |
|--------|-------|---------|-------------|
| `--path <DIR>` | | `.` | Directory to analyze |
| `--since <EXPR>` | | `1 year ago` | Time window for churn analysis (git date expression) |
| `--limit <N>` | `-n` | `20` | Maximum number of hotspots to show |
| `--threshold <SCORE>` | | `0.0` | Minimum hotspot score (0.0–1.0) |

## Examples

Show top 20 hotspots from the last year:

```bash
bobbin hotspots
```

Narrow to the last 3 months, top 10:

```bash
bobbin hotspots --since "3 months ago" -n 10
```

Only show files with a score above 0.5:

```bash
bobbin hotspots --threshold 0.5
```

Verbose output includes a legend explaining the scoring:

```bash
bobbin hotspots --verbose
```

JSON output for CI or dashboards:

```bash
bobbin hotspots --json
```

## JSON Output

```json
{
  "count": 3,
  "since": "1 year ago",
  "hotspots": [
    {
      "file": "src/cli/hook.rs",
      "score": 0.823,
      "churn": 47,
      "complexity": 0.72,
      "language": "rust"
    }
  ]
}
```

## Supported Languages

Complexity analysis supports: Rust, TypeScript/JavaScript, Python, Go, Java, C, C++.

## Prerequisites

Requires a bobbin index and a git repository. Run `bobbin init` and `bobbin index` first.

## See Also

- [Hotspots Guide](../guides/hotspots.md) — strategies for using hotspot data
- [status](status.md) — check index statistics
