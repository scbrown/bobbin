# Bobbin

A local-first code context engine with Temporal RAG. Bobbin indexes your codebase for semantic search using embeddings stored in LanceDB, with temporal coupling analysis via SQLite.

## Features

- **Local-first**: All indexing and search happens on your machine. No data leaves your repository.
- **Structure-aware**: Uses Tree-sitter for AST-based semantic chunking. Functions, classes, and modules are first-class citizens.
- **Markdown-aware**: Uses pulldown-cmark for semantic markdown parsing. Extracts sections, tables, code blocks, and YAML frontmatter.
- **Hybrid search**: Combines semantic (vector) and keyword (FTS) search using Reciprocal Rank Fusion for best-of-both-worlds results.
- **Contextual embeddings**: Embeds chunks with surrounding context for improved retrieval quality.
- **Multi-repo**: Index and search across multiple repositories from a single `.bobbin/` database.
- **MCP server**: Expose search capabilities to AI agents (Claude, Cursor) via the Model Context Protocol.
- **Fast**: Sub-100ms queries using LanceDB vector search and FTS.
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

Build or update the search index. Walks repository files, parses them with Tree-sitter (or pulldown-cmark for Markdown), generates embeddings, and stores everything in LanceDB.

```bash
bobbin index                # Full index of current directory
bobbin index --incremental  # Only update changed files
bobbin index --force        # Force reindex all files
bobbin index --verbose      # Show detailed statistics
bobbin index --json         # Output in JSON format
bobbin index --repo myproject  # Tag chunks with a repository name (for multi-repo)
bobbin index --source /other/repo --repo other  # Index a different directory
```

### `bobbin search <QUERY>`

Search across your codebase. By default, uses hybrid search combining semantic (vector similarity) and keyword (FTS) results using Reciprocal Rank Fusion (RRF).

```bash
bobbin search "error handling"                    # Hybrid search (default)
bobbin search "database connection" --limit 20   # Limit results
bobbin search "auth" --type function             # Filter by chunk type
bobbin search "auth" --mode semantic             # Semantic-only search
bobbin search "handleAuth" --mode keyword        # Keyword-only search
bobbin search "auth" --repo myproject            # Search within a specific repo
```

**Search modes:**
| Mode | Description |
|------|-------------|
| `hybrid` | Combines semantic + keyword using RRF (default) |
| `semantic` | Vector similarity search only |
| `keyword` | Full-text keyword search only |

The `semantic_weight` in `.bobbin/config.toml` controls the balance between semantic and keyword results in hybrid mode (default: 0.7, meaning 70% weight to semantic matches).

### `bobbin grep <PATTERN>`

Keyword and regex search using LanceDB FTS.

```bash
bobbin grep "TODO"
bobbin grep "handleRequest" --ignore-case
bobbin grep "TODO" --repo myproject              # Filter to a specific repo
```

### `bobbin history <FILE>`

Show commit history and churn statistics for a file.

```bash
bobbin history src/main.rs
bobbin history src/main.rs --limit 50    # Show more entries
bobbin history src/main.rs --json        # JSON output with stats
```

### `bobbin serve [PATH]`

Start an MCP (Model Context Protocol) server, exposing Bobbin's search and analysis tools to AI agents like Claude and Cursor.

```bash
bobbin serve                # Serve current directory
bobbin serve /path/to/repo  # Serve a specific repository
```

**MCP tools exposed:**
- `search` - Semantic/hybrid/keyword code search
- `grep` - Pattern matching with regex support
- `related` - Find temporally coupled files
- `read_chunk` - Read a specific code chunk with context

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

# Contextual embedding settings
[embedding.context]
# Number of context lines before/after a chunk to include in the embedding
context_lines = 5

# Languages where contextual embedding is enabled
enabled_languages = ["markdown"]

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

Bobbin uses Tree-sitter for structure-aware parsing, and pulldown-cmark for Markdown. The following languages have full semantic extraction:

| Language | Extensions | Extracted Units |
|----------|------------|-----------------|
| Rust | `.rs` | functions, impl blocks, structs, enums, traits, modules |
| TypeScript | `.ts`, `.tsx` | functions, methods, classes, interfaces |
| Python | `.py` | functions, classes |
| Go | `.go` | functions, methods, type declarations |
| Java | `.java` | methods, constructors, classes, interfaces, enums |
| C++ | `.cpp`, `.cc`, `.hpp` | functions, classes, structs, enums |
| Markdown | `.md` | sections, tables, code blocks, YAML frontmatter |

Other file types fall back to line-based chunking (50 lines per chunk with 10-line overlap).

## Architecture

```
.bobbin/
├── config.toml     # Configuration
├── index.db        # SQLite: temporal coupling + metadata
└── vectors/        # LanceDB: chunks, embeddings, and FTS (primary storage)
```

**Stack:**
- **Tree-sitter**: AST-based code parsing (Rust, TypeScript, Python, Go, Java, C++)
- **pulldown-cmark**: Semantic markdown parsing
- **ONNX Runtime**: Local embedding generation (all-MiniLM-L6-v2, 384 dimensions)
- **LanceDB**: Primary storage - chunks, vectors, and full-text search
- **SQLite**: Temporal coupling data and global metadata only
- **rmcp**: MCP server for AI agent integration

## Data Storage

**LanceDB (primary):**
- All chunk data: content, metadata, file paths, language, chunk type
- 384-dimensional embeddings for each code chunk
- Full-text search index on chunk content
- Multi-repo support via `repo` field
- Contextual embeddings stored in `full_context` field

**SQLite (auxiliary):**
- `coupling` - Temporal coupling between files (from git history)
- `meta` - Global metadata key-value pairs (e.g., embedding model)

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
- [x] Hybrid search (RRF combining semantic + keyword)
- [x] Git temporal coupling analysis
- [x] Related files suggestions
- [x] Additional language support (Go, Java, C++)

### Phase 3: Polish
- [x] MCP server integration
- [x] Multi-repo support
- [x] LanceDB-primary storage consolidation
- [x] Contextual embeddings
- [x] Semantic markdown chunking (pulldown-cmark)
- [x] File history and churn analysis
- [ ] Watch mode / daemon
- [ ] Shell completions (bash/zsh/fish)
- [ ] Performance optimizations

## License

MIT
