---
title: prime
description: Show LLM-friendly project overview with live stats
tags: [cli, prime, overview]
status: draft
category: cli-reference
related: [cli/status.md, mcp/tools.md]
commands: [prime]
feature: prime
source_files: [src/cli/prime.rs]
---

# prime

Show LLM-friendly project overview with live stats.

## Synopsis

```bash
bobbin prime [OPTIONS] [PATH]
```

## Description

The `prime` command generates a comprehensive project overview designed to be consumed by LLMs and AI agents. It includes what bobbin does, architecture details, available commands, MCP tools, and live index statistics.

Use `--section` to request a specific part, or `--brief` for a compact summary.

## Arguments

| Argument | Description |
|----------|-------------|
| `PATH` | Directory to check (default: `.`) |

## Options

| Option | Default | Description |
|--------|---------|-------------|
| `--brief` | | Show brief (compact) overview only |
| `--section <NAME>` | | Show a specific section (e.g., `architecture`, `commands`, `mcp tools`) |

## Examples

Show full project overview:

```bash
bobbin prime
```

Brief summary:

```bash
bobbin prime --brief
```

Specific section:

```bash
bobbin prime --section "mcp tools"
```

JSON output:

```bash
bobbin prime --json
```

## Prerequisites

Works best with a bobbin index for live stats, but can run without one.

## See Also

- [status](status.md) — index statistics
