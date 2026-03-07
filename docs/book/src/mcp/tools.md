---
title: Tools Reference
description: Complete reference for all bobbin MCP tools
tags: [mcp, tools, reference]
status: draft
category: mcp
related: [mcp/overview.md, cli/search.md, cli/context.md]
---

# Tools Reference

All tools are available when bobbin runs as an MCP server (`bobbin serve`). Each tool accepts JSON parameters and returns JSON results.

## search

Search for code using natural language. Finds functions, classes, and other code elements that match the semantic meaning of your query.

**Parameters:**

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `query` | string | yes | — | Natural language search query |
| `type` | string | no | all | Filter by chunk type: `function`, `method`, `class`, `struct`, `enum`, `interface`, `module`, `impl`, `trait` |
| `limit` | integer | no | 10 | Maximum number of results |
| `mode` | string | no | `hybrid` | Search mode: `hybrid`, `semantic`, or `keyword` |
| `repo` | string | no | all | Filter to a specific repository |

**Response fields:** `query`, `mode`, `count`, `results[]` (each with `file_path`, `name`, `chunk_type`, `start_line`, `end_line`, `score`, `match_type`, `language`, `content_preview`)

## grep

Search for code using exact keywords or regex patterns.

**Parameters:**

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `pattern` | string | yes | — | Pattern to search for |
| `ignore_case` | boolean | no | false | Case-insensitive search |
| `regex` | boolean | no | false | Enable regex matching (post-filters FTS results) |
| `type` | string | no | all | Filter by chunk type |
| `limit` | integer | no | 10 | Maximum number of results |
| `repo` | string | no | all | Filter to a specific repository |

**Response fields:** `pattern`, `count`, `results[]` (each with `file_path`, `name`, `chunk_type`, `start_line`, `end_line`, `score`, `language`, `content_preview`, `matching_lines[]`)

## context

Assemble a comprehensive context bundle for a task. Combines semantic search results with temporally coupled files from git history.

**Parameters:**

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `query` | string | yes | — | Natural language task description |
| `budget` | integer | no | 500 | Maximum lines of content |
| `depth` | integer | no | 1 | Coupling expansion depth (0 = no coupling) |
| `max_coupled` | integer | no | 3 | Max coupled files per seed file |
| `limit` | integer | no | 20 | Max initial search results |
| `coupling_threshold` | float | no | 0.1 | Minimum coupling score |
| `repo` | string | no | all | Filter to a specific repository |

**Response fields:** `query`, `budget` (`max_lines`, `used_lines`), `files[]` (each with `path`, `language`, `relevance`, `score`, `coupled_to[]`, `chunks[]`), `summary` (`total_files`, `total_chunks`, `direct_hits`, `coupled_additions`)

## related

Find files related to a given file based on git commit history (temporal coupling).

**Parameters:**

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `file` | string | yes | — | File path relative to repo root |
| `limit` | integer | no | 10 | Maximum number of results |
| `threshold` | float | no | 0.0 | Minimum coupling score (0.0–1.0) |

**Response fields:** `file`, `related[]` (each with `path`, `score`, `co_changes`)

## find_refs

Find the definition and all usages of a symbol by name.

**Parameters:**

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `symbol` | string | yes | — | Exact symbol name (e.g., `parse_config`) |
| `type` | string | no | all | Filter by symbol type |
| `limit` | integer | no | 20 | Maximum number of usage results |
| `repo` | string | no | all | Filter to a specific repository |

**Response fields:** `symbol`, `definition` (`name`, `chunk_type`, `file_path`, `start_line`, `end_line`, `signature`), `usage_count`, `usages[]` (each with `file_path`, `line`, `context`)

## list_symbols

List all symbols (functions, structs, traits, etc.) defined in a file.

**Parameters:**

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `file` | string | yes | — | File path relative to repo root |
| `repo` | string | no | all | Filter to a specific repository |

**Response fields:** `file`, `count`, `symbols[]` (each with `name`, `chunk_type`, `start_line`, `end_line`, `signature`)

## read_chunk

Read a specific section of code from a file by line range.

**Parameters:**

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `file` | string | yes | — | File path relative to repo root |
| `start_line` | integer | yes | — | Starting line number |
| `end_line` | integer | yes | — | Ending line number |
| `context` | integer | no | 0 | Context lines to include before and after |

**Response fields:** `file`, `start_line`, `end_line`, `actual_start_line`, `actual_end_line`, `content`, `language`

## hotspots

Identify code hotspots — files with both high churn and high complexity.

**Parameters:**

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `since` | string | no | `1 year ago` | Time window (e.g., `6 months ago`, `3 months ago`) |
| `limit` | integer | no | 20 | Maximum number of hotspots |
| `threshold` | float | no | 0.0 | Minimum hotspot score (0.0–1.0) |

**Response fields:** `count`, `since`, `hotspots[]` (each with `file`, `score`, `churn`, `complexity`, `language`)

## prime

Get an LLM-friendly overview of the bobbin project with live index statistics.

**Parameters:**

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `section` | string | no | all | Specific section: `what bobbin does`, `architecture`, `supported languages`, `key commands`, `mcp tools`, `quick start`, `configuration` |
| `brief` | boolean | no | false | Compact overview (title and first section only) |

**Response fields:** `primer` (markdown text), `section`, `initialized`, `stats` (`total_files`, `total_chunks`, `total_embeddings`, `languages[]`, `last_indexed`)

## impact

Predict which files are affected by a change to a target file or function. Combines git co-change coupling and semantic similarity signals.

**Parameters:**

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `target` | string | yes | — | File path or `file:symbol` reference |
| `depth` | integer | no | 1 | Transitive expansion depth (0–3) |
| `mode` | string | no | `combined` | Signal mode: `combined`, `coupling`, `semantic`, `deps` |
| `threshold` | float | no | 0.1 | Minimum impact score |
| `limit` | integer | no | 20 | Maximum number of results |

## review

Assemble review context from a git diff. Finds indexed chunks overlapping changed lines and expands via temporal coupling.

**Parameters:**

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `diff` | string | no | unstaged | Diff spec: `unstaged`, `staged`, `branch:<name>`, `commit:<range>` |
| `budget` | integer | no | 500 | Maximum lines of context |
| `depth` | integer | no | 1 | Coupling expansion depth |

## similar

Find code chunks semantically similar to a target, or scan for duplicate clusters.

**Parameters:**

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `target` | string | no | — | Chunk reference (`file.rs:function_name`) or free text |
| `scan` | boolean | no | false | Scan entire codebase for near-duplicate clusters |
| `threshold` | float | no | 0.85 | Minimum similarity score |
| `limit` | integer | no | 10 | Maximum results |
| `cross_repo` | boolean | no | false | Include cross-repo matches |

## search_beads

Search for beads (issues/tasks) using natural language. Requires beads to be indexed via `bobbin index --include-beads`.

**Parameters:**

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `query` | string | yes | — | Natural language query |
| `priority` | integer | no | all | Filter by priority (1–4) |
| `status` | string | no | all | Filter by status |
| `assignee` | string | no | all | Filter by assignee |
| `limit` | integer | no | 10 | Maximum results |
| `enrich` | boolean | no | true | Enrich with live Dolt metadata |

## dependencies

Show import dependencies for a file. Returns forward and/or reverse dependencies.

**Parameters:**

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `file` | string | yes | — | File path relative to repo root |
| `reverse` | boolean | no | false | Show reverse dependencies (what imports this file) |
| `both` | boolean | no | false | Show both forward and reverse |
| `repo` | string | no | all | Filter to a specific repository |

## file_history

Show git commit history for a specific file, with author breakdown and churn rate.

**Parameters:**

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `file` | string | yes | — | File path relative to repo root |
| `limit` | integer | no | 20 | Maximum commits to return |

## status

Show current index status and statistics.

**Parameters:**

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `languages` | boolean | no | false | Include per-language breakdown |

## commit_search

Search git commit history using natural language.

**Parameters:**

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `query` | string | yes | — | Natural language query |
| `author` | string | no | all | Filter by author |
| `file` | string | no | all | Filter by file path |
| `limit` | integer | no | 10 | Maximum results |

## feedback_submit

Submit feedback on a bobbin context injection. Rate injections as useful, noise, or harmful.

**Parameters:**

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `injection_id` | string | yes | — | Injection ID from `[injection_id: inj-xxx]` |
| `rating` | string | yes | — | `useful`, `noise`, or `harmful` |
| `agent` | string | no | auto | Agent identity (auto-detected from env) |
| `reason` | string | no | — | Explanation (max 1000 chars) |

## feedback_list

List recent feedback records with optional filters.

**Parameters:**

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `rating` | string | no | all | Filter by rating |
| `agent` | string | no | all | Filter by agent |
| `limit` | integer | no | 20 | Maximum results (max 50) |

## feedback_stats

Get aggregated feedback statistics — total injections, coverage rate, rating breakdown, and lineage counts.

**Parameters:** None.

## feedback_lineage_store

Record a lineage action that ties feedback to a concrete fix. Links feedback records to commits, beads, or config changes.

**Parameters:**

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `feedback_ids` | integer[] | yes | — | Feedback record IDs to link |
| `action_type` | string | yes | — | `code_fix`, `config_change`, `tag_effect`, `access_rule`, or `exclusion_rule` |
| `bead` | string | no | — | Associated bead ID |
| `commit_hash` | string | no | — | Git commit hash |
| `description` | string | yes | — | What was done |
| `agent` | string | no | auto | Agent identity |

## feedback_lineage_list

List lineage records showing how feedback was acted on.

**Parameters:**

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `feedback_id` | integer | no | all | Filter by feedback ID |
| `bead` | string | no | all | Filter by bead ID |
| `commit_hash` | string | no | all | Filter by commit hash |
| `limit` | integer | no | 20 | Maximum results (max 50) |

## archive_search

Search archive records (HLA chat logs, Pensieve agent memory) using natural language.

**Parameters:**

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `query` | string | yes | — | Natural language query |
| `source` | string | no | all | Filter: `hla` or `pensieve` |
| `filter` | string | no | all | Filter by name/channel |
| `after` | string | no | — | Only records after date (YYYY-MM-DD) |
| `before` | string | no | — | Only records before date (YYYY-MM-DD) |
| `limit` | integer | no | 10 | Maximum results |
| `mode` | string | no | `hybrid` | `hybrid`, `semantic`, or `keyword` |

## archive_recent

List recent archive records by date.

**Parameters:**

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `after` | string | yes | — | Only records after date (YYYY-MM-DD) |
| `source` | string | no | all | Filter: `hla` or `pensieve` |
| `limit` | integer | no | 20 | Maximum results |
