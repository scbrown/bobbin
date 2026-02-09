---
title: Searching
description: Using semantic search and grep to find code across your codebase
tags: [search, grep, guide]
status: draft
category: guide
related: [cli/search.md, cli/grep.md, reference/search-modes.md]
commands: [search, grep]
---

# Searching

Bobbin gives you two ways to search your codebase: **semantic search** that understands what your code does, and **keyword grep** that matches exact text. By default, both run together in hybrid mode, giving you the best of both worlds.

## When to use which

**Semantic search** shines when you know *what* you're looking for but not *what it's called*. Ask a question in plain English and bobbin finds relevant code even if your query shares no words with the source.

**Keyword grep** is better when you know the exact identifier, error message, or string literal. It searches the full-text index built during `bobbin index`.

**Hybrid mode** (the default) runs both and merges the results using Reciprocal Rank Fusion (RRF). This is the right choice most of the time.

## Your first search

After running `bobbin init` and `bobbin index`, try a natural-language query:

```bash
bobbin search "how does authentication work"
```

Bobbin embeds your query into the same vector space as your code, finds the closest chunks, and also runs a keyword search. The merged results appear ranked by relevance.

## Choosing a search mode

Use `--mode` to pick a specific engine:

```bash
# Hybrid (default) — best general-purpose choice
bobbin search "database connection pooling"

# Semantic only — good for conceptual queries
bobbin search "retry logic with exponential backoff" --mode semantic

# Keyword only — good for exact identifiers
bobbin search "MAX_RETRY_COUNT" --mode keyword
```

Semantic-only mode is useful when your query is a description rather than code. Keyword-only mode avoids false positives when you're hunting for a specific symbol name.

## Filtering results

### By chunk type

Bobbin parses code into structural chunks: functions, classes, structs, enums, traits, interfaces, impl blocks, modules, and documentation sections. Narrow your search with `--type`:

```bash
# Only functions
bobbin search "parse config file" --type function

# Only structs
bobbin search "database connection" --type struct

# Only documentation
bobbin search "retry behavior" --type doc
```

### By repository

In a [multi-repo](multi-repo.md) setup, filter to a single repository:

```bash
bobbin search "auth middleware" --repo backend
```

### Adjusting result count

The default limit is 10. Increase it when you need a broader view:

```bash
bobbin search "error handling" --limit 30
```

## Keyword grep

`bobbin grep` searches the full-text index for exact terms and patterns:

```bash
# Simple keyword search
bobbin grep "TODO"

# Case-insensitive
bobbin grep "handlerequest" --ignore-case

# Regex post-filter (FTS results are filtered by the regex)
bobbin grep "fn.*test" --regex

# With surrounding context lines
bobbin grep "deprecated" --type function --context 2
```

Grep is fast because it queries the FTS index built at index time rather than scanning files on disk.

## Practical workflows

### Exploring unfamiliar code

When you join a new project and want to understand the authentication flow:

```bash
bobbin search "user authentication flow"
bobbin search "login endpoint" --type function
bobbin search "session management"
```

Each query returns the most relevant code chunks with file paths and line ranges, giving you an instant map of where things live.

### Tracking down a bug

You see an error message in production logs. Start with a keyword search to find where it originates:

```bash
bobbin grep "connection refused: retry limit exceeded"
```

Then broaden with semantic search to find related retry and connection logic:

```bash
bobbin search "connection retry and timeout handling"
```

### Finding usage patterns

Need to see how a specific API is used across the codebase?

```bash
bobbin grep "DatabasePool::new" --context 3
```

The `--context` flag shows lines around each match, so you can see initialization patterns without opening every file.

### JSON output for scripting

Both `search` and `grep` support `--json` for integration with other tools:

```bash
# Pipe search results into jq
bobbin search "auth" --json | jq '.results[].file_path'

# Feed grep results to another script
bobbin grep "FIXME" --json | jq '.results[] | "\(.file_path):\(.start_line)"'
```

## Tuning search quality

### Semantic weight

The `[search]` section in `.bobbin/config.toml` controls hybrid weighting:

```toml
[search]
semantic_weight = 0.7  # 0.0 = keyword only, 1.0 = semantic only
```

Raise this if your queries tend to be natural-language descriptions. Lower it if you mostly search for identifiers.

### Contextual embeddings

For better semantic retrieval on documentation-heavy projects, enable contextual embedding enrichment:

```toml
[embedding.context]
context_lines = 5
enabled_languages = ["markdown", "python"]
```

This includes surrounding lines when computing each chunk's vector, improving retrieval quality.

## Next steps

- [Context Assembly](context-assembly.md) — use search results as seeds for a broader context bundle
- [Deps & Refs](deps-refs.md) — follow import chains and symbol references
- [`search` CLI reference](../cli/search.md) — full flag reference
- [`grep` CLI reference](../cli/grep.md) — full flag reference
