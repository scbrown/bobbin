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

## Semantic Weight

The `semantic_weight` parameter controls how hybrid search blends results:

- **1.0** = pure semantic search (vector similarity only)
- **0.0** = pure keyword search (full-text search only)
- **0.7** (default) = heavily favors semantic matches, with keyword results filling in exact-match gaps

The hybrid search uses Reciprocal Rank Fusion (RRF) to combine results from both search modes. See [Architecture: Storage & Data Flow](../architecture/storage.md) for details on the RRF algorithm.
