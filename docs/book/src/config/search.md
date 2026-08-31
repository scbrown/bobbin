---
title: Search Settings
description: Configuring search defaults, thresholds, and result limits
tags: [config, search]
status: draft
category: config
related: [cli/search.md, config/reference.md, reference/search-modes.md]
---

# Search Settings

The `[search]` section controls search behavior defaults.

## Configuration

```toml
[search]
default_limit = 10
semantic_weight = 0.7
```

## Options

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `default_limit` | int | `10` | Default number of results returned |
| `semantic_weight` | float | `0.7` | Balance between semantic (1.0) and keyword (0.0) in hybrid mode |
| `reranker` | table | absent | Opt-in cross-encoder reranking stage. See below. |

## Semantic Weight

The `semantic_weight` parameter controls how hybrid search blends results:

- **1.0** = pure semantic search (vector similarity only)
- **0.0** = pure keyword search (full-text search only)
- **0.7** (default) = heavily favors semantic matches, with keyword results filling in exact-match gaps

The hybrid search uses Reciprocal Rank Fusion (RRF) to combine results from both search modes. See [Architecture: Storage & Data Flow](../architecture/storage.md) for details on the RRF algorithm.

## Cross-encoder reranking (opt-in)

Absent by default. When configured, the top-K hybrid results are rescored by
a local cross-encoder ONNX model after RRF fusion and before the final
truncation:

```toml
[search.reranker]
model_path = "/models/cross-encoder.onnx"   # required, user-supplied
tokenizer_path = "/models/tokenizer.json"   # required, user-supplied
max_seq_len = 512                            # (query, passage) pair budget
top_k = 50                                   # results rescored per query
rerank_weight = 1.0                          # 1.0 = reranker replaces top-K order
```

- **Bobbin never downloads reranker models.** Point the paths at a
  cross-encoder exported to ONNX with a single relevance logit per
  (query, passage) pair. Missing paths refuse loudly at startup (`bobbin
  serve`) or at first hybrid search — never a silent downgrade.
- **Blend rule:** reranker logits are sigmoid-squashed to (0, 1); the K fused
  scores are min-max normalized; the final score is
  `rerank_weight * sigmoid(logit) + (1 - rerank_weight) * fused_norm`. At the
  default `rerank_weight = 1.0` the reranker replaces the ordering within the
  top-K. Results beyond `top_k` keep their fused scores and positions.
- Applies to the hybrid mode of `bobbin search`, the HTTP `/search` endpoint,
  and the MCP `search` tool. Specialized lanes (beads, commits, archive
  search) are not reranked.
- **Status note:** the reranking stage is unit-tested with deterministic fake
  scorers; the ONNX model-in-the-loop path compiles and is seam-tested but has
  not been validated against a real cross-encoder model — treat retrieval
  quality as unmeasured until the eval harness runs it. Keep it off in
  production until then.
