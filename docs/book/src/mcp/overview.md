---
title: "MCP Overview"
description: "Model Context Protocol integration for AI coding assistants"
tags: [mcp, overview]
category: mcp
---

# MCP Overview

Bobbin implements a [Model Context Protocol (MCP)](https://modelcontextprotocol.io/) server that exposes code search and analysis capabilities to AI coding assistants. When connected, an AI agent can search your codebase semantically, find coupled files, resolve symbol references, and assemble task-focused context — all without leaving the conversation.

## How It Works

```text
AI Agent (Claude Code, Cursor, etc.)
    │
    ├── MCP Protocol (JSON-RPC over stdio)
    │
    ▼
Bobbin MCP Server
    │
    ├── Tools:    search, grep, context, related, find_refs, ...
    ├── Resources: bobbin://index/stats
    ├── Prompts:   explore_codebase
    │
    ▼
Local Storage (LanceDB + SQLite)
```

The MCP server runs as a subprocess of the AI client. It communicates via stdin/stdout using the MCP JSON-RPC protocol. All processing happens locally — no data is sent to external services.

## Starting the Server

```bash
bobbin serve              # MCP server on stdio (default)
bobbin serve --server     # HTTP REST API
bobbin serve --server --mcp  # Both HTTP and MCP simultaneously
```

In normal use, you don't start the server manually. Your AI client launches it automatically based on its MCP configuration.

## Capabilities

The bobbin MCP server advertises three capability types:

### Tools

Nine tools for code search and analysis:

| Tool | Description |
|------|-------------|
| `search` | Semantic/hybrid/keyword code search |
| `grep` | Keyword and regex pattern matching |
| `context` | Task-aware context assembly with coupling expansion |
| `related` | Find temporally coupled files |
| `find_refs` | Find symbol definitions and usages |
| `list_symbols` | List all symbols defined in a file |
| `read_chunk` | Read a specific code section by file and line range |
| `hotspots` | Find high-churn, high-complexity files |
| `prime` | Get an LLM-friendly project overview with live stats |

See [Tools Reference](tools.md) for complete parameter documentation.

### Resources

| URI | Description |
|-----|-------------|
| `bobbin://index/stats` | Index statistics (file count, chunk count, languages) |

### Prompts

| Name | Description |
|------|-------------|
| `explore_codebase` | Guided exploration prompt with suggested queries for a focus area |

The `explore_codebase` prompt accepts an optional `focus` parameter: `architecture`, `entry_points`, `dependencies`, `tests`, or any custom query.

## Protocol Version

Bobbin implements MCP protocol version `2024-11-05` using the [rmcp](https://crates.io/crates/rmcp) Rust library.

## Next Steps

- [Tools Reference](tools.md) — detailed documentation for each tool
- [Client Configuration](client-config.md) — setup for Claude Code, Cursor, and other clients
- [HTTP Mode](http-mode.md) — running bobbin as a remote HTTP server
