---
title: deps
description: Show import dependencies for a file
tags: [cli, deps]
status: draft
category: cli-reference
related: [cli/refs.md, guides/deps-refs.md]
commands: [deps]
feature: deps
source_files: [src/cli/deps.rs]
---

# deps

Show import dependencies for a file, including what it imports and what depends on it.

## Synopsis

```bash
bobbin deps [OPTIONS] <FILE>
```

## Description

The `deps` command queries the bobbin metadata store to display import relationships for a given file. By default it shows **forward dependencies** (what the file imports). With `--reverse` it shows **reverse dependencies** (files that import this file). Use `--both` to see both directions at once.

Dependencies are extracted during indexing from `import`, `use`, `require`, and similar statements depending on the language. Resolved paths show the actual file on disk; unresolved imports are marked accordingly.

## Arguments

| Argument | Description |
|----------|-------------|
| `<FILE>` | File to show dependencies for (required) |

## Options

| Option | Short | Description |
|--------|-------|-------------|
| `--reverse` | `-r` | Show reverse dependencies (files that import this file) |
| `--both` | `-b` | Show both directions (imports and dependents) |

## Examples

Show what a file imports:

```bash
bobbin deps src/main.rs
```

Show what files depend on a module:

```bash
bobbin deps --reverse src/config.rs
```

Show both imports and dependents:

```bash
bobbin deps --both src/lib.rs
```

JSON output for scripting:

```bash
bobbin deps --json src/main.rs
```

## JSON Output

When `--json` is passed, the output has this structure:

```json
{
  "file": "src/main.rs",
  "imports": [
    {
      "specifier": "use crate::config::Config",
      "resolved_path": "src/config.rs"
    }
  ],
  "dependents": null
}
```

With `--reverse` or `--both`, the `dependents` array is populated:

```json
{
  "file": "src/config.rs",
  "imports": null,
  "dependents": [
    {
      "specifier": "use crate::config::Config",
      "source_file": "src/main.rs"
    }
  ]
}
```

Fields that are `null` are omitted from the JSON.

## Prerequisites

Requires a bobbin index. Run `bobbin init` and `bobbin index` first.

## See Also

- [refs](refs.md) — find symbol references across the index
- [related](related.md) — find files related to a given file
