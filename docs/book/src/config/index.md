---
title: Index Settings
description: Configuring file patterns, languages, and indexing behavior
tags: [config, index]
status: draft
category: config
related: [cli/index.md, config/reference.md]
---

# Index Settings

The `[index]` section controls which files are indexed.

## Configuration

```toml
[index]
include = [
    "**/*.rs",
    "**/*.ts",
    "**/*.tsx",
    "**/*.js",
    "**/*.jsx",
    "**/*.py",
    "**/*.go",
    "**/*.md",
]

exclude = [
    "**/node_modules/**",
    "**/target/**",
    "**/dist/**",
    "**/.git/**",
    "**/build/**",
    "**/__pycache__/**",
]

use_gitignore = true
```

## Options

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `include` | string[] | See above | Glob patterns for files to include |
| `exclude` | string[] | See above | Additional exclusion patterns (on top of `.gitignore`) |
| `use_gitignore` | bool | `true` | Whether to respect `.gitignore` files |

## Notes

- **Include patterns** determine which file extensions are parsed and indexed. Add patterns to index additional file types.
- **Exclude patterns** are applied in addition to `.gitignore`. Use them to skip generated code, vendor directories, or other non-useful content.
- When `use_gitignore` is `true`, files matched by `.gitignore` are automatically excluded even if they match an include pattern.
