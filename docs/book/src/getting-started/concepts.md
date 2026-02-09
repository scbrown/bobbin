---
title: "Concepts"
description: "Core concepts: chunks, embeddings, hybrid search, coupling, and context assembly"
tags: [concepts, fundamentals]
category: getting-started
---

# Core Concepts

Bobbin's design is built around a few key ideas. Understanding them will help you get the most out of the tool.

## Chunks

A **chunk** is a semantic unit of code extracted from a source file. Rather than indexing entire files or arbitrary line ranges, bobbin uses [tree-sitter](https://tree-sitter.github.io/tree-sitter/) to parse source code into meaningful structural units:

| Chunk Type | Languages | Example |
|-----------|-----------|---------|
| `function` | Rust, TypeScript, Python, Go, Java, C++ | `fn parse_config(...)` |
| `method` | TypeScript, Java, C++ | `class.handleRequest()` |
| `class` | TypeScript, Python, Java, C++ | `class AuthService` |
| `struct` | Rust, Go, C++ | `struct Config` |
| `enum` | Rust, Java, C++ | `enum Status` |
| `interface` | TypeScript, Java | `interface Handler` |
| `trait` | Rust | `trait Serialize` |
| `impl` | Rust | `impl Config` |
| `module` | Rust | `mod auth` |
| `section` | Markdown | `## Architecture` |
| `table` | Markdown | Markdown tables |
| `code_block` | Markdown | Fenced code blocks |

Files that don't match a supported language fall back to **line-based chunking** (50 lines per chunk with 10-line overlap).

See [Chunk Types Reference](../reference/chunk-types.md) for the complete list.

## Embeddings

Each chunk is converted into a **384-dimensional vector** using the [all-MiniLM-L6-v2](https://huggingface.co/sentence-transformers/all-MiniLM-L6-v2) model, run locally via ONNX Runtime. These vectors capture semantic meaning — similar code produces similar vectors, even when the wording differs.

Bobbin supports **contextual embedding enrichment**: before computing a chunk's vector, it can prepend surrounding lines for additional context. This is configurable per language in `[embedding.context]`.

## Search Modes

Bobbin offers three search modes:

| Mode | How It Works | Best For |
|------|-------------|----------|
| **Hybrid** (default) | Combines semantic + keyword via RRF | General-purpose queries |
| **Semantic** | Vector similarity (ANN) only | Conceptual queries ("authentication logic") |
| **Keyword** | Full-text search (FTS) only | Exact identifiers ("handleRequest") |

**Reciprocal Rank Fusion (RRF)** merges the ranked results from both semantic and keyword search. The `semantic_weight` config (default: 0.7) controls the balance.

See [Search Modes Reference](../reference/search-modes.md) for details.

## Temporal Coupling

**Temporal coupling** measures how often two files change together in git history. If `auth.rs` and `middleware.rs` frequently appear in the same commits, they have high coupling — modifying one likely means you should look at the other.

Bobbin analyzes git history (configurable depth, default: 1000 commits) and stores coupling scores in SQLite. This data powers:

- `bobbin related <file>` — list files coupled to a given file
- `bobbin context <query>` — automatically expand search results with coupled files

## Context Assembly

The `context` command combines search and coupling into a single **context bundle**:

1. **Search**: Find chunks matching your query
2. **Expand**: Add temporally coupled files for each match
3. **Deduplicate**: Remove redundant chunks across files
4. **Budget**: Trim to fit a line budget (default: 500 lines)

The result is a focused set of code that's relevant to a task — ideal for feeding to an AI agent or understanding a change's scope.

## Hotspots

A **hotspot** is a file that is both frequently changed (high churn) and complex (high AST complexity). Hotspot score is the geometric mean of normalized churn and complexity. These files represent the riskiest parts of a codebase — they change often and are hard to change safely.

## Storage

Bobbin uses two storage backends:

| Store | Technology | Contents |
|-------|-----------|----------|
| **Primary** | LanceDB | Chunks, vector embeddings, full-text search index |
| **Metadata** | SQLite | Temporal coupling data, file metadata |

All data lives in `.bobbin/` within your repository. Nothing is sent externally.

## Next Steps

- [Agent Setup](agent-setup.md) — connect bobbin to AI coding tools
- [Searching Guide](../guides/searching.md) — advanced search techniques
- [Architecture Overview](../architecture/overview.md) — deeper dive into internals
