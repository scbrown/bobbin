---
title: grep
description: Keyword and regex search
tags: [cli, grep]
status: draft
category: cli-reference
related: [cli/search.md, guides/searching.md]
commands: [grep]
feature: grep
source_files: [src/cli/grep.rs]
---

# grep

Keyword and regex search using LanceDB full-text search.

## Usage

```bash
bobbin grep <PATTERN> [OPTIONS]
```

## Examples

```bash
bobbin grep "TODO"
bobbin grep "handleRequest" --ignore-case
bobbin grep "fn.*test" --regex                   # Regex post-filter
bobbin grep "TODO" --type function --context 2   # With context lines
bobbin grep "auth" --repo myproject
```

## Options

| Flag | Short | Description |
|------|-------|-------------|
| `--ignore-case` | `-i` | Case insensitive search |
| `--regex` | `-E` | Use extended regex matching (post-filters FTS results) |
| `--type <TYPE>` | `-t` | Filter by chunk type |
| `--limit <N>` | `-n` | Maximum results (default: 10) |
| `--context <N>` | `-C` | Number of context lines around matches (default: 0) |
| `--repo <NAME>` | `-r` | Filter to a specific repository |
