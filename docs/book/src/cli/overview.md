---
title: "CLI Overview"
description: "Global flags, output modes, and thin-client mode"
status: draft
category: cli-reference
tags: [cli, overview]
---

# CLI Overview

All commands support these global flags:

| Flag | Description |
|------|-------------|
| `--json` | Output in JSON format |
| `--quiet` | Suppress non-essential output |
| `--verbose` | Show detailed progress |
| `--server <URL>` | Use remote bobbin server (thin-client mode) |

## Commands

| Command | Description |
|---------|-------------|
| [`bobbin init`](init.md) | Initialize bobbin in current repository |
| [`bobbin index`](index.md) | Build/rebuild the search index |
| [`bobbin search`](search.md) | Hybrid search (combines semantic + keyword) |
| [`bobbin context`](context.md) | Assemble task-relevant context from search + git coupling |
| [`bobbin grep`](grep.md) | Keyword/regex search with highlighting |
| [`bobbin deps`](deps.md) | Import dependency analysis |
| [`bobbin refs`](refs.md) | Symbol reference resolution |
| [`bobbin related`](related.md) | Find files related to a given file |
| [`bobbin history`](history.md) | Show commit history and churn statistics |
| [`bobbin hotspots`](hotspots.md) | Identify high-churn/complexity code |
| [`bobbin status`](status.md) | Show index statistics |
| [`bobbin serve`](serve.md) | Start MCP server for AI agent integration |
| [`bobbin benchmark`](benchmark.md) | Run embedding benchmarks |
| [`bobbin watch`](watch.md) | Watch mode for automatic re-indexing |
| [`bobbin completions`](completions.md) | Generate shell completions |
| [`bobbin hook`](hook.md) | Claude Code hook integration |

## Supported Languages

Bobbin uses Tree-sitter for structure-aware parsing, and pulldown-cmark for Markdown:

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
