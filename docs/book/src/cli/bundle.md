---
title: bundle
description: Explore and manage context bundles — curated knowledge anchors
tags: [cli, bundle, context]
status: draft
category: cli-reference
related: [guides/bundles.md, cli/context.md, guides/tags.md]
commands: [bundle]
feature: bundles
source_files: [src/cli/bundle.rs, src/tags.rs]
---

# bundle

Explore and manage context bundles. Bundles are named, hierarchical groups of files, symbols, docs, and keywords that capture a concept or subsystem.

## Usage

```bash
bobbin bundle <COMMAND> [OPTIONS]
```

## Subcommands

### list

Show all bundles in a tree view (L0 map).

```bash
bobbin bundle list
bobbin bundle list --json
```

Output shows bundle hierarchy with names, descriptions, and member counts:

```text
Context Bundles (9 total):

  context — "Assembles relevant code for agent prompts" (1 files)
    ├── pipeline — "5-phase assembly: seed → coupling → bridge → filter → budget" (2 files)
    └── tags — "Tag-based scoring, pinning, and access control" (3 files)
  hook — "CLI injection into agent prompts" (1 files)
  search — "Hybrid semantic + keyword search" (1 files)
    └── lance — "LanceDB vector store backend" (1 files)
```

### show

Display a bundle's contents. L1 (outline) by default, L2 (full source) with `--deep`.

```bash
bobbin bundle show <NAME>           # L1: paths and symbol names
bobbin bundle show <NAME> --deep    # L2: full source code included
bobbin bundle show <NAME> --json    # JSON output
```

**L1 output** lists file paths, symbol references, docs, and keywords.

**L2 output** (`--deep`) reads and includes the full content of every ref and file — use this to bootstrap working context for a task.

### create

Create a new bundle.

```bash
bobbin bundle create <NAME> [OPTIONS]
bobbin bundle create <NAME> --global    # Store in ~/.config/bobbin/tags.toml
```

**Options:**

| Flag | Short | Description |
|------|-------|-------------|
| `--description` | `-d` | One-line description |
| `--keywords` | `-k` | Comma-separated trigger keywords |
| `--files` | `-f` | Comma-separated file paths |
| `--refs` | `-r` | Comma-separated `file::Symbol` references |
| `--docs` | | Comma-separated documentation file paths |
| `--includes` | `-i` | Comma-separated names of bundles to compose |
| `--global` | | Store in global config instead of per-repo |

**Examples:**

```bash
bobbin bundle create "search/reranking" --global \
  -d "Score normalization and result reranking" \
  -k "rerank,score,normalize" \
  -f "src/search/reranker.rs" \
  -r "src/search/reranker.rs::RerankerConfig"

bobbin bundle create "context/pipeline" --global \
  -d "5-phase assembly pipeline" \
  -f "src/search/context.rs,src/search/scorer.rs" \
  -i "tags"
```

### add

Add members to an existing bundle.

```bash
bobbin bundle add <NAME> [OPTIONS]
bobbin bundle add <NAME> --global -f "src/new_file.rs"
bobbin bundle add <NAME> --global -r "src/file.rs::NewSymbol"
bobbin bundle add <NAME> --global -k "new keyword"
bobbin bundle add <NAME> --global --docs "docs/new-guide.md"
```

Supports the same `-f`, `-r`, `-k`, `--docs` flags as `create`.

### remove

Remove members from a bundle, or delete the entire bundle.

```bash
bobbin bundle remove <NAME> --global -f "src/old_file.rs"
bobbin bundle remove <NAME> --global -r "src/file.rs::OldSymbol"
bobbin bundle remove <NAME> --global --all    # Delete entire bundle
```

## Global Options

| Flag | Description |
|------|-------------|
| `--json` | Output in JSON format |
| `--quiet` | Suppress non-essential output |
| `--verbose` | Show detailed progress |
| `--server <URL>` | Use remote bobbin server |
| `--role <ROLE>` | Role for access filtering |

## Storage

Bundles are defined as `[[bundles]]` entries in `tags.toml`:

- **Per-repo**: `.bobbin/tags.toml`
- **Global**: `~/.config/bobbin/tags.toml` (use `--global`)

Global bundles are recommended for concepts that span multiple repos.

## See Also

- [Context Bundles Guide](../guides/bundles.md) — workflow patterns, best practices
- [Tags & Effects](../guides/tags.md) — bundles share the tags.toml config
- [context](context.md) — context assembly uses bundle data
