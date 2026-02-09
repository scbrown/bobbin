---
title: "init"
description: "Initialize bobbin in the current repository"
status: draft
category: cli-reference
tags: [cli, init]
commands: [init]
feature: init
source_files: [src/cli/init.rs]
---

# init

Initialize Bobbin in a repository. Creates a `.bobbin/` directory with configuration, SQLite database, and LanceDB vector store.

## Usage

```bash
bobbin init [PATH]
```

## Examples

```bash
bobbin init              # Initialize in current directory
bobbin init /path/to/repo
bobbin init --force      # Overwrite existing configuration
```

## Options

| Flag | Description |
|------|-------------|
| `--force` | Overwrite existing configuration |
