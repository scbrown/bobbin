---
title: "Language Support"
description: "Supported languages, Tree-sitter grammars, and adding new language support"
tags: [architecture, languages, tree-sitter]
category: architecture
---

# Language Support

Bobbin uses Tree-sitter for structure-aware parsing of source code, and pulldown-cmark for Markdown documents.

## Supported Languages

| Language | Extensions | Parser | Extracted Units |
|----------|------------|--------|-----------------|
| Rust | `.rs` | Tree-sitter | functions, impl blocks, structs, enums, traits, modules |
| TypeScript | `.ts`, `.tsx` | Tree-sitter | functions, methods, classes, interfaces |
| Python | `.py` | Tree-sitter | functions, classes |
| Go | `.go` | Tree-sitter | functions, methods, type declarations |
| Java | `.java` | Tree-sitter | methods, constructors, classes, interfaces, enums |
| C++ | `.cpp`, `.cc`, `.hpp` | Tree-sitter | functions, classes, structs, enums |
| Markdown | `.md` | pulldown-cmark | sections, tables, code blocks, YAML frontmatter |

## Chunk Types

All extracted units map to a `ChunkType` enum:

```rust
enum ChunkType {
    Function,   // Standalone functions
    Method,     // Class methods
    Class,      // Class definitions
    Struct,     // Struct definitions (Rust, C++, Go)
    Enum,       // Enum definitions
    Interface,  // Interface definitions (TS, Java)
    Module,     // Module definitions
    Impl,       // Impl blocks (Rust)
    Trait,      // Trait definitions (Rust)
    Doc,        // Documentation chunks (Markdown frontmatter, preamble)
    Section,    // Heading-delimited sections (Markdown)
    Table,      // Table elements (Markdown)
    CodeBlock,  // Fenced code blocks (Markdown)
    Other,      // Fallback for line-based chunks
}
```

## Fallback Chunking

File types without a dedicated parser fall back to line-based chunking: 50 lines per chunk with 10-line overlap. This ensures all files in the include patterns are searchable, even without structural parsing.

## File Selection

Which files get indexed is controlled by [configuration](../config/index.md):

```toml
[index]
include = ["**/*.rs", "**/*.ts", "**/*.tsx", "**/*.js", "**/*.jsx", "**/*.py", "**/*.go", "**/*.md"]
exclude = ["**/node_modules/**", "**/target/**", "**/dist/**", "**/.git/**"]
use_gitignore = true
```
