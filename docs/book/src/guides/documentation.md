---
title: Indexing Documentation
description: Using bobbin to index and search documentation repos, wikis, and knowledge bases
tags: [documentation, markdown, guide]
status: draft
category: guide
related: [guides/searching.md, reference/chunk-types.md, config/reference.md]
commands: [index, search, grep]
---

# Indexing Documentation

Bobbin isn't just for source code. Its markdown parser understands headings, tables, code blocks, and YAML frontmatter, making it a powerful tool for indexing documentation repos, wikis, and knowledge bases built with tools like mdBook, Sphinx, MkDocs, or Docusaurus.

## Why index your docs?

Documentation scattered across repos, wikis, and knowledge bases is hard to search. A keyword search for "authentication" returns hundreds of hits without context. Bobbin's semantic search lets you ask questions like "how does the OAuth flow work" and get back the specific documentation sections that answer your question.

## Getting started

### Step 1: Initialize bobbin

```bash
cd ~/docs-repo
bobbin init
```

### Step 2: Configure for documentation

Markdown files (`**/*.md`) are included by default. For a pure docs repo, you can tighten the include patterns and skip irrelevant directories:

```toml
[index]
include = [
    "**/*.md",
]

exclude = [
    "**/node_modules/**",
    "**/build/**",
    "**/dist/**",
    "**/.git/**",
    "**/site/**",        # MkDocs build output
    "**/book/**",        # mdBook build output
]
```

### Step 3: Index

```bash
bobbin index
```

Bobbin parses each markdown file into structural chunks:

- **Sections** — content under each heading, with the full heading hierarchy preserved
- **Tables** — standalone table chunks for easy lookup
- **Code blocks** — fenced code blocks extracted as individual chunks
- **Frontmatter** — YAML frontmatter metadata captured as `doc` chunks

### Step 4: Search

```bash
bobbin search "how does deployment work"
```

## How markdown parsing works

Bobbin uses pulldown-cmark to parse markdown into structural chunks. Understanding the chunk types helps you write more effective queries.

### Sections

Every heading creates a section chunk containing the heading and all content up to the next heading of the same or higher level. Section names include the full heading hierarchy:

```
# API Reference            → "API Reference"
## Authentication          → "API Reference > Authentication"
### OAuth Flow             → "API Reference > Authentication > OAuth Flow"
## Rate Limiting           → "API Reference > Rate Limiting"
```

This means searching for "Authentication > OAuth" matches exactly the right section, even in a large document with many headings.

### Frontmatter

YAML frontmatter at the top of a markdown file is extracted as a `doc` chunk:

```markdown
---
title: Deployment Guide
tags: [ops, deployment, kubernetes]
status: published
---

# Deployment Guide
...
```

The frontmatter chunk captures the full YAML block, so you can search for metadata like tags and titles.

### Tables

Markdown tables are extracted as standalone `table` chunks named after their parent section:

```markdown
## HTTP Methods

| Method | Description    | Idempotent |
|--------|---------------|------------|
| GET    | Retrieve      | Yes        |
| POST   | Create        | No         |
| PUT    | Replace       | Yes        |
| DELETE | Remove        | Yes        |
```

This table becomes a chunk named "HTTP Methods (table)", searchable independently from the surrounding text.

### Code blocks

Fenced code blocks are extracted as `code_block` chunks, named by their language tag:

````markdown
## Installation

```bash
pip install mypackage
```
````

This produces a chunk named "code: bash" linked to the "Installation" section.

## Searching documentation

### By section content

Natural-language queries work well for finding documentation sections:

```bash
bobbin search "how to configure the database connection"
bobbin search "troubleshooting SSL certificate errors"
```

### By chunk type

Filter to specific markdown chunk types with `--type`:

```bash
# Find documentation sections about authentication
bobbin search "authentication" --type section

# Find tables (API references, config options, comparison charts)
bobbin search "configuration options" --type table

# Find code examples
bobbin search "kubernetes deployment" --type code_block

# Find frontmatter (metadata, tags, document properties)
bobbin search "status: draft" --type doc
```

### Grep for exact content

Use `bobbin grep` when you know the exact text:

```bash
# Find all documents tagged with "deprecated"
bobbin grep "deprecated" --type doc

# Find sections mentioning a specific API endpoint
bobbin grep "/api/v2/users" --type section

# Find all bash code examples
bobbin grep "#!/bin/bash" --type code_block
```

## Documentation repo patterns

### mdBook

mdBook projects have a `src/` directory with markdown files and a `SUMMARY.md`:

```toml
[index]
include = ["**/*.md"]
exclude = ["**/book/**"]  # Skip build output
```

```bash
bobbin index --source ./src
```

### MkDocs

MkDocs uses `docs/` as the source directory:

```toml
[index]
include = ["**/*.md"]
exclude = ["**/site/**"]  # Skip build output
```

```bash
bobbin index --source ./docs
```

### Sphinx (with MyST)

Sphinx projects using MyST markdown have `.md` files alongside `.rst`:

```toml
[index]
include = ["**/*.md"]
exclude = ["**/_build/**"]
```

### Wiki repositories

Git-based wikis (GitHub Wiki, GitLab Wiki) are plain directories of markdown files:

```bash
git clone https://github.com/org/repo.wiki.git
cd repo.wiki
bobbin init && bobbin index
```

## Tuning for documentation

### Contextual embeddings

For documentation-heavy projects, contextual embedding enrichment improves search quality by including surrounding context when computing each chunk's vector:

```toml
[embedding.context]
context_lines = 5
enabled_languages = ["markdown"]
```

This is enabled for markdown by default. Increase `context_lines` if your documents have short sections that need more surrounding context to be meaningful.

### Semantic weight

Documentation queries tend to be natural language rather than exact identifiers. Consider raising the semantic weight:

```toml
[search]
semantic_weight = 0.8
```

## Practical workflows

### Searching a knowledge base

You maintain an internal knowledge base with hundreds of documents. A new team member asks "how do we handle incident response?"

```bash
bobbin search "incident response process"
bobbin search "on-call rotation and escalation" --type section
bobbin search "severity levels" --type table
```

### Finding outdated documentation

Grep for markers that indicate staleness:

```bash
bobbin grep "TODO" --type section
bobbin grep "FIXME" --type section
bobbin grep "deprecated" --type doc
bobbin grep "status: draft" --type doc
```

### Cross-referencing code and docs

Index both your code and documentation into the same bobbin store:

```bash
bobbin index --repo api --source ~/projects/api
bobbin index --repo docs --source ~/projects/docs
```

Now search across both:

```bash
# Find the code for something mentioned in docs
bobbin search "rate limiting implementation" --repo api

# Find the docs for something you see in code
bobbin search "RateLimiter configuration" --repo docs
```

## Next steps

- [Searching](searching.md) — general search techniques
- [Multi-Repo](multi-repo.md) — indexing docs alongside code repos
- [Chunk Types](../reference/chunk-types.md) — full reference for markdown chunk types
- [Configuration Reference](../config/reference.md) — tuning settings for doc-heavy projects
