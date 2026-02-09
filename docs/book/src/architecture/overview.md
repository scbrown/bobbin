---
title: "Architecture Overview"
description: "High-level architecture of bobbin's indexing, search, and context pipeline"
tags: [architecture, overview]
category: architecture
---

# Architecture Overview

Bobbin is a local-first code context engine built in Rust. It provides semantic and keyword search over codebases using:

- **Tree-sitter** for structural code parsing (Rust, TypeScript, Python, Go, Java, C++)
- **pulldown-cmark** for semantic markdown parsing (sections, tables, code blocks, frontmatter)
- **ONNX Runtime** for local embedding generation (all-MiniLM-L6-v2)
- **LanceDB** for primary storage: chunks, vector embeddings, and full-text search
- **SQLite** for temporal coupling data and global metadata
- **rmcp** for MCP server integration with AI agents

## Module Structure

```text
src/
├── main.rs           # Entry point, CLI initialization
├── config.rs         # Configuration management (.bobbin/config.toml)
├── types.rs          # Shared types (Chunk, SearchResult, etc.)
│
├── cli/              # Command-line interface
│   ├── mod.rs        # Command dispatcher
│   ├── init.rs       # Initialize bobbin in a repository
│   ├── index.rs      # Build/update the search index
│   ├── search.rs     # Semantic search command
│   ├── grep.rs       # Keyword/regex search command
│   ├── related.rs    # Find related files command
│   ├── history.rs    # File commit history and churn statistics
│   ├── status.rs     # Index status and statistics
│   └── serve.rs      # Start MCP server
│
├── index/            # Indexing engine
│   ├── mod.rs        # Module exports
│   ├── parser.rs     # Tree-sitter + pulldown-cmark code parsing
│   ├── embedder.rs   # ONNX embedding generation
│   └── git.rs        # Git history analysis (temporal coupling)
│
├── mcp/              # MCP (Model Context Protocol) server
│   ├── mod.rs        # Module exports
│   ├── server.rs     # MCP server implementation
│   └── tools.rs      # Tool request/response types (search, grep, related, read_chunk)
│
├── search/           # Query engine
│   ├── mod.rs        # Module exports
│   ├── semantic.rs   # Vector similarity search (LanceDB ANN)
│   ├── keyword.rs    # Full-text search (LanceDB FTS)
│   └── hybrid.rs     # Combined search with RRF
│
└── storage/          # Persistence layer
    ├── mod.rs        # Module exports
    ├── lance.rs      # LanceDB: chunks, vectors, FTS (primary storage)
    └── sqlite.rs     # SQLite: temporal coupling + global metadata
```

## Data Flow

### Indexing Pipeline

```text
Repository Files
      │
      ▼
┌─────────────┐
│ File Walker │ (respects .gitignore)
└─────────────┘
      │
      ▼
┌────────────────┐
│ Tree-sitter /  │ → Extract semantic chunks (functions, classes, sections, etc.)
│ pulldown-cmark │
└────────────────┘
      │
      ▼
┌─────────────┐
│  Embedder   │ → Generate 384-dim vectors via ONNX
│   (ONNX)    │   (with optional contextual enrichment)
└─────────────┘
      │
      ▼
┌─────────────┐
│  LanceDB    │ → Store chunks, vectors, metadata, and FTS index
│ (primary)   │
└─────────────┘
```

### Query Pipeline

```text
User Query
      │
      ▼
┌─────────────┐
│  Embedder   │ → Query embedding
└─────────────┘
      │
      ├────────────────────┐
      ▼                    ▼
┌─────────────┐      ┌─────────────┐
│  LanceDB    │      │ LanceDB FTS │
│  (ANN)      │      │ (keyword)   │
└─────────────┘      └─────────────┘
      │                    │
      └────────┬───────────┘
               ▼
        ┌─────────────┐
        │ Hybrid RRF  │ → Reciprocal Rank Fusion
        └─────────────┘
               │
               ▼
          Results
```

See also: [Storage & Data Flow](storage.md) | [Embedding Pipeline](embedding.md) | [Language Support](languages.md)
