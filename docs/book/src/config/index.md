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
    "**/*.java",
    "**/*.cpp",
    "**/*.cc",
    "**/*.hpp",
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

# Multimodal ingest (opt-in). When true, the indexer also walks PDFs,
# extracts their text, and chunks it like a plain-text document.
multimodal = false
```

## Options

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `include` | string[] | See above | Glob patterns for files to include |
| `exclude` | string[] | See above | Additional exclusion patterns (on top of `.gitignore`) |
| `use_gitignore` | bool | `true` | Whether to respect `.gitignore` files |
| `multimodal` | bool | `false` | Enable multimodal ingest (PDF text extraction). See below. |

## Notes

- **Include patterns** determine which file extensions are parsed and indexed. Add patterns to index additional file types.
- **Exclude patterns** are applied in addition to `.gitignore`. Use them to skip generated code, vendor directories, or other non-useful content.
- When `use_gitignore` is `true`, files matched by `.gitignore` are automatically excluded even if they match an include pattern.

## Multimodal ingest

By default bobbin indexes code, markdown, and beads. Set `multimodal = true` to
also ingest **PDFs** (runbooks, design docs, specs):

- The indexer automatically walks `**/*.pdf` — you do **not** need to add it to
  `include`. Toggling the flag is the only knob.
- Text is extracted with a pure-Rust extractor (no Python, no native toolchain)
  and chunked like a plain-text document. Chunks are tagged with
  `language = "pdf"`, so you can filter on them in search.
- Image-only or encrypted PDFs may yield little or no text; those files are
  skipped the same way an empty file is.
- Image captioning (vision LLM) is **not** yet supported and is tracked as a
  follow-up.
