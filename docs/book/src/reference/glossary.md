---
title: "Glossary"
description: "Definitions of key terms used throughout bobbin's documentation"
tags: [reference, glossary]
category: reference
---

# Glossary

## A

**ANN (Approximate Nearest Neighbor)**
: Vector similarity search algorithm used by LanceDB to find embeddings closest to a query vector. Trades exact precision for speed on large datasets.

## C

**Chunk**
: A semantic unit of code extracted from a source file — a function, class, struct, markdown section, etc. Chunks are the fundamental unit of indexing and search in bobbin. See [Chunk Types](chunk-types.md).

**Chunk Type**
: The structural category of a chunk: `function`, `method`, `class`, `struct`, `enum`, `interface`, `trait`, `impl`, `module`, `section`, `table`, `code_block`, `doc`, `commit`, or `other`.

**Context Assembly**
: The process of building a focused bundle of code relevant to a task. Combines search results with temporally coupled files, deduplicates, and trims to a line budget. See `bobbin context`.

**Context Budget**
: Maximum number of lines of code content included in a context bundle. Default: 500 lines.

**Contextual Embedding**
: Enriching a chunk with surrounding lines before computing its embedding vector. Improves search relevance by giving the embedding model more context about what the chunk does.

**Coupling**
: See *Temporal Coupling*.

## E

**Embedding**
: A fixed-length numerical vector (384 dimensions) that represents the semantic meaning of a chunk. Generated locally using the all-MiniLM-L6-v2 ONNX model.

## F

**FTS (Full-Text Search)**
: Token-based keyword search provided by LanceDB's built-in full-text search index. Powers keyword mode and `bobbin grep`.

## H

**Hotspot**
: A file with both high churn (frequently changed) and high complexity (complex AST structure). Hotspot score is the geometric mean of normalized churn and complexity. See `bobbin hotspots`.

**Hybrid Search**
: The default search mode that combines semantic and keyword search results via Reciprocal Rank Fusion (RRF). See [Search Modes](search-modes.md).

## L

**LanceDB**
: Embedded columnar vector database used as bobbin's primary storage. Stores chunks, embedding vectors, and the full-text search index.

**Line-Based Chunking**
: Fallback parsing strategy for unsupported file types. Splits files into chunks of 50 lines with 10-line overlap.

## M

**MCP (Model Context Protocol)**
: An open protocol for connecting AI assistants to external tools and data sources. Bobbin implements an MCP server that exposes its search and analysis capabilities. See [MCP Overview](../mcp/overview.md).

## O

**ONNX Runtime**
: Cross-platform inference engine used by bobbin to run the embedding model locally. No GPU required.

## P

**Primer**
: An LLM-friendly overview document of the bobbin project, shown via `bobbin prime`. Includes architecture, commands, and live index statistics.

## R

**Reciprocal Rank Fusion (RRF)**
: Algorithm for merging multiple ranked lists. Used by hybrid search to combine semantic and keyword results. Each result's score is based on its rank position in each list, weighted by `semantic_weight`.

## S

**Semantic Search**
: Search by meaning using vector similarity. Converts the query into an embedding and finds the most similar chunks via ANN search. See [Search Modes](search-modes.md).

**Semantic Weight**
: Configuration value (0.0–1.0) that controls the balance between semantic and keyword results in hybrid search. Default: 0.7 (favors semantic).

## T

**Temporal Coupling**
: A measure of how often two files change together in git history. Files with high coupling scores are likely related — changing one often means the other needs changes too. See `bobbin related`.

**Thin Client**
: CLI mode where bobbin forwards requests to a remote HTTP server instead of accessing local storage. Enabled via the `--server <URL>` global flag.

**Tree-sitter**
: Incremental parsing library used by bobbin to extract structural code elements (functions, classes, etc.) from source files. Supports Rust, TypeScript, Python, Go, Java, and C++.

## V

**Vector Store**
: The LanceDB database that holds chunk embeddings and supports both ANN search and full-text search. Located in `.bobbin/lance/`.
