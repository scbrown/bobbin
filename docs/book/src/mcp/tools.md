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
