---
title: "related"
description: "Find files related to a given file via git coupling analysis"
status: draft
category: cli-reference
tags: [cli, related]
commands: [related]
feature: related
source_files: [src/cli/related.rs]
---

# related

Find files that are temporally coupled to a given file -- files that frequently change together in git history.

## Usage

```bash
bobbin related <FILE> [OPTIONS]
```

## Examples

```bash
bobbin related src/auth.rs
bobbin related src/auth.rs --limit 20
bobbin related src/auth.rs --threshold 0.5   # Only strong coupling
```

## Options

| Flag | Short | Description |
|------|-------|-------------|
| `--limit <N>` | `-n` | Maximum results (default: 10) |
| `--threshold <F>` | | Minimum coupling score (default: 0.0) |
