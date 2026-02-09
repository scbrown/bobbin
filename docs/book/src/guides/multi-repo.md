---
title: Multi-Repo
description: Indexing and searching across multiple repositories with the --repo flag
tags: [multi-repo, guide]
status: draft
category: guide
related: [cli/search.md, cli/index.md]
commands: [search, index]
---

# Multi-Repo

Many projects span multiple repositories — a backend, a frontend, shared libraries, infrastructure configs. Bobbin's multi-repo support lets you index all of them into a single store and search across everything at once, or filter to a specific repo when you need to.

## How it works

Every chunk in bobbin's index is tagged with a **repository name**. By default this is `"default"`, but you can set it explicitly with the `--repo` flag during indexing. At search time, you can filter by repo name or search across all of them.

The index itself lives in a single `.bobbin/` directory. Multiple repos feed into the same LanceDB vector store and SQLite metadata store.

## Setting up multi-repo indexing

### Step 1: Initialize bobbin

Pick a location for your shared index. This can be any directory — it doesn't need to be inside any of the repositories:

```bash
mkdir ~/code-index && cd ~/code-index
bobbin init
```

Or use an existing repo's `.bobbin/` as the shared store:

```bash
cd ~/projects/backend
bobbin init
```

### Step 2: Index each repository

Use `--repo` to name each repository and `--source` to point at its source directory:

```bash
# Index the backend
bobbin index --repo backend --source ~/projects/backend

# Index the frontend
bobbin index --repo frontend --source ~/projects/frontend

# Index shared libraries
bobbin index --repo shared-lib --source ~/projects/shared-lib
```

Each repository's chunks are tagged with the name you provide. Files from different repos coexist in the same index.

### Step 3: Search

Search across everything:

```bash
bobbin search "user authentication"
```

Results show which repo each chunk came from. Or filter to a specific repo:

```bash
bobbin search "user authentication" --repo backend
```

## Practical workflows

### Cross-repo search

You're debugging an issue that spans the API and the frontend. Search both at once:

```bash
bobbin search "session token validation"
```

Results from both repos appear in a single ranked list, so you can see the backend's token generation alongside the frontend's token handling.

### Scoped grep

Find a specific identifier within one repo:

```bash
bobbin grep "UserProfile" --repo frontend
```

This avoids noise from the backend's different `UserProfile` type.

### Cross-repo context assembly

Assemble context that spans repositories:

```bash
bobbin context "refactor the authentication flow" --content full
```

Context assembly pulls from all indexed repos by default. The coupling expansion step works within each repo's git history (since repos have separate git histories), but the initial search spans everything.

To focus on one repo:

```bash
bobbin context "refactor auth" --repo backend --content full
```

### Keeping the index current

Re-index individual repos as they change:

```bash
# Only re-index the backend (incremental)
bobbin index --repo backend --source ~/projects/backend --incremental

# Force re-index the frontend
bobbin index --repo frontend --source ~/projects/frontend --force
```

Incremental mode (`--incremental`) skips files whose content hash hasn't changed, making updates fast.

### Watch mode for multiple repos

Run separate watchers for each repo:

```bash
# Terminal 1: watch backend
bobbin watch --repo backend --source ~/projects/backend

# Terminal 2: watch frontend
bobbin watch --repo frontend --source ~/projects/frontend
```

Each watcher monitors its source directory and updates the shared index with the correct repo tag. See [Watch & Automation](watch-automation.md) for details on setting these up as background services.

## Naming conventions

Choose repo names that are short and meaningful. They appear in search results and are used in `--repo` filters:

```bash
# Good — short, descriptive
bobbin index --repo api --source ~/projects/api-server
bobbin index --repo web --source ~/projects/web-client
bobbin index --repo infra --source ~/projects/infrastructure

# Avoid — too long or ambiguous
bobbin index --repo my-company-api-server-v2 --source ...
```

## Documentation alongside code

A common multi-repo pattern is indexing documentation repos alongside their code counterparts. This lets you cross-reference docs and implementation:

```bash
# Index the code
bobbin index --repo api --source ~/projects/api-server

# Index the documentation
bobbin index --repo docs --source ~/projects/docs-site/src

# Index a wiki
bobbin index --repo wiki --source ~/projects/repo.wiki
```

Now you can search across both code and documentation:

```bash
# Find the docs explaining a feature
bobbin search "rate limiting configuration" --repo docs

# Find the code implementing what the docs describe
bobbin search "rate limiter middleware" --repo api

# Search everything at once
bobbin search "rate limiting"
```

You can also filter by chunk type across repos. For example, find all tables in documentation:

```bash
bobbin search "API endpoints" --repo docs --type table
```

Or find code examples in the docs:

```bash
bobbin search "authentication example" --repo docs --type code_block
```

For detailed guidance on indexing documentation, see [Indexing Documentation](documentation.md).

## Monorepo alternative

If your code is in a monorepo, you don't need multi-repo indexing. A single `bobbin index` covers everything. But you might still use `--repo` to logically partition a monorepo:

```bash
cd ~/monorepo
bobbin init

# Index different top-level directories as separate "repos"
bobbin index --repo services --source ./services
bobbin index --repo packages --source ./packages
bobbin index --repo tools --source ./tools
```

This lets you filter searches to `--repo services` without the overhead of managing separate worktrees.

## Limitations

- **Git coupling is per-repo.** Temporal coupling analysis uses each repository's git history. Cross-repo coupling (files from different repos that change at the same time) is not tracked.
- **Config is shared.** All repos indexed into the same `.bobbin/` share the same `config.toml` settings (include/exclude patterns, embedding model, etc.). If repos need different include patterns, you'll need to manage that at the indexing level.
- **No automatic discovery.** You must explicitly index each repo. There's no "scan this directory for repos" feature.

## Next steps

- [Watch & Automation](watch-automation.md) — keep multi-repo indexes fresh
- [Searching](searching.md) — search techniques that work across repos
- [Context Assembly](context-assembly.md) — cross-repo context bundles
- [`index` CLI reference](../cli/index.md) — full indexing options
