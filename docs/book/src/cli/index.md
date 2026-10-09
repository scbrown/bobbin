---
title: index
description: Build or update the search index
tags: [cli, index]
status: draft
category: cli-reference
related: [cli/watch.md, config/index.md]
commands: [index]
feature: index
source_files: [src/cli/index.rs, src/cli/index_encoding.rs]
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
| `--include-beads` | | Also index beads (issues) from Dolt |

To reindex one bead without re-fetching the whole bead corpus, use
[`index-bead`](index-bead.md).

## Maintenance and recovery

Each index run ends with a Lance maintenance sweep (pruning old table versions and compacting fragments). Two properties of that sweep are worth knowing when you run `bobbin index` on a schedule:

- **Maintenance failures fail the command.** A sweep that errors makes `bobbin index` exit non-zero instead of reporting success with a silently skipped compaction. Lock contention is the one non-error outcome: a sweep starved by another process's lock is reported loudly on stderr (even under `--quiet` and `--json`) and labeled `skipped_lock_held` in `--json` output.
- **FTS compaction panics recover truthfully.** Lance's incremental full-text-index remap can panic while compacting the chunks table. When exactly that failure is detected, bobbin rebuilds the FTS index from scratch — discarding the broken incremental generation — and retries the compaction once. Unrelated compaction errors (I/O, schema, out of memory) are never treated as rebuildable and surface as-is.

## Text encodings and file errors

Source text is read as UTF-8. When its bytes are not valid UTF-8, indexing falls
back to ISO-8859-1 (Latin-1), preserving each byte as its corresponding Unicode
code point. This is a fixed fallback, not automatic encoding detection; convert
other legacy encodings to UTF-8 when exact text matters. Valid UTF-8 is unchanged.
Document and PDF extraction retain their existing decoding behavior.

Individual I/O or extraction failures are skipped and reported by path in the
run summary and counted in the JSON `errors` field. Storage and maintenance
failures still fail the command.
