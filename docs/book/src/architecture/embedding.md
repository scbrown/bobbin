---
title: "Embedding Pipeline"
description: "The chunking, parsing, and vector embedding pipeline"
tags: [architecture, embedding, chunking]
category: architecture
---

# Embedding Pipeline

Bobbin generates 384-dimensional vector embeddings locally using ONNX Runtime with the all-MiniLM-L6-v2 model. No data leaves your machine.

## Pipeline Overview

```text
Source File
    │
    ▼
┌────────────────┐
│ Tree-sitter /  │  Parse into semantic chunks
│ pulldown-cmark │  (functions, classes, sections, etc.)
└────────────────┘
    │
    ▼
┌────────────────┐
│ Context        │  Optionally enrich with surrounding lines
│ Enrichment     │  (configurable per language)
└────────────────┘
    │
    ▼
┌────────────────┐
│ ONNX Runtime   │  Generate 384-dim vectors
│ (MiniLM-L6-v2) │  Batched processing (default: 32)
└────────────────┘
    │
    ▼
┌────────────────┐
│ LanceDB        │  Store vectors + metadata
└────────────────┘
```

## Contextual Embedding

By default, chunks are embedded with their literal content. For configured languages (currently Markdown), Bobbin enriches each chunk with surrounding context lines before computing the embedding. This improves retrieval quality by giving the embedding model more semantic signal.

Configuration:

```toml
[embedding.context]
context_lines = 5              # Lines before/after each chunk
enabled_languages = ["markdown"] # Languages with context enrichment
```

## Model Details

| Property | Value |
|----------|-------|
| Model | all-MiniLM-L6-v2 |
| Dimensions | 384 |
| Runtime | ONNX Runtime (CPU) |
| Model location | `~/.cache/bobbin/models/` |
| Download | Automatic on first run |

## Batch Processing

Embeddings are generated in configurable batches (default: 32 chunks per batch) to balance throughput and memory usage. The batch size can be tuned in configuration:

```toml
[embedding]
batch_size = 32
```
