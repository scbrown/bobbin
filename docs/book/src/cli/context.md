---
title: context
description: Assemble task-relevant context from search and git history
tags: [cli, context]
status: draft
category: cli-reference
related: [guides/context-assembly.md, cli/search.md]
commands: [context]
feature: context
source_files: [src/cli/context.rs]
---

# context

Assemble task-relevant context from search results and git history. Searches for code matching your query, then expands results with temporally coupled files (files that change together in git history). Outputs a context bundle optimized for feeding to AI agents or for understanding a task's scope.

## Usage

```bash
bobbin context <QUERY> [OPTIONS]
```

## Examples

```bash
bobbin context "fix the login bug"                   # Default: 500 line budget
bobbin context "refactor auth" --budget 1000          # Larger context budget
bobbin context "add tests" --content full             # Include full code content
bobbin context "auth" --content none                  # Paths/metadata only
bobbin context "auth" --depth 0                       # No coupling expansion
bobbin context "auth" --repo myproject --json         # JSON output for a specific repo
```

## Options

| Flag | Short | Description |
|------|-------|-------------|
| `--budget <LINES>` | `-b` | Maximum lines of content to include (default: 500) |
| `--content <MODE>` | `-c` | Content mode: `full`, `preview` (default for terminal), `none` |
| `--depth <N>` | `-d` | Coupling expansion depth, 0 = no coupling (default: 1) |
| `--max-coupled <N>` | | Max coupled files per seed file (default: 3) |
| `--limit <N>` | `-n` | Max initial search results (default: 20) |
| `--coupling-threshold <F>` | | Min coupling score threshold (default: 0.1) |
| `--repo <NAME>` | `-r` | Filter to a specific repository |

## Context Bundle

The context bundle includes:

- **Direct matches**: Code chunks matching your query, ranked by relevance
- **Coupled files**: Files with shared commit history to the direct matches
- **Budget tracking**: How many lines were used out of the budget
- **File metadata**: Paths, chunk types, line ranges, relevance scores
