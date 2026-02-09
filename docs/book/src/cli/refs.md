---
title: "refs"
description: "Find symbol references and list file symbols"
category: cli-reference
tags: [cli, refs]
commands: [refs]
feature: refs
source_files: [src/cli/refs.rs]
---

# refs

Find symbol references and list symbols defined in a file.

## Synopsis

```bash
bobbin refs find [OPTIONS] <SYMBOL>
bobbin refs symbols [OPTIONS] <FILE>
```

## Description

The `refs` command has two subcommands:

- **`find`** — Locate where a symbol is defined and list all usages across the indexed codebase.
- **`symbols`** — List every symbol (functions, structs, traits, etc.) defined in a specific file.

Both subcommands query the vector store index built by `bobbin index`.

## Global Options

These options apply to both subcommands:

| Option | Short | Default | Description |
|--------|-------|---------|-------------|
| `--path <DIR>` | | `.` | Directory to search in |
| `--repo <NAME>` | `-r` | | Filter results to a specific repository |

## Subcommands

### `refs find`

Find the definition and usages of a symbol by name.

```bash
bobbin refs find [OPTIONS] <SYMBOL>
```

| Argument/Option | Short | Default | Description |
|-----------------|-------|---------|-------------|
| `<SYMBOL>` | | | Symbol name to find references for (required) |
| `--type <TYPE>` | `-t` | | Filter by symbol type (function, struct, trait, etc.) |
| `--limit <N>` | `-n` | `20` | Maximum number of usage results |

**Example:**

```bash
# Find all references to "Config"
bobbin refs find Config

# Find only struct definitions named "Config"
bobbin refs find --type struct Config

# Limit results to 5
bobbin refs find -n 5 parse_file
```

### `refs symbols`

List all symbols defined in a file.

```bash
bobbin refs symbols <FILE>
```

| Argument | Description |
|----------|-------------|
| `<FILE>` | File path to list symbols for (required) |

**Example:**

```bash
bobbin refs symbols src/main.rs

# With verbose output to see signatures
bobbin refs symbols --verbose src/config.rs
```

## JSON Output

### `refs find --json`

```json
{
  "symbol": "Config",
  "type": "struct",
  "definition": {
    "name": "Config",
    "chunk_type": "struct",
    "file_path": "src/config.rs",
    "start_line": 10,
    "end_line": 25,
    "signature": "pub struct Config { ... }"
  },
  "usage_count": 3,
  "usages": [
    {
      "file_path": "src/main.rs",
      "line": 5,
      "context": "use crate::config::Config;"
    }
  ]
}
```

### `refs symbols --json`

```json
{
  "file": "src/config.rs",
  "count": 4,
  "symbols": [
    {
      "name": "Config",
      "chunk_type": "struct",
      "start_line": 10,
      "end_line": 25,
      "signature": "pub struct Config { ... }"
    }
  ]
}
```

## Prerequisites

Requires a bobbin index. Run `bobbin init` and `bobbin index` first.

## See Also

- [deps](deps.md) — show import dependencies for a file
- [search](search.md) — semantic search across the codebase
