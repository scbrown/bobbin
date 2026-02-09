---
title: "status"
description: "Show index status and statistics"
status: draft
category: cli-reference
tags: [cli, status]
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
