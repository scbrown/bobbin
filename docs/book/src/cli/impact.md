---
title: impact
description: Predict which files are affected by a change
tags: [cli, impact, analysis]
status: draft
category: cli-reference
related: [cli/related.md, cli/deps.md]
commands: [impact]
feature: impact
source_files: [src/cli/impact.rs]
---

# impact

Predict which files are affected by a change to a target file or function.

## Synopsis

```bash
bobbin impact [OPTIONS] <TARGET>
```

## Description

The `impact` command combines git co-change coupling, semantic similarity, and dependency signals to predict which files would be affected if you change a target file or function. It produces a ranked list of impacted files with signal attribution.

Use `--depth` for transitive expansion: at depth 2+, the command also checks the impact of impacted files, widening the blast radius estimate.

## Options

| Option | Short | Default | Description |
|--------|-------|---------|-------------|
| `--path <DIR>` | | `.` | Directory to analyze |
| `--depth <N>` | `-d` | `1` | Transitive impact depth (1–3) |
| `--mode <MODE>` | `-m` | `combined` | Signal mode: `combined`, `coupling`, `semantic`, `deps` |
| `--limit <N>` | `-n` | `15` | Maximum number of results |
| `--threshold <SCORE>` | `-t` | `0.1` | Minimum impact score (0.0–1.0) |
| `--repo <NAME>` | `-r` | | Filter to a specific repository |

## Examples

Show files impacted by changing a file:

```bash
bobbin impact src/auth/middleware.rs
```

Show transitive impact (depth 2):

```bash
bobbin impact src/auth/middleware.rs --depth 2
```

Use only coupling signal:

```bash
bobbin impact src/auth/middleware.rs --mode coupling
```

JSON output:

```bash
bobbin impact src/auth/middleware.rs --json
```

## Prerequisites

Requires a bobbin index and a git repository. Run `bobbin init` and `bobbin index` first.

## See Also

- [related](related.md) — find temporally coupled files
- [deps](deps.md) — show import dependencies
