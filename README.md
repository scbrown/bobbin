# Bobbin

A local-first semantic code indexing tool. Bobbin indexes your codebase for semantic search using embeddings stored in LanceDB, with metadata and full-text search in SQLite.

## Features

- **Local-first**: All indexing and search happens on your machine. No data leaves your repository.
- **Structure-aware**: Uses Tree-sitter for AST-based semantic chunking. Functions, classes, and modules are first-class citizens.
- **Fast**: Sub-100ms queries using LanceDB vector search and SQLite FTS5.
- **Incremental**: Only re-indexes files that have changed (hash-based detection).
- **Configurable**: Customize include/exclude patterns, embedding model, and search weights via `.bobbin/config.toml`.

## Installation

```bash
cargo install bobbin
```

Or build from source:

```bash
git clone https://github.com/bobbin-dev/bobbin
cd bobbin
cargo build --release
```

## Quick Start

```bash
# Initialize bobbin in your repository
bobbin init

# Index your codebase (downloads embedding model on first run)
bobbin index

# Search for code semantically
bobbin search "authentication middleware"

# Keyword search
bobbin grep "handleAuth"
```

## Commands

### `bobbin init [PATH]`

Initialize Bobbin in a repository. Creates a `.bobbin/` directory with configuration, SQLite database, and LanceDB vector store.

```bash
bobbin init              # Initialize in current directory
bobbin init /path/to/repo
bobbin init --force      # Overwrite existing configuration
```

### `bobbin index [PATH]`

Build or update the search index. Walks repository files, parses them with Tree-sitter, generates embeddings, and stores everything in the local database.

```bash
bobbin index                # Full index of current directory
bobbin index --incremental  # Only update changed files
bobbin index --force        # Force reindex all files
bobbin index --verbose      # Show detailed statistics
bobbin index --json         # Output in JSON format
```

### `bobbin search <QUERY>`

Semantic search across your codebase using vector similarity.

```bash
bobbin search "error handling"
bobbin search "database connection" --limit 20
bobbin search "auth" --type rust
```

### `bobbin grep <PATTERN>`

Keyword and regex search using SQLite FTS5.

```bash
bobbin grep "TODO"
bobbin grep "handleRequest" --ignore-case
```

### `bobbin status`

Show index statistics.

```bash
bobbin status
bobbin status --detailed    # Per-language breakdown
```

### Global Flags

All commands support these flags:

| Flag | Description |
|------|-------------|
| `--json` | Output in JSON format |
| `--quiet` | Suppress non-essential output |
| `--verbose` | Show detailed progress |

## Configuration

Bobbin stores its configuration in `.bobbin/config.toml`. Here's the default configuration:

```toml
[index]
# Glob patterns for files to include
include = [
    "**/*.rs",
    "**/*.ts",
    "**/*.tsx",
    "**/*.js",
    "**/*.jsx",
    "**/*.py",
    "**/*.go",
    "**/*.md",
]

# Glob patterns for files to exclude (in addition to .gitignore)
exclude = [
    "**/node_modules/**",
    "**/target/**",
    "**/dist/**",
    "**/.git/**",
    "**/build/**",
    "**/__pycache__/**",
]

# Whether to respect .gitignore files
use_gitignore = true

[embedding]
# Embedding model (downloaded automatically)
model = "all-MiniLM-L6-v2"

# Batch size for embedding generation
batch_size = 32

[search]
# Default number of search results
default_limit = 10

# Weight for semantic vs keyword search (0.0 = keyword only, 1.0 = semantic only)
semantic_weight = 0.7

[git]
# Enable temporal coupling analysis
coupling_enabled = true

# Number of commits to analyze for coupling
coupling_depth = 1000

# Minimum co-changes to establish coupling
coupling_threshold = 3
```

## Supported Languages

Bobbin uses Tree-sitter for structure-aware parsing. The following languages have full semantic extraction:

| Language | Extensions | Extracted Units |
|----------|------------|-----------------|
| Rust | `.rs` | functions, impl blocks, structs, enums, traits, modules |
| TypeScript | `.ts`, `.tsx` | functions, methods, classes, interfaces |
| Python | `.py` | functions, classes |
| Markdown | `.md` | headers, sections |

Other file types fall back to line-based chunking (50 lines per chunk with 10-line overlap).

## Architecture

```
.bobbin/
├── config.toml     # Configuration
├── index.db        # SQLite: metadata + full-text search (FTS5)
└── vectors/        # LanceDB: vector embeddings
```

**Stack:**
- **Tree-sitter**: AST-based code parsing
- **ONNX Runtime**: Local embedding generation (all-MiniLM-L6-v2, 384 dimensions)
- **LanceDB**: Embedded vector database for semantic search
- **SQLite**: Metadata storage and FTS5 full-text search

## Data Storage

**SQLite Tables:**
- `files` - Indexed file metadata (path, language, hash, timestamps)
- `chunks` - Semantic code units (functions, classes, etc.)
- `chunks_fts` - Full-text search index
- `coupling` - Temporal coupling between files (from git history)

**LanceDB:**
- 384-dimensional embeddings for each code chunk
- Approximate nearest neighbor search

## Development

```bash
# Build
cargo build

# Run tests
cargo test

# Lint
cargo clippy

# Type check
cargo check
```

## Roadmap

### Phase 1: Foundation (MVP) - Complete
- [x] Tree-sitter code indexing (Rust, TypeScript, Python)
- [x] LanceDB vector storage
- [x] SQLite metadata + FTS
- [x] CLI: `init` command
- [x] CLI: `index` command (full and incremental)
- [x] Configuration management
- [x] ONNX embedding generation
- [x] CLI: `search` command
- [x] CLI: `grep` command
- [x] CLI: `status` command

### Phase 2: Intelligence
- [ ] Semantic + hybrid search
- [ ] Git temporal coupling analysis
- [ ] Related files suggestions
- [ ] Additional language support (Go, Java, C++)

### Phase 3: Polish
- [ ] MCP server integration
- [ ] Watch mode / daemon
- [ ] Multi-repo support
- [ ] Performance optimizations

## License

MIT
