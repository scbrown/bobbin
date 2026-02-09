---
title: status
description: Show index status and statistics
tags: [cli, status]
status: draft
category: cli-reference
related: [cli/index.md]
commands: [status]
feature: status
source_files: [src/cli/status.rs]
---

# status

Show index statistics.

## Usage

```bash
bobbin status [OPTIONS]
```

## Examples

```bash
bobbin status
bobbin status --detailed              # Per-language breakdown
bobbin status --repo myproject        # Stats for a specific repo
```

## Options

| Flag | Short | Description |
|------|-------|-------------|
| `--detailed` | | Show per-language breakdown |
| `--repo <NAME>` | `-r` | Stats for a specific repository only |
