# Bobbin Architecture

## Overview

Bobbin is a local-first code context engine built in Rust. It provides semantic and keyword search over codebases using:

- **Tree-sitter** for structural code parsing
- **ONNX Runtime** for local embedding generation (all-MiniLM-L6-v2)
- **LanceDB** for vector storage and similarity search
- **SQLite** for metadata and full-text search

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
│   └── status.rs     # Index status and statistics
│
├── index/            # Indexing engine
│   ├── mod.rs        # Module exports
│   ├── parser.rs     # Tree-sitter code parsing
│   ├── embedder.rs   # ONNX embedding generation
│   └── git.rs        # Git history analysis (temporal coupling)
│
├── search/           # Query engine
│   ├── mod.rs        # Module exports
│   ├── semantic.rs   # Vector similarity search
│   ├── keyword.rs    # SQLite FTS search
│   └── hybrid.rs     # Combined search with RRF
│
└── storage/          # Persistence layer
    ├── mod.rs        # Module exports
    ├── lance.rs      # LanceDB vector operations
    └── sqlite.rs     # SQLite metadata and FTS
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
┌─────────────┐
│ Tree-sitter │ → Extract semantic chunks (functions, classes, etc.)
│   Parser    │
└─────────────┘
      │
      ▼
┌─────────────┐
│  Embedder   │ → Generate 384-dim vectors via ONNX
│   (ONNX)    │
└─────────────┘
      │
      ├────────────────┬────────────────┐
      ▼                ▼                ▼
┌─────────────┐  ┌─────────────┐  ┌─────────────┐
│  LanceDB    │  │   SQLite    │  │   SQLite    │
│  (vectors)  │  │ (metadata)  │  │   (FTS)     │
└─────────────┘  └─────────────┘  └─────────────┘
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
│  LanceDB    │      │ SQLite FTS  │
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
    Struct,     // Struct definitions (Rust)
    Enum,       // Enum definitions
    Interface,  // Interface definitions (TS)
    Module,     // Module definitions
    Impl,       // Impl blocks (Rust)
    Trait,      // Trait definitions (Rust)
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

### SQLite Tables

```sql
-- Indexed files
CREATE TABLE files (
    id INTEGER PRIMARY KEY,
    path TEXT UNIQUE NOT NULL,
    language TEXT,
    mtime INTEGER,
    hash TEXT,
    indexed_at INTEGER
);

-- Semantic chunks
CREATE TABLE chunks (
    id TEXT PRIMARY KEY,
    file_id INTEGER REFERENCES files(id),
    chunk_type TEXT,
    name TEXT,
    start_line INTEGER,
    end_line INTEGER,
    content TEXT,
    vector_id TEXT
);

-- Full-text search (FTS5)
CREATE VIRTUAL TABLE chunks_fts USING fts5(
    content, name,
    content='chunks'
);

-- Temporal coupling (git forensics)
CREATE TABLE coupling (
    file_a INTEGER,
    file_b INTEGER,
    score REAL,
    co_changes INTEGER,
    last_co_change INTEGER,
    PRIMARY KEY (file_a, file_b)
);
```

### LanceDB Schema

```
vectors table:
  - id: string        (matches chunks.id)
  - vector: float[384] (MiniLM embedding)
  - file_path: string
  - chunk_name: string
```

## Configuration

Default configuration stored in `.bobbin/config.toml`:

```toml
[index]
include = ["**/*.rs", "**/*.ts", "**/*.py", "**/*.go", "**/*.md"]
exclude = ["**/node_modules/**", "**/target/**"]
use_gitignore = true

[embedding]
model = "all-MiniLM-L6-v2"
batch_size = 32

[search]
default_limit = 10
semantic_weight = 0.7

[git]
coupling_enabled = true
coupling_depth = 1000
coupling_threshold = 3
```

## CLI Commands

| Command | Description |
|---------|-------------|
| `bobbin init` | Initialize bobbin in current repository |
| `bobbin index` | Build/rebuild the search index |
| `bobbin search <query>` | Semantic search for code |
| `bobbin grep <pattern>` | Keyword/regex search |
| `bobbin related <file>` | Find files related to a given file |
| `bobbin status` | Show index statistics |

Global flags: `--json`, `--quiet`, `--verbose`
