# Bobbin Primer

Bobbin is a local-first code context engine. It indexes your codebase for semantic search, keyword search, and git coupling analysis — all running locally with no API keys required.

## What Bobbin Does

- **Semantic search**: Find code by meaning, not just text (`bobbin search "authentication middleware"`)
- **Keyword/regex search**: Exact pattern matching via full-text search (`bobbin grep "TODO"`)
- **Task-aware context**: Build budget-controlled context bundles from search + git history (`bobbin context "fix the login bug"`)
- **Git temporal coupling**: Discover files that change together (`bobbin related src/auth.rs`)
- **Code hotspots**: Find high-churn, high-complexity files (`bobbin hotspots`)
- **Symbol references**: Find definitions and usages across the codebase (`bobbin refs parse_config`)
- **MCP server**: Expose all capabilities to AI agents via Model Context Protocol (`bobbin serve`)

## Architecture

```
Repository Files → Tree-sitter/pulldown-cmark → ONNX Embedder → LanceDB + SQLite
                   (structural parsing)         (384-dim vectors) (storage)
```

**Storage**: LanceDB holds chunks, vectors, and FTS index. SQLite holds temporal coupling data.

**Search**: Hybrid search combines semantic (vector ANN) and keyword (FTS) results via Reciprocal Rank Fusion (RRF).

**Parsing**: Tree-sitter extracts functions, classes, structs, traits, etc. from 7 languages. Markdown is parsed into sections, tables, and code blocks.

## Supported Languages

Rust, TypeScript, Python, Go, Java, C++, Markdown. Other file types use line-based chunking.

## Key Commands

| Command | Description |
|---------|-------------|
| `bobbin init` | Initialize in current repository |
| `bobbin index` | Build/update the search index |
| `bobbin search <query>` | Semantic + keyword hybrid search |
| `bobbin context <query>` | Task-aware context assembly |
| `bobbin grep <pattern>` | Keyword/regex search |
| `bobbin related <file>` | Find temporally coupled files |
| `bobbin refs <symbol>` | Find symbol definitions and usages |
| `bobbin history <file>` | Commit history and churn stats |
| `bobbin hotspots` | High-churn + high-complexity files |
| `bobbin status` | Index statistics |
| `bobbin serve` | Start MCP server |
| `bobbin hook` | Manage Claude Code hooks |
| `bobbin prime` | This overview |

All commands support `--json`, `--quiet`, and `--verbose` global flags.

## MCP Tools

When running as an MCP server (`bobbin serve`), these tools are available:

| Tool | Description |
|------|-------------|
| `search` | Semantic/hybrid/keyword code search |
| `grep` | Pattern matching with regex support |
| `context` | Task-aware context assembly |
| `related` | Find temporally coupled files |
| `find_refs` | Find symbol definitions and usages |
| `list_symbols` | List all symbols in a file |
| `read_chunk` | Read specific code sections |
| `hotspots` | Find high-churn, high-complexity files |
| `impact` | Predict files affected by a change |
| `review` | Diff-aware context for code review |
| `similar` | Find similar code or duplicate clusters |
| `dependencies` | Show import dependencies for a file |
| `file_history` | Commit history and stats for a file |
| `status` | Index statistics and health |
| `search_beads` | Search indexed issues/tasks |
| `commit_search` | Semantic git commit search |
| `prime` | Get this project overview with live stats |

## Quick Start

```bash
cargo install bobbin
cd your-project
bobbin init && bobbin index
bobbin search "error handling"
```

## Configuration

Stored in `.bobbin/config.toml`. Key settings:

- `index.include` / `index.exclude`: File glob patterns
- `embedding.model`: Embedding model (default: all-MiniLM-L6-v2)
- `search.semantic_weight`: Hybrid search balance (default: 0.7)
- `git.coupling_enabled`: Enable temporal coupling analysis
