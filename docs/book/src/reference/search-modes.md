---
title: "Search Modes"
description: "Semantic search, keyword grep, and hybrid search modes explained"
tags: [reference, search, hybrid]
category: reference
---

# Search Modes

Bobbin supports three search modes, selectable via the `--mode` flag (CLI) or `mode` parameter (MCP).

## Hybrid (Default)

```bash
bobbin search "error handling"           # hybrid is the default
bobbin search "error handling" --mode hybrid
```

Hybrid search runs both semantic and keyword searches in parallel, then merges results using **Reciprocal Rank Fusion (RRF)**.

### How RRF Works

1. Run semantic search → get ranked list A
2. Run keyword search → get ranked list B
3. For each result, compute: `score = w / (k + rank_A) + (1 - w) / (k + rank_B)`
   - `w` = `semantic_weight` (default: 0.7)
   - `k` = smoothing constant (60)
4. Sort by combined score

Results that appear in both lists get boosted. Results unique to one list still appear but with lower scores.

### When to Use

Hybrid is the best default for most queries. It handles both conceptual queries ("functions that validate user input") and specific terms ("`parseConfig`") well.

## Semantic

```bash
bobbin search "authentication middleware" --mode semantic
```

Semantic search converts your query into a 384-dimensional vector using the same embedding model as the index, then finds the most similar code chunks via approximate nearest neighbor (ANN) search in LanceDB.

### Strengths

- Finds conceptually similar code even when wording differs
- "error handling" matches `catch`, `Result<T>`, `try/except`
- Good for exploratory queries when you don't know exact names

### Limitations

- May miss exact identifier matches that keyword search would find
- Requires the embedding model to be loaded (slight startup cost on first query)

## Keyword

```bash
bobbin search "handleRequest" --mode keyword
bobbin grep "handleRequest"              # grep always uses keyword mode
```

Keyword search uses LanceDB's full-text search (FTS) index. It matches tokens in chunk content and names.

### Strengths

- Fast, exact matching
- Finds specific identifiers, variable names, and error messages
- No embedding model needed

### Limitations

- No semantic understanding — "error handling" won't match "catch"
- Token-based, not substring-based (FTS tokenization rules apply)

## Comparison

| Feature | Hybrid | Semantic | Keyword |
|---------|--------|----------|---------|
| Conceptual matching | Yes | Yes | No |
| Exact identifier matching | Yes | Weak | Yes |
| Speed | Moderate | Moderate | Fast |
| Requires embeddings | Yes | Yes | No |
| Default mode | Yes | No | No |

## Configuration

The hybrid search balance is controlled by `semantic_weight` in `.bobbin/config.toml`:

```toml
[search]
semantic_weight = 0.7  # 0.0 = keyword only, 1.0 = semantic only
default_limit = 10
```

Higher values favor semantic results; lower values favor keyword matches. The default (0.7) works well for most codebases.

## grep vs search --mode keyword

Both use the same underlying FTS index. The differences:

| Feature | `bobbin grep` | `bobbin search --mode keyword` |
|---------|--------------|-------------------------------|
| Regex support | Yes (`--regex`) | No |
| Case-insensitive | Yes (`-i`) | No |
| Matching lines shown | Yes | No |
| Output format | Grep-style with line highlighting | Search-style with scores |
