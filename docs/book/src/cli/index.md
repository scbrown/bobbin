---
title: index
description: Build or update the search index
tags: [cli, index]
status: draft
category: cli-reference
related: [cli/watch.md, config/index.md]
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

## Maintenance and recovery

Each index run ends with a Lance maintenance sweep (pruning old table versions and compacting fragments). Two properties of that sweep are worth knowing when you run `bobbin index` on a schedule:

- **Maintenance failures fail the command.** A sweep that errors makes `bobbin index` exit non-zero instead of reporting success with a silently skipped compaction. Lock contention is the one non-error outcome: a sweep starved by another process's lock is reported loudly on stderr (even under `--quiet` and `--json`) and labeled `skipped_lock_held` in `--json` output.
- **FTS compaction panics recover truthfully.** Lance's incremental full-text-index remap can panic while compacting the chunks table. When exactly that failure is detected, bobbin rebuilds the FTS index from scratch — discarding the broken incremental generation — and retries the compaction once. Unrelated compaction errors (I/O, schema, out of memory) are never treated as rebuildable and surface as-is.
