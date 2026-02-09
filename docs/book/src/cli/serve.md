---
title: "serve"
description: "Start MCP server for AI agent integration"
status: draft
category: cli-reference
tags: [cli, serve, mcp]
commands: [serve]
feature: serve
source_files: [src/cli/serve.rs]
---

# serve

Start an MCP (Model Context Protocol) server, exposing Bobbin's search and analysis capabilities to AI agents like Claude and Cursor.

## Usage

```bash
bobbin serve [PATH] [OPTIONS]
```

## Examples

```bash
bobbin serve                # Serve current directory
bobbin serve /path/to/repo  # Serve a specific repository
```

## MCP Tools Exposed

| Tool | Description |
|------|-------------|
| `search` | Semantic/hybrid/keyword code search |
| `grep` | Pattern matching with regex support |
| `context` | Task-aware context assembly |
| `related` | Find temporally coupled files |
| `read_chunk` | Read a specific code chunk with context lines |

See [MCP Integration](../mcp/overview.md) for configuration examples.
