# Bobbin Architecture

## Overview

Bobbin is a local-first code context engine built in Rust. It provides semantic and keyword search over codebases using:

- **Tree-sitter** for structural code parsing (Rust, TypeScript, Python, Go, Java, C++)
- **pulldown-cmark** for semantic markdown parsing (sections, tables, code blocks, frontmatter)
- **ONNX Runtime** for local embedding generation (all-MiniLM-L6-v2)
- **LanceDB** for primary storage: chunks, vector embeddings, and full-text search
- **SQLite** for temporal coupling data and global metadata
- **rmcp** for MCP server integration with AI agents

## Module Structure

```
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

```
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

```
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

## Key Types

### Chunk

A semantic unit extracted from source code:

```rust
struct Chunk {
    id: String,           // SHA256-based unique ID
    file_path: String,    // Source file path
    chunk_type: ChunkType,// function, class, struct, etc.
    name: Option<String>, // Function/class name
    start_line: u32,      // Starting line number
    end_line: u32,        // Ending line number
    content: String,      // Actual code content
    language: String,     // Programming language
}
```

### ChunkType

```rust
enum ChunkType {
    Function,   // Standalone functions
    Method,     // Class methods
    Class,      // Class definitions
    Struct,     // Struct definitions (Rust, C++, Go)
    Enum,       // Enum definitions
    Interface,  // Interface definitions (TS, Java)
    Module,     // Module definitions
    Impl,       // Impl blocks (Rust)
    Trait,      // Trait definitions (Rust)
    Doc,        // Documentation chunks (Markdown frontmatter, preamble)
    Section,    // Heading-delimited sections (Markdown)
    Table,      // Table elements (Markdown)
    CodeBlock,  // Fenced code blocks (Markdown)
    Other,      // Fallback for line-based chunks
}
```

### SearchResult

```rust
struct SearchResult {
    chunk: Chunk,              // The matched chunk
    score: f32,                // Relevance score
    match_type: Option<MatchType>, // How it was matched
}
```

## Storage Schema

### LanceDB (Primary Storage)

All chunk data, embeddings, and full-text search live in LanceDB:

```
chunks table:
  - id: string            # SHA256-based unique chunk ID
  - vector: float[384]    # MiniLM embedding
  - repo: string          # Repository name (for multi-repo support)
  - file_path: string     # Relative file path
  - file_hash: string     # Content hash for incremental indexing
  - language: string      # Programming language
  - chunk_type: string    # function, method, class, section, etc.
  - chunk_name: string?   # Function/class/section name (nullable)
  - start_line: uint32    # Starting line number
  - end_line: uint32      # Ending line number
  - content: string       # Original chunk content
  - full_context: string? # Context-enriched text used for embedding (nullable)
  - indexed_at: string    # Timestamp
```

LanceDB also maintains an FTS index on the `content` field for keyword search.

### SQLite (Auxiliary)

SQLite only stores temporal coupling data and global metadata:

```sql
-- Temporal coupling (git co-change relationships)
CREATE TABLE coupling (
    file_a TEXT NOT NULL,
    file_b TEXT NOT NULL,
    score REAL,
    co_changes INTEGER,
    last_co_change INTEGER,
    PRIMARY KEY (file_a, file_b)
);

-- Global metadata
CREATE TABLE meta (
    key TEXT PRIMARY KEY,
    value TEXT
);
```

## Configuration

Default configuration stored in `.bobbin/config.toml`:

```toml
[index]
include = ["**/*.rs", "**/*.ts", "**/*.tsx", "**/*.js", "**/*.jsx", "**/*.py", "**/*.go", "**/*.md"]
exclude = ["**/node_modules/**", "**/target/**", "**/dist/**", "**/.git/**", "**/build/**", "**/__pycache__/**"]
use_gitignore = true

[embedding]
model = "all-MiniLM-L6-v2"
batch_size = 32

[embedding.context]
context_lines = 5
enabled_languages = ["markdown"]

[search]
default_limit = 10
semantic_weight = 0.7

[git]
coupling_enabled = true
coupling_depth = 1000
coupling_threshold = 3
```

## Hybrid Search (RRF)

The hybrid search combines semantic (vector) and keyword (FTS) results using Reciprocal Rank Fusion:

```
RRF_score = semantic_weight / (k + semantic_rank) + keyword_weight / (k + keyword_rank)
```

Where:
- `k = 60` (standard RRF constant)
- `semantic_weight` from config (default 0.7)
- `keyword_weight = 1 - semantic_weight`

Results that appear in both searches get boosted scores and are marked as `[hybrid]` matches.

## CLI Commands

| Command | Description |
|---------|-------------|
| `bobbin init` | Initialize bobbin in current repository |
| `bobbin index` | Build/rebuild the search index |
| `bobbin index --repo <name>` | Index with multi-repo tagging |
| `bobbin search <query>` | Hybrid search (combines semantic + keyword) |
| `bobbin search --mode semantic` | Semantic-only vector search |
| `bobbin search --mode keyword` | Keyword-only FTS search |
| `bobbin search --repo <name>` | Search within a specific repository |
| `bobbin grep <pattern>` | Keyword/regex search with highlighting |
| `bobbin related <file>` | Find files related to a given file |
| `bobbin history <file>` | Show commit history and churn statistics |
| `bobbin status` | Show index statistics |
| `bobbin serve` | Start MCP server for AI agent integration |

Global flags: `--json`, `--quiet`, `--verbose`
