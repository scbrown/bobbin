---
title: "Embedding Settings"
description: "Configuring the embedding model, batch size, and ONNX runtime"
tags: [config, embedding, onnx]
category: config
---

# Embedding Settings

The `[embedding]` and `[embedding.context]` sections control embedding model and batch processing.

## Configuration

```toml
[embedding]
model = "all-MiniLM-L6-v2"
batch_size = 32

[embedding.context]
context_lines = 5
enabled_languages = ["markdown"]
```

## `[embedding]` Options

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `model` | string | `"all-MiniLM-L6-v2"` | ONNX embedding model name. Downloaded to `~/.cache/bobbin/models/` on first use. |
| `batch_size` | int | `32` | Number of chunks to embed per batch |

## `[embedding.context]` Options

Controls contextual embedding, where chunks are embedded with surrounding source lines for better retrieval.

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `context_lines` | int | `5` | Lines of context before and after each chunk |
| `enabled_languages` | string[] | `["markdown"]` | Languages where contextual embedding is active |

## Notes

- The embedding model is downloaded automatically on first run and cached in `~/.cache/bobbin/models/`.
- Contextual embedding enriches each chunk with surrounding lines before computing its vector, improving search relevance at the cost of slightly longer indexing time.
- Increasing `batch_size` may improve indexing throughput but uses more memory.
