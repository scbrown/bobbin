# Bobbin v2: Unified Search Engine

## Overview

Extend bobbin to be a unified search solution supporting multiple repositories, better document chunking, contextual embeddings, and centralized HTTP server deployment.

## Goals

1. **LanceDB-primary storage** - Consolidate metadata into LanceDB, keep SQLite only for relational data (git coupling)
2. **Multi-repo indexing** - Index multiple git repos, search across all or filter by repo
3. **Semantic markdown chunking** - Respect document structure (headings, tables, code blocks)
4. **Contextual embeddings** - Toggleable per content type (on for docs, off for code)
5. **HTTP server mode** - Centralized deployment with webhook support for CI integration

## Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                   bobbin server                             │
│  ┌──────────┐ ┌──────────┐ ┌──────────┐ ┌───────────┐      │
│  │ Webhook  │ │   MCP    │ │  HTTP    │ │  Indexer  │      │
│  │ Handler  │ │ Server   │ │   API    │ │  Engine   │      │
│  └──────────┘ └──────────┘ └──────────┘ └───────────┘      │
│                            │                                │
│           ┌────────────────┼────────────────┐              │
│           ▼                ▼                ▼              │
│    ┌───────────┐    ┌───────────┐    ┌───────────┐        │
│    │  LanceDB  │    │  SQLite   │    │   ONNX    │        │
│    │ (vectors  │    │   (git    │    │  (embed   │        │
│    │ +metadata)│    │ coupling) │    │   model)  │        │
│    └───────────┘    └───────────┘    └───────────┘        │
└─────────────────────────────────────────────────────────────┘
```

## LanceDB Schema

```
Table: chunks
├── id: string              # SHA256(repo:file_path:start_line:end_line)
├── vector: float[384]      # MiniLM embedding
├── repo: string            # repository name
├── file_path: string       # relative path within repo
├── file_hash: string       # SHA256 of file content
├── language: string        # rust, python, markdown, etc.
├── chunk_type: string      # function, class, section, table, etc.
├── name: string            # function name, heading text, etc.
├── start_line: uint32
├── end_line: uint32
├── content: string         # chunk text for display
├── full_context: string?   # chunk + surrounding context (nullable)
└── indexed_at: timestamp
```

## SQLite Schema (git coupling only)

```sql
CREATE TABLE coupling (
    repo TEXT NOT NULL,
    file_a TEXT NOT NULL,
    file_b TEXT NOT NULL,
    co_changes INTEGER NOT NULL,
    score REAL NOT NULL,
    PRIMARY KEY (repo, file_a, file_b)
);

CREATE TABLE repo_state (
    repo TEXT PRIMARY KEY,
    last_commit TEXT,
    indexed_at TEXT
);
```

## Chunking Strategy

| Content Type | Chunker | Contextual Embedding |
|--------------|---------|---------------------|
| Rust, Go, Python, TS | tree-sitter AST | OFF |
| YAML, TOML, JSON | Structure-aware | OFF |
| Markdown | Semantic (headings, tables) | ON |
| Plain text | Sliding window | ON |

### Markdown Semantic Chunker

- Chunk at heading boundaries (respecting hierarchy)
- Keep tables as atomic units
- Keep fenced code blocks atomic
- Extract YAML frontmatter for metadata

## Contextual Embeddings

For document types, embed chunk with surrounding context:

```
embed(context_before + chunk_content + context_after)
```

Configuration:
```toml
[embedding.contextual]
enabled_for = ["markdown", "text", "rst"]
disabled_for = ["rust", "python", "go", "yaml"]
context_lines = 10
```

## API

### CLI Commands

```bash
bobbin index --repo NAME --url URL    # Add repo to index
bobbin search "query"                  # Search all repos
bobbin search --repo NAME "query"      # Search specific repo
bobbin search --server HOST "query"    # Thin client mode
bobbin status                          # Index statistics
```

### HTTP Endpoints

```
GET  /search?q=...&repo=...&limit=...
GET  /chunk/{id}
GET  /status
POST /webhook/push
POST /index/{repo}
```

### MCP Tools

```
search(query, repo?, type?, limit?)
read_chunk(chunk_id)
related_files(file_path, repo)
index_status()
```

## Implementation Phases

See beads: bobbin-cmb (epic) and children bobbin-cmb.1 through bobbin-cmb.5

1. **bobbin-cmb.1**: LanceDB Consolidation
2. **bobbin-cmb.2**: Multi-Repo Support
3. **bobbin-cmb.3**: Markdown Semantic Chunker
4. **bobbin-cmb.4**: Contextual Embeddings
5. **bobbin-cmb.5**: HTTP Server Mode

## References

- [Search comparison doc](https://git.lan/stiwi/goldblum/docs/search.md)
- [Goldblum integration plan](https://git.lan/stiwi/goldblum/docs/architecture/bobbin-v2-unified-search.md)
