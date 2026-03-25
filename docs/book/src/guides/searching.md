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

# Only documentation frontmatter
bobbin search "retry behavior" --type doc
```

For markdown files, additional chunk types let you target specific document structures:

```bash
# Heading-delimited sections
bobbin search "deployment steps" --type section

# Tables (API references, config options, comparison charts)
bobbin search "HTTP status codes" --type table

# Fenced code blocks (examples, snippets)
bobbin search "docker compose" --type code_block
```

See [Indexing Documentation](documentation.md) for a full guide to searching docs.

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

## Inline query syntax

Bobbin supports inline filter syntax directly in the query string, similar to GitHub code search. Filters are extracted from the query before search runs.

### Available filters

| Filter | Example | Description |
| ------ | ------- | ----------- |
| `repo:` | `repo:aegis auth handler` | Scope to a specific repository |
| `lang:` | `lang:rust,go error handling` | Filter by language(s), comma-separated |
| `type:` | `type:function parse config` | Filter by chunk type (function, struct, section, etc.) |
| `file:` | `file:*.rs connection pool` | Filter by file path (glob pattern) |
| `group:` | `group:infra deploy pipeline` | Filter by named repo group |
| `tag:` | `tag:domain:monitoring alert` | Include only chunks with this tag |
| `-repo:` | `-repo:test auth flow` | Exclude a repository |
| `-lang:` | `-lang:markdown token refresh` | Exclude a language |
| `-tag:` | `-tag:type:changelog recent changes` | Exclude chunks with a tag |

### Combining filters

Multiple filters can be combined in a single query:

```bash
bobbin search "repo:aegis lang:go type:function error handling"
bobbin search "lang:rust,python -repo:test database connection"
```

### Boolean operators

Use `OR` (uppercase) to match either term, and `-` or `NOT` to exclude:

```bash
bobbin search "authentication OR authorization"
bobbin search "database -migration"
bobbin search "NOT deprecated connection pool"
```

### Exact phrases

Wrap terms in double quotes to match an exact phrase:

```bash
bobbin search '"connection refused" retry logic'
```

### Regex patterns

Use `/pattern/` syntax for regex filtering within results:

```bash
bobbin search "/fn\s+handle_.*request/"
```

### Tag filtering via API and UI

The web UI exposes tag, exclude_tag, repo, and group filters as input fields below the search bar. The HTTP API accepts the same as query params:

```
GET /search?q=error+handling&repo=aegis&tag=domain:monitoring&exclude_tag=type:changelog
```

## Advanced search features

### Recency weighting

Recently modified files get a scoring boost. This helps surface actively-maintained code over stale artifacts.

```toml
[search]
recency_half_life_days = 30.0  # After 30 days, boost drops to 50%
recency_weight = 0.3           # Max 30% score penalty for old files (0.0 = disabled)
```

The formula: `score * (1.0 - weight + weight * decay)`. At `recency_weight = 0.3`, a very old file loses at most 30% of its score. At 1.0, old files can lose 100%.

Set `recency_weight = 0.0` to disable entirely (treat all files equally regardless of age).

### Doc demotion

Documentation and config files naturally score high on many queries because they describe everything. Doc demotion reduces their ranking so source code surfaces first.

```toml
[search]
doc_demotion = 0.3  # Multiply doc/config RRF scores by 0.3 (70% penalty)
```

Values: `1.0` = no demotion, `0.0` = completely suppress docs. Only affects files classified as documentation or config — source and test files are untouched.

### Bridge mode

Bridging discovers source files through documentation and commit context. When a search matches a doc file that references `auth.rs`, bridging can find and include `auth.rs` even if it didn't match directly.

Four modes:

| Mode | Behavior |
|------|----------|
| `off` | No bridging (baseline) |
| `inject` | Add discovered files as new results (default) |
| `boost` | Boost scores of files already in results |
| `boost_inject` | Both: boost existing + inject undiscovered |

Bridge mode is configured in `calibration.json` (via `bobbin calibrate --bridge-sweep`) or in context assembly config. The `bridge_boost_factor` controls how much existing scores increase: `final_score *= (1.0 + factor)`.

### Keyword-triggered repo scoping

In multi-repo setups, certain queries should automatically scope to specific repos. Configure this in `[hooks]`:

```toml
[[hooks.keyword_repos]]
keywords = ["ansible", "playbook", "deploy"]
repos = ["goldblum", "homelab-mcp"]

[[hooks.keyword_repos]]
keywords = ["bobbin", "search", "index"]
repos = ["bobbin"]
```

When any keyword matches (case-insensitive substring), search is scoped to those repos instead of searching everywhere. This reduces noise when the query clearly targets a specific domain.

### Repo affinity boost

Files from the agent's current repo get a configurable score multiplier:

```toml
[hooks]
repo_affinity_boost = 2.0  # 2x score for local repo files (1.0 = disabled)
```

This biases results toward the repo the agent is working in, which is usually what you want.

### RRF constant (rrf_k)

Controls how Reciprocal Rank Fusion merges semantic and keyword results:

```toml
[search]
rrf_k = 60.0  # Standard value
```

Lower values make top-ranked results dominate more. Higher values flatten the score distribution, giving lower-ranked results more influence. Most users should leave this at 60.0.

### Calibrating all parameters

Rather than tuning manually, use `bobbin calibrate` to auto-tune:

```bash
bobbin calibrate --apply               # Quick sweep of core params
bobbin calibrate --full --apply        # Extended: also tunes recency + coupling
bobbin calibrate --bridge-sweep --apply # Sweep bridge mode using calibrated core
```

See [calibrate CLI reference](../cli/calibrate.md) for details.

## Next steps

- [Context Assembly](context-assembly.md) — use search results as seeds for a broader context bundle
- [Tags & Effects](tags.md) — boost or demote results by tag patterns
- [Deps & Refs](deps-refs.md) — follow import chains and symbol references
- [Access Control](access-control.md) — role-based result filtering
- [`search` CLI reference](../cli/search.md) — full flag reference
- [`grep` CLI reference](../cli/grep.md) — full flag reference
