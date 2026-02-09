---
title: search
description: Hybrid, semantic, and keyword search across your codebase
tags: [cli, search]
status: draft
category: cli-reference
related: [cli/grep.md, guides/searching.md, reference/search-modes.md]
commands: [search]
feature: search
source_files: [src/cli/search.rs]
---

# search

Search across your codebase. By default uses hybrid search combining semantic (vector similarity) and keyword (FTS) results using Reciprocal Rank Fusion (RRF).

## Usage

```bash
bobbin search <QUERY> [OPTIONS]
```

## Examples

```bash
bobbin search "error handling"                    # Hybrid search (default)
bobbin search "database connection" --limit 20    # More results
bobbin search "auth" --type function              # Filter by chunk type
bobbin search "auth" --mode semantic              # Semantic-only search
bobbin search "handleAuth" --mode keyword         # Keyword-only search
bobbin search "auth" --repo myproject             # Search within a specific repo
```

## Options

| Flag | Short | Description |
|------|-------|-------------|
| `--type <TYPE>` | `-t` | Filter by chunk type (function, method, class, struct, enum, interface, module, impl, trait, doc, section, table, `code_block`) |
| `--limit <N>` | `-n` | Maximum results (default: 10) |
| `--mode <MODE>` | `-m` | Search mode: `hybrid` (default), `semantic`, or `keyword` |
| `--repo <NAME>` | `-r` | Filter to a specific repository |

## Search Modes

| Mode | Description |
|------|-------------|
| `hybrid` | Combines semantic + keyword using RRF (default) |
| `semantic` | Vector similarity search only |
| `keyword` | Full-text keyword search only |
