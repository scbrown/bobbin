---
title: benchmark
description: Benchmark embedding model load times, latency, and throughput
tags: [cli, benchmark]
status: draft
category: cli-reference
related: [architecture/embedding.md, config/embedding.md]
commands: [benchmark]
feature: benchmark
source_files: [src/cli/benchmark.rs]
---

# benchmark

Benchmark embedding models to compare load times, single-query latency, and batch throughput.

## Synopsis

```bash
bobbin benchmark -q <QUERY> [OPTIONS] [PATH]
```

## Description

The `benchmark` command runs timed trials against one or more ONNX embedding models. For each model it measures:

- **Load time** — how long it takes to initialize the model.
- **Single embed** — per-query embedding latency (mean, min, max, p50, p95).
- **Batch embed** — latency for embedding all queries in a single batch.

If no `--model` is specified, all three built-in models are tested:

- `all-MiniLM-L6-v2`
- `bge-small-en-v1.5`
- `gte-small`

Models are automatically downloaded if not already cached.

## Arguments

| Argument | Default | Description |
|----------|---------|-------------|
| `[PATH]` | `.` | Directory containing `.bobbin/` config |

## Options

| Option | Short | Default | Description |
|--------|-------|---------|-------------|
| `--query <TEXT>` | `-q` | | Queries to benchmark (required, can be repeated) |
| `--model <NAME>` | `-m` | all built-in | Models to compare (can be repeated) |
| `--iterations <N>` | | `5` | Number of iterations per query |
| `--batch-size <N>` | | `32` | Batch size for embedding |

## Examples

Benchmark with a single query:

```bash
bobbin benchmark -q "authentication middleware"
```

Compare two models with multiple queries:

```bash
bobbin benchmark \
  -q "error handling" \
  -q "database connection pool" \
  -m all-MiniLM-L6-v2 \
  -m bge-small-en-v1.5 \
  --iterations 10
```

JSON output for programmatic comparison:

```bash
bobbin benchmark -q "test query" --json
```

## JSON Output

```json
{
  "models": [
    {
      "model": "all-MiniLM-L6-v2",
      "dimension": 384,
      "load_time_ms": 45.2,
      "embed_single": {
        "mean_ms": 3.12,
        "min_ms": 2.80,
        "max_ms": 4.01,
        "p50_ms": 3.05,
        "p95_ms": 3.90
      },
      "embed_batch": {
        "mean_ms": 8.45,
        "min_ms": 7.90,
        "max_ms": 9.10,
        "p50_ms": 8.40,
        "p95_ms": 9.05
      }
    }
  ],
  "queries": ["test query"],
  "iterations": 5
}
```

## See Also

- [Embedding Settings](../config/embedding.md) — configure which model bobbin uses
- [index](index.md) — build the search index using the configured model
