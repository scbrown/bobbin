# Bobbin Primer

Bobbin is a local-first code context engine — semantic search, keyword search, git coupling analysis, and context assembly, all running locally with no API keys.

## Commands

| Command | What it does | Docs |
|---------|-------------|------|
| `bobbin search <query>` | Hybrid semantic + keyword search | `docs/book/src/guides/searching.md` |
| `bobbin context <query>` | Budget-controlled context bundles | `docs/book/src/cli/context.md` |
| `bobbin grep <pattern>` | Regex/keyword search | `docs/book/src/cli/grep.md` |
| `bobbin index` | Build/update search index | `docs/book/src/cli/index.md` |
| `bobbin calibrate` | Auto-tune search params via git history | `docs/book/src/cli/calibrate.md` |
| `bobbin related <file>` | Files that change together (git coupling) | `docs/book/src/guides/git-coupling.md` |
| `bobbin refs <symbol>` | Symbol definitions and usages | `docs/book/src/cli/refs.md` |
| `bobbin hotspots` | High-churn, high-complexity files | `docs/book/src/cli/hotspots.md` |
| `bobbin impact <file>` | Predict affected files from a change | `docs/book/src/cli/impact.md` |
| `bobbin review <range>` | Diff-aware context for code review | `docs/book/src/cli/review.md` |
| `bobbin history <file>` | Commit history and churn stats | `docs/book/src/cli/history.md` |
| `bobbin serve` | HTTP API / MCP server | `docs/book/src/mcp/overview.md` |
| `bobbin hook` | Claude Code hook management | `docs/book/src/guides/hooks.md` |
| `bobbin tag` | Semantic tag management | `docs/book/src/guides/tags.md` |
| `bobbin status` | Index stats and calibration state | `docs/book/src/cli/status.md` |

All commands support `--json`, `--quiet`, `--verbose`, and `--help`.

## Configuration

Layered: compiled defaults → global (`~/.config/bobbin/config.toml`) → per-repo (`.bobbin/config.toml`) → calibration (`.bobbin/calibration.json`) → CLI flags. Tables merge recursively; arrays and scalars replace.

→ Full reference: `docs/book/src/config/reference.md`

Key sections: `[index]` file patterns, `[search]` hybrid weights, `[embedding]` model settings, `[git]` coupling, `[hooks]` injection tuning, `[sources]` remote browse URLs, `[access]` role-based filtering.

## Search Modes

- **hybrid** (default): Combines semantic + keyword via RRF. Weight controlled by `search.semantic_weight` (0.0–1.0).
- **semantic**: Vector similarity only.
- **keyword**: Full-text search only.
- **regex**: Pattern matching (`bobbin grep`).

## Key Features

| Feature | Summary | Docs |
|---------|---------|------|
| Calibration | Grid sweep of search params against git history | `docs/book/src/cli/calibrate.md` |
| Tags & effects | Pattern-based chunk tagging with score boosts/demotions | `docs/book/src/guides/tags.md`, `docs/book/src/guides/tags-playbook.md` |
| Hooks | Auto-inject context into Claude Code prompts | `docs/book/src/guides/hooks.md`, `docs/book/src/config/hooks.md` |
| RBAC | Role-based repo/path access control | `docs/book/src/config/reference.md` (`[access]`) |
| Multi-repo | Index multiple repos into one store | `docs/book/src/guides/multi-repo.md` |
| Reactions | Pattern-triggered hook actions | `.bobbin/reactions.toml` |
| Feedback | Agent ratings improve search quality | `docs/book/src/guides/feedback.md` |
| Archive | Index structured markdown records | `docs/book/src/guides/archive.md` |
| MCP server | Expose tools via Model Context Protocol | `docs/book/src/mcp/overview.md` |

## Architecture

```
Repository → Tree-sitter / pulldown-cmark → ONNX Embedder → LanceDB + SQLite
             (structural parsing)           (384-dim)        (vectors + FTS + coupling)
```

Languages: Rust, TypeScript, Python, Go, Java, C++, Markdown. Others use line-based chunking.

## MCP Tools

When running as MCP server (`bobbin serve`): `search`, `grep`, `context`, `related`, `find_refs`, `list_symbols`, `read_chunk`, `hotspots`, `impact`, `review`, `similar`, `dependencies`, `file_history`, `status`, `search_beads`, `commit_search`, `prime`.

→ Full tool reference: `docs/book/src/mcp/tools.md`

## Quick Start

```bash
bobbin init && bobbin index
bobbin search "error handling"
bobbin context "fix the login bug"
```

## Documentation Index

All docs live in `docs/book/`:

| Section | Path |
|---------|------|
| Guides | `docs/book/src/guides/` — searching, context, git coupling, multi-repo, tags, hooks |
| CLI reference | `docs/book/src/cli/` — all commands with options and examples |
| Configuration | `docs/book/src/config/` — full config.toml reference, section-by-section |
| Architecture | `docs/book/src/architecture/` — storage, embedding, language support |
| MCP | `docs/book/src/mcp/` — tools reference, client config, HTTP mode |
