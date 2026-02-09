---
title: Context Assembly
description: Assembling task-relevant context from search results and git history
tags: [context, guide]
status: draft
category: guide
related: [cli/context.md, cli/search.md]
commands: [context]
---

# Context Assembly

When you need to understand a task's full scope, searching for individual code snippets isn't enough. The `bobbin context` command builds a **context bundle** — a curated set of code chunks plus their temporally coupled neighbors — sized to fit a budget you control.

## How it works

Context assembly follows three steps:

1. **Search** — runs hybrid search for your query and collects the top-ranked code chunks.
2. **Coupling expansion** — for each result, looks up files that frequently change together in git history (temporal coupling) and pulls in related chunks.
3. **Budget fitting** — trims the assembled context to stay within a line budget, prioritizing the highest-relevance items.

The result is a focused package of code that's relevant to your task and includes the surrounding files you'd likely need to touch.

## Basic usage

```bash
bobbin context "fix the login validation bug"
```

This returns up to 500 lines (the default budget) of context: the most relevant code chunks for "login validation" plus files that are temporally coupled to them.

## Controlling the output

### Budget

The `--budget` flag sets the maximum number of content lines:

```bash
# Small, focused context
bobbin context "auth token refresh" --budget 200

# Larger context for a broad refactoring task
bobbin context "refactor database layer" --budget 2000
```

A tighter budget forces bobbin to be more selective, keeping only the highest-scored results. A larger budget includes more coupled files and lower-ranked matches.

### Content mode

Choose how much code to include per chunk:

```bash
# Full source code (good for feeding to an AI agent)
bobbin context "add caching to API" --content full

# 3-line preview per chunk (default in terminal)
bobbin context "add caching to API" --content preview

# Paths and metadata only (useful for planning)
bobbin context "add caching to API" --content none
```

**`full`** is the most useful mode when you're assembling context for an AI coding assistant — it gets the actual code, not just pointers to it.

**`preview`** is the default when output goes to a terminal. It gives you enough to decide if a result is relevant without flooding your screen.

**`none`** returns file paths, line ranges, chunk types, and relevance scores. Useful when you just need to know *which* files matter.

### Coupling depth

Control how aggressively bobbin expands via temporal coupling:

```bash
# No coupling expansion — search results only
bobbin context "auth" --depth 0

# One level of coupling (default)
bobbin context "auth" --depth 1

# Two levels — coupled files of coupled files
bobbin context "auth" --depth 2
```

Depth 0 gives you raw search results. Depth 1 adds files that change alongside those results. Depth 2 goes one step further, which is useful for understanding ripple effects across a codebase but can get noisy.

### Fine-tuning coupling

```bash
# Only include strongly coupled files
bobbin context "auth" --coupling-threshold 0.3

# Include more coupled files per seed
bobbin context "auth" --max-coupled 5
```

The `--coupling-threshold` filters out weak relationships. The default of 0.1 is permissive — raise it if your context is pulling in too many loosely related files.

## Practical workflows

### Feeding context to an AI agent

The primary use case for `bobbin context` is giving an AI coding assistant the right code for a task:

```bash
# Assemble context and pipe it to clipboard
bobbin context "implement rate limiting for API endpoints" --content full --budget 1000 | pbcopy
```

Paste the output into your AI conversation. The context bundle includes file paths, line ranges, and full source code — everything the agent needs to understand and modify the relevant code.

### Scoping a refactoring task

Before starting a refactoring, use context assembly to understand what you'll need to touch:

```bash
bobbin context "error handling patterns" --content none --budget 1000
```

The `--content none` output gives you a manifest of files, chunk types, and coupling relationships. Use it as a checklist.

### Understanding blast radius

When you're about to change a widely-used module, check what's coupled to it:

```bash
bobbin context "DatabasePool configuration" --depth 2 --content preview
```

Depth 2 shows you not just what directly uses the database pool, but what changes alongside those users. This reveals the true blast radius of your change.

### JSON output for automation

```bash
bobbin context "auth" --json | jq '.files[].path'
```

The JSON output includes relevance scores, coupling scores, budget usage, and full chunk metadata — useful for building custom tooling on top of bobbin.

## How coupling expansion works

During indexing, bobbin analyzes git history to find files that frequently appear in the same commits. This is called **temporal coupling**. The `[git]` section in your config controls the analysis:

```toml
[git]
coupling_enabled = true
coupling_depth = 1000      # Commits to analyze
coupling_threshold = 3     # Minimum co-changes to establish a link
```

When `bobbin context` runs with `--depth 1` (the default), it:

1. Takes each search result file.
2. Looks up its top coupled files (limited by `--max-coupled`, default 3).
3. Filters by `--coupling-threshold` (default 0.1).
4. Includes relevant chunks from those coupled files in the context bundle.

This means if `auth.rs` frequently changes alongside `session.rs` and `middleware.rs`, a query about authentication will automatically surface all three.

## Next steps

- [Git Coupling](git-coupling.md) — deep dive into temporal coupling analysis
- [Searching](searching.md) — understand the search engine that seeds context assembly
- [`context` CLI reference](../cli/context.md) — full flag reference
