---
title: Introduction
description: Bobbin — the local-first code context engine for AI-assisted development
tags: [overview, introduction]
status: published
category: getting-started
related: [getting-started/installation.md, getting-started/quick-start.md]
---

# Bobbin

**Local-first code context engine.** Semantic search, keyword search, and git coupling analysis — all running on your machine. Local indexing and search need no API keys. The first run downloads the model; subsequent searches use the cached model and index.

Bobbin indexes the structure, history, and meaning of your codebase, then delivers precisely the right context when you (or your AI agent) need it.

## What Bobbin Does

- **Hybrid search** — semantic + keyword results fused via Reciprocal Rank Fusion. Ask in natural language or grep by pattern.
- **Git temporal coupling** — discovers files that change together in your commit history, revealing hidden dependencies no import graph can see.
- **Task-aware context assembly** — `bobbin context "fix the login bug"` builds a budget-controlled bundle of the most relevant code, ready for an AI agent.
- **MCP server** — `bobbin serve` exposes tools to Claude Code, Cursor, and any MCP-compatible agent.
- **Knowledge graph** — optional [Quipu](https://github.com/scbrown/quipu) integration adds structured knowledge (SPARQL, SHACL) alongside code search, exposed as `knowledge_context` and `knowledge_query` MCP tools.
- **Claude Code hooks** — automatically injects relevant code context into every prompt, and primes new sessions with project overview and index stats.
- **GPU-accelerated indexing** — Optional CUDA inference on NVIDIA GPUs; CPU indexing remains available. Throughput depends on the model, hardware and repository.

## Choosing a search path

| Need | Start with | Evidence it uses |
|---|---|---|
| A literal name or pattern | `bobbin grep` | Indexed text |
| Code described in natural language | `bobbin search` | Embeddings and keyword matches |
| Related files for a task | `bobbin context` | Search results and Git coupling |
| Who calls a symbol | [Yupana](https://github.com/scbrown/yupana) | A structural code graph |

Git coupling describes files that changed together; it does not establish
that one function calls another. Consult the [evaluation methodology](eval/overview.md)
for measured retrieval quality and its limits.

## Quick Start

Start with a [release with checksum verification](getting-started/installation.md), then run
the [three-command fixture](getting-started/quick-start.md) to verify your index
before using your own repository.

## Navigate This Book

| Section | What You'll Find |
|---------|-----------------|
| [Getting Started](getting-started/installation.md) | Installation, first index, core concepts, agent setup |
| [Guides](guides/searching.md) | Searching, context assembly, git coupling, Quipu integration, hooks, multi-repo |
| [CLI Reference](cli/overview.md) | Every command with flags, examples, and output formats |
| [MCP Integration](mcp/overview.md) | AI agent tools, client configuration, HTTP mode |
| [Configuration](config/reference.md) | Full `.bobbin/config.toml` reference |
| [Architecture](architecture/overview.md) | System design, storage, embedding pipeline |
| [Evaluation](eval/overview.md) | Methodology, results across ruff/flask/polars, metrics |

## Go deeper

- [The stack](stack.md) explains how Bobbin fits with the other tools.
- [Docs map](docs-map.md) routes the design notes, plans and research outside this book.
- [Evaluation](eval/overview.md) explains how retrieval quality is measured.
