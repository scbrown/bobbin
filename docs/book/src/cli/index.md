---
title: "index"
description: "Build or update the search index"
status: draft
category: cli-reference
tags: [cli, index]
commands: [index]
feature: index
source_files: [src/cli/index.rs]
---

# index

Build or update the search index. Walks repository files, parses them with Tree-sitter (or pulldown-cmark for Markdown), generates embeddings, and stores everything in LanceDB.

## Usage

```bash
bobbin index [PATH] [OPTIONS]
```

## Examples

```bash
bobbin index                           # Full index of current directory
bobbin index --incremental             # Only update changed files
bobbin index --force                   # Force reindex all files
bobbin index --repo myproject          # Tag chunks with a repository name
bobbin index --source /other/repo --repo other  # Index a different directory
```

## Options

| Flag | Short | Description |
|------|-------|-------------|
| `--incremental` | | Only update changed files |
| `--force` | | Force reindex all files |
| `--repo <NAME>` | | Repository name for multi-repo indexing (default: "default") |
| `--source <PATH>` | | Source directory to index files from (defaults to path) |
