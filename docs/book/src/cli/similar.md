---
title: similar
description: Find semantically similar code or scan for duplicate clusters
tags: [cli, similar, duplicates]
status: draft
category: cli-reference
related: [cli/search.md, cli/grep.md]
commands: [similar]
feature: similar
source_files: [src/cli/similar.rs]
---

# similar

Find semantically similar code chunks or scan for near-duplicate clusters.

## Synopsis

```bash
bobbin similar [OPTIONS] [TARGET]
bobbin similar --scan [OPTIONS]
```

## Description

The `similar` command uses vector similarity to find code that is semantically close to a target, or to scan the entire codebase for duplicate/near-duplicate clusters.

**Single-target mode:** Provide a chunk reference (`file.rs:function_name`) or free text to find similar code.

**Scan mode:** Set `--scan` to detect duplicate/near-duplicate code clusters across the codebase.

## Options

| Option | Short | Default | Description |
|--------|-------|---------|-------------|
| `--scan` | | | Scan entire codebase for near-duplicate clusters |
| `--threshold <SCORE>` | `-t` | `0.85` | Minimum cosine similarity threshold |
| `--limit <N>` | `-n` | `10` | Maximum number of results or clusters |
| `--repo <NAME>` | `-r` | | Filter to a specific repository |
| `--cross-repo` | | | In scan mode, compare chunks across different repos |
| `--persist` | | | In scan mode, persist the threshold-gated pairs as `similar_to` chunk edges |

## Examples

Find code similar to a specific function:

```bash
bobbin similar "src/search/hybrid.rs:search"
```

Find code similar to a free-text description:

```bash
bobbin similar "error handling with retries"
```

Scan the codebase for near-duplicates:

```bash
bobbin similar --scan
```

Lower the threshold for broader matches:

```bash
bobbin similar --scan --threshold 0.7
```

JSON output:

```bash
bobbin similar --scan --json
```

Persist the scan's near-duplicate pairs as chunk edges:

```bash
bobbin similar --scan --persist --threshold 0.9
```

## Persisted `similar_to` edges

With `--persist`, every threshold-gated near-duplicate pair found by the scan
is written to the `chunk_edges` table as a `similar_to` edge — the same table
the deterministic structural edges (`next_chunk`, `part_of`) live in — and
becomes queryable through the `chunk_neighbors` MCP tool and HTTP endpoint
like any other edge type.

Persistence is opt-in and **replaces** rather than accumulates: each persist
run clears the previous `similar_to` set (scoped to `--repo` when given, all
repos otherwise) before writing, so re-running a scan converges to the same
edges instead of duplicating them, and pairs that no longer clear the
threshold disappear. Edge direction is normalized (lexically smaller chunk ID
is the source) since similarity is symmetric; the table stores edge presence,
not the score — presence means the pair cleared the scan threshold at persist
time.

## Prerequisites

Requires a bobbin index. Run `bobbin init` and `bobbin index` first.

## See Also

- [search](search.md) — semantic and hybrid search
- [grep](grep.md) — keyword/regex search
