---
title: Introduction
description: Bobbin — the local-first code context engine for AI-assisted development
tags: [overview, introduction]
category: getting-started
related: [getting-started/installation.md, getting-started/quick-start.md]
---

# Bobbin

**Local-first code context engine.** Semantic search, keyword search, and git coupling analysis — all running on your machine. No API keys. No cloud. Sub-100ms queries.

Bobbin indexes the structure, history, and meaning of your codebase, then delivers precisely the right context when you (or your AI agent) need it.

## What Bobbin Does

- **Hybrid search** — semantic + keyword results fused via Reciprocal Rank Fusion. Ask in natural language or grep by pattern.
- **Git temporal coupling** — discovers files that change together in your commit history, revealing hidden dependencies no import graph can see.
- **Task-aware context assembly** — `bobbin context "fix the login bug"` builds a budget-controlled bundle of the most relevant code, ready for an AI agent.
- **MCP server** — `bobbin serve` exposes 12 tools to Claude Code, Cursor, and any MCP-compatible agent.
- **Claude Code hooks** — automatically injects relevant code context into every prompt, and primes new sessions with project overview and index stats.
- **GPU-accelerated indexing** — CUDA support for 10-25x faster embedding on NVIDIA GPUs. Index 57K chunks in under 5 minutes.

## Quick Start

```bash
cargo install bobbin
cd your-project
bobbin init && bobbin index
bobbin search "error handling"
```

See [Installation](getting-started/installation.md) and [Quick Start](getting-started/quick-start.md) for full setup instructions.

## Navigate This Book

| Section | What You'll Find |
|---------|-----------------|
| [Getting Started](getting-started/installation.md) | Installation, first index, core concepts, agent setup |
| [Guides](guides/searching.md) | Searching, context assembly, git coupling, hooks, multi-repo |
| [CLI Reference](cli/overview.md) | Every command with flags, examples, and output formats |
| [MCP Integration](mcp/overview.md) | AI agent tools, client configuration, HTTP mode |
| [Configuration](config/reference.md) | Full `.bobbin/config.toml` reference |
| [Architecture](architecture/overview.md) | System design, storage, embedding pipeline |
| [Evaluation](eval/overview.md) | Methodology, results across ruff/flask/polars, metrics |
