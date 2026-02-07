# Bobbin

**Local-first code context engine.** Index your codebase, search it semantically, and understand which files evolve together -- all running locally with no API keys or cloud services.

Bobbin parses your code with Tree-sitter, generates embeddings with ONNX, and stores everything in LanceDB. It combines vector similarity search with keyword search via Reciprocal Rank Fusion, and uses git history to surface files that frequently change together. The result: fast, accurate code retrieval that understands structure and evolution.

## Features

- **Structure-aware parsing** -- Tree-sitter for Rust, TypeScript, Python, Go, Java, C++; pulldown-cmark for Markdown
- **Hybrid search** -- Semantic + keyword results fused via RRF for best-of-both-worlds retrieval
- **Git-aware context** -- Temporal coupling analysis reveals which files change together
- **Context assembly** -- `bobbin context` builds task-aware bundles from search + git coupling
- **MCP server** -- Expose search to Claude Code, Cursor, and other AI agents
- **Local and fast** -- ONNX embeddings, LanceDB storage, sub-100ms queries, no network required

## Quick Start

```bash
bobbin init                                # Initialize in your repo
bobbin index                               # Index your codebase
bobbin search "authentication middleware"   # Semantic search
bobbin context "fix the login bug"         # Task-aware context bundle
bobbin related src/auth.rs                 # Files that change together
```

## Installation

```bash
cargo install bobbin
```

Or [build from source](CONTRIBUTING.md).

## AI Agent Integration

Bobbin ships an MCP server so AI agents can search your codebase directly:

```bash
bobbin serve
```

Add to your Claude Code or Cursor MCP config:

```json
{
  "mcpServers": {
    "bobbin": {
      "command": "bobbin",
      "args": ["serve"]
    }
  }
}
```

This exposes five tools: `search`, `grep`, `context`, `related`, and `read_chunk`.

## Multi-Repo Support

Index multiple repositories into a single database, then search across all of them or filter by name:

```bash
bobbin index --repo frontend --source ../frontend
bobbin index --repo backend  --source ../backend
bobbin search "auth handler" --repo backend
bobbin context "login flow"                        # Searches all repos
```

## Documentation

| | |
|---|---|
| **[CLI Reference](docs/commands.md)** | All commands, flags, and examples |
| **[Configuration](docs/configuration.md)** | `.bobbin/config.toml` reference |
| **[Architecture](docs/architecture.md)** | System design, data flow, storage schema |
| **[Roadmap](docs/roadmap.md)** | Development phases and planned features |
| **[Contributing](CONTRIBUTING.md)** | Build, test, and development setup |

## License

MIT
