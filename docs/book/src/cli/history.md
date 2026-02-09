---
title: "history"
description: "Show commit history for a file"
status: draft
category: cli-reference
tags: [cli, history]
commands: [history]
feature: history
source_files: [src/cli/history.rs]
---

# history

Show commit history and churn statistics for a file.

## Usage

```bash
bobbin history <FILE> [OPTIONS]
```

## Examples

```bash
bobbin history src/main.rs
bobbin history src/main.rs --limit 50
bobbin history src/main.rs --json        # JSON output with stats
```

## Options

| Flag | Short | Description |
|------|-------|-------------|
| `--limit <N>` | `-n` | Maximum entries to show (default: 20) |

## Output

Output includes:

- Commit date, author, and message for each entry
- Referenced issue IDs (if present in commit messages)
- Statistics: total commits, churn rate (commits/month), author breakdown
