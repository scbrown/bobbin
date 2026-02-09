---
title: Chunk Types
description: "All chunk types produced by bobbin's parsers: function, class, module, doc, and more"
tags: [reference, chunks, parsing]
status: draft
category: reference
related: [architecture/languages.md, architecture/embedding.md]
---

# Chunk Types

Bobbin parses source files into semantic **chunks** — structural units like functions, classes, and documentation sections. Each chunk is stored with its type, name, line range, content, and embedding vector.

## Code Chunk Types

These chunk types are extracted by tree-sitter from supported programming languages.

| Type | Description | Languages |
|------|-------------|-----------|
| `function` | Standalone function definitions | Rust (`fn`), TypeScript, Python (`def`), Go (`func`), Java, C++ |
| `method` | Functions defined inside a class or type | TypeScript, Java, C++ |
| `class` | Class definitions (including body) | TypeScript, Python, Java, C++ |
| `struct` | Struct/record type definitions | Rust, Go, C++ |
| `enum` | Enumeration type definitions | Rust, Java, C++ |
| `interface` | Interface definitions | TypeScript, Java |
| `trait` | Trait definitions | Rust |
| `impl` | Implementation blocks | Rust (`impl Type`) |
| `module` | Module declarations | Rust (`mod`) |

## Markdown Chunk Types

These chunk types are extracted by pulldown-cmark from Markdown files.

| Type | Description | Example |
|------|-------------|---------|
| `section` | Content under a heading (including the heading) | `## Architecture` and its body text |
| `table` | Markdown tables | `\| Column \| Column \|` |
| `code_block` | Fenced code blocks | ` ```rust ... ``` ` |
| `doc` | YAML frontmatter blocks | `---\ntitle: "..."` |

### `section`

A section chunk captures a heading and all content up to the next heading of the same or higher level. Section names include the full heading hierarchy, so nested headings produce names like `"API Reference > Authentication > OAuth Flow"`.

Given this markdown:

```markdown
# API Reference

Overview text.

## Authentication

Auth details here.

### OAuth Flow

OAuth steps.
```

Bobbin produces three section chunks:

- `"API Reference"` — contains "Overview text."
- `"API Reference > Authentication"` — contains "Auth details here."
- `"API Reference > Authentication > OAuth Flow"` — contains "OAuth steps."

Content before the first heading (excluding frontmatter) becomes a `doc` chunk named "Preamble".

**Search example:**

```bash
bobbin search "OAuth authorization" --type section
```

### `table`

Table chunks capture the full markdown table. They are named after their parent section heading — for example, a table under `## Configuration` becomes `"Configuration (table)"`.

Given this markdown:

```markdown
## Configuration

| Key      | Default | Description          |
|----------|---------|----------------------|
| timeout  | 30      | Request timeout (s)  |
| retries  | 3       | Max retry attempts   |
```

Bobbin produces one table chunk named `"Configuration (table)"`.

**Search example:**

```bash
bobbin search "timeout settings" --type table
bobbin grep "retries" --type table
```

### `code_block`

Code block chunks capture fenced code blocks. They are named by their language tag — ` ```bash ` produces a chunk named `"code: bash"`.

Given this markdown:

````markdown
## Installation

```bash
pip install mypackage
```

```python
import mypackage
mypackage.init()
```
````

Bobbin produces two code_block chunks: `"code: bash"` and `"code: python"`.

**Search example:**

```bash
bobbin search "install dependencies" --type code_block
bobbin grep "pip install" --type code_block
```

### `doc` (frontmatter)

Doc chunks capture YAML frontmatter at the top of a markdown file. The chunk is named `"Frontmatter"`.

Given this markdown:

```markdown
---
title: Deployment Guide
tags: [ops, deployment]
status: published
---

# Deployment Guide
```

Bobbin produces one doc chunk named `"Frontmatter"` containing the YAML block.

**Search example:**

```bash
bobbin grep "status: draft" --type doc
bobbin search "deployment guide metadata" --type doc
```

## Special Chunk Types

| Type | Description |
|------|-------------|
| `commit` | Git commit messages (used internally for history analysis) |
| `other` | Fallback for line-based chunks from unsupported file types |

## Line-Based Fallback

Files that don't match a supported language are split into **line-based chunks**: 50 lines per chunk with a 10-line overlap between consecutive chunks. These chunks have type `other`.

## Filtering by Type

Both the CLI and MCP tools support filtering by chunk type:

```bash
# CLI
bobbin search "auth" --type function
bobbin grep "TODO" --type struct

# MCP tool
search(query: "auth", type: "function")
grep(pattern: "TODO", type: "struct")
```

Accepted type values (case-insensitive, with aliases):

| Value | Aliases |
|-------|---------|
| `function` | `func`, `fn` |
| `method` | — |
| `class` | — |
| `struct` | — |
| `enum` | — |
| `interface` | — |
| `module` | `mod` |
| `impl` | — |
| `trait` | — |
| `doc` | `documentation` |
| `section` | — |
| `table` | — |
| `code_block` | `codeblock` |
| `commit` | — |
| `other` | — |

## Language-to-Chunk Mapping

| Language | Extensions | Chunk Types Extracted |
|----------|------------|---------------------|
| Rust | `.rs` | function, method, struct, enum, trait, impl, module |
| TypeScript | `.ts`, `.tsx` | function, method, class, interface |
| Python | `.py` | function, class |
| Go | `.go` | function, method, struct |
| Java | `.java` | method, class, interface, enum |
| C++ | `.cpp`, `.cc`, `.hpp` | function, method, class, struct, enum |
| Markdown | `.md` | section, table, `code_block`, doc |
| Other | * | other (line-based) |
