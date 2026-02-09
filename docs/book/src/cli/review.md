---
title: review
description: Assemble review context from a git diff
tags: [cli, review, context]
status: draft
category: cli-reference
related: [cli/context.md, cli/related.md]
commands: [review]
feature: review
source_files: [src/cli/review.rs]
---

# review

Assemble review context from a git diff.

## Synopsis

```bash
bobbin review [OPTIONS] [RANGE] [PATH]
```

## Description

The `review` command finds the indexed code chunks that overlap with changed lines from a git diff, then expands via temporal coupling. It returns a budget-aware context bundle annotated with which files were changed.

Use this to quickly understand what you need to review after changes are made.

## Arguments

| Argument | Description |
|----------|-------------|
| `RANGE` | Commit range (e.g., `HEAD~3..HEAD`) |
| `PATH` | Directory to search in (default: `.`) |

## Options

| Option | Short | Default | Description |
|--------|-------|---------|-------------|
| `--branch <BRANCH>` | `-b` | | Compare branch against main |
| `--staged` | | | Only staged changes |
| `--budget <LINES>` | | `500` | Maximum lines of context to include |
| `--depth <N>` | `-d` | `1` | Coupling expansion depth (0 = no coupling) |
| `--content <MODE>` | `-c` | | Content mode: `full`, `preview`, `none` |
| `--repo <NAME>` | `-r` | | Filter coupled files to specific repository |

## Examples

Review unstaged changes:

```bash
bobbin review
```

Review the last 3 commits:

```bash
bobbin review HEAD~3..HEAD
```

Review staged changes only:

```bash
bobbin review --staged
```

Review a feature branch against main:

```bash
bobbin review --branch feature/auth
```

JSON output:

```bash
bobbin review --json
```

## Prerequisites

Requires a bobbin index and a git repository. Run `bobbin init` and `bobbin index` first.

## See Also

- [context](context.md) — task-aware context assembly
- [related](related.md) — find temporally coupled files
