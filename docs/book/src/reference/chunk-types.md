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
