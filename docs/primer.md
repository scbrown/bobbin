# Bobbin Primer

Bobbin is a local-first code context engine. It indexes your codebase for semantic search, keyword search, and git coupling analysis — all running locally with no API keys required.

## What Bobbin Does

- **Semantic search**: Find code by meaning, not just text (`bobbin search "authentication middleware"`)
- **Keyword/regex search**: Exact pattern matching via full-text search (`bobbin grep "TODO"`)
- **Task-aware context**: Build budget-controlled context bundles from search + git history (`bobbin context "fix the login bug"`)
- **Git temporal coupling**: Discover files that change together (`bobbin related src/auth.rs`)
- **Code hotspots**: Find high-churn, high-complexity files (`bobbin hotspots`)
- **Symbol references**: Find definitions and usages across the codebase (`bobbin refs parse_config`)
- **Auto-calibration**: Tune search parameters against git history (`bobbin calibrate`)
- **Multi-repo indexing**: Index multiple repos into a single store (`bobbin index --repo name --source path`)
- **Claude Code hooks**: Automatic context injection into agent prompts (`bobbin hook install`)
- **MCP server**: Expose all capabilities to AI agents via Model Context Protocol (`bobbin serve`)

## Architecture

```
Repository Files → Tree-sitter/pulldown-cmark → ONNX Embedder → LanceDB + SQLite
                   (structural parsing)         (384-dim vectors) (storage)
```

**Storage**: LanceDB holds chunks, vectors, and FTS index. SQLite holds temporal coupling and metadata.

**Search**: Hybrid search combines semantic (vector ANN) and keyword (FTS) results via Reciprocal Rank Fusion (RRF).

**Parsing**: Tree-sitter extracts functions, classes, structs, traits, etc. from 7 languages. Markdown is parsed into sections, tables, and code blocks.

## Supported Languages

Rust, TypeScript, Python, Go, Java, C++, Markdown. Other file types use line-based chunking.

## Key Commands

| Command | Description |
|---------|-------------|
| `bobbin init` | Initialize in current repository |
| `bobbin index` | Build/update the search index |
| `bobbin search <query>` | Semantic + keyword hybrid search |
| `bobbin context <query>` | Task-aware context assembly |
| `bobbin grep <pattern>` | Keyword/regex search |
| `bobbin calibrate` | Auto-tune search parameters against git history |
| `bobbin related <file>` | Find temporally coupled files |
| `bobbin refs <symbol>` | Find symbol definitions and usages |
| `bobbin history <file>` | Commit history and churn stats |
| `bobbin hotspots` | High-churn + high-complexity files |
| `bobbin impact <file>` | Predict affected files from a change |
| `bobbin review <range>` | Diff-aware context for code review |
| `bobbin status` | Index statistics and calibration state |
| `bobbin serve` | Start HTTP API / MCP server |
| `bobbin hook` | Manage Claude Code hooks |
| `bobbin tag` | Manage semantic tags for chunks |
| `bobbin prime` | This overview |

All commands support `--json`, `--quiet`, and `--verbose` global flags.

## Configuration Hierarchy

Bobbin uses a layered configuration system. Each layer overrides the one above:

1. **Compiled defaults** — sensible out-of-the-box values
2. **Global config** (`~/.config/bobbin/config.toml`) — machine-wide defaults
3. **Per-repo config** (`.bobbin/config.toml`) — project-specific settings
4. **Calibration** (`.bobbin/calibration.json`) — auto-tuned search parameters
5. **CLI flags** — per-invocation overrides

The global config is useful for setting `[server].url`, `[hooks]` preferences,
and `[search]` defaults that apply everywhere. Per-repo config overrides only
the fields you set — unset fields inherit from global. Tables merge recursively;
arrays and scalars replace wholesale.

Key settings:

- `index.include` / `index.exclude`: File glob patterns
- `embedding.model`: Embedding model (default: all-MiniLM-L6-v2)
- `search.semantic_weight`: Hybrid search balance (0.0–1.0, default: 0.7)
- `search.recency_weight` / `search.recency_half_life_days`: Temporal decay
- `search.doc_demotion`: Downweight documentation in rankings
- `git.coupling_enabled`: Enable temporal coupling analysis
- `hooks.gate_threshold`: Minimum relevance score for context injection
- `hooks.budget`: Maximum lines of injected context
- `hooks.skip_prefixes`: Operational commands that skip injection
- `hooks.keyword_repos`: Query keywords → repo scoping rules

See `docs/book/` for the full configuration reference and feature guides.

## Advanced Features

### Calibration

`bobbin calibrate` sweeps search parameters (semantic weight, RRF k, doc demotion,
recency, coupling depth, bridge mode) against your git history to find the optimal
configuration. Results are saved to `.bobbin/calibration.json` and automatically
used by search, context, and hooks.

For multi-repo setups: `bobbin calibrate --repo <name>` calibrates against a
specific indexed repository's git history.

### Tags & Effects

Chunks can be tagged with semantic labels via pattern rules in `tags.toml`.
Tags drive scoring effects — boosts, demotions, and exclusions — that are
applied during context assembly. Effects can be scoped to specific roles
(e.g., boost runbook docs only for the witness agent).

Tag effects apply via the `/context` API path (used by hooks and the context
command). The `/search` CLI returns raw LanceDB scores without tag effects.

### Hooks Integration

`bobbin hook install` adds automatic context injection to Claude Code via
`UserPromptSubmit` hooks. Advanced features include:

- **Reactions**: Pattern-triggered actions (`.bobbin/reactions.toml`)
- **Noise filtering**: Automated message detection and skip_prefixes
- **Progressive reducing**: Session-level delta injection (only new chunks)
- **Feedback**: Periodic prompts to rate injection quality
- **Repo affinity**: Boost files from the agent's current repo

### Role-Based Access Control

The `[access]` config section controls which repos/paths are visible to
which roles. Roles have `allow`/`deny` repo patterns and `deny_paths` for
fine-grained filtering.

### Feedback System

Agents can rate injected context quality. Ratings are stored in `feedback.db`
and used to tag chunks as `feedback:hot` (useful) or `feedback:cold` (noise),
improving future search relevance.

### Archive Integration

The `[archive]` config section enables indexing of structured markdown records
(e.g., HLA directives, agent memories) with YAML frontmatter schema matching
and webhook-triggered re-indexing.

## MCP Tools

When running as an MCP server (`bobbin serve`), these tools are available:

| Tool | Description |
|------|-------------|
| `search` | Semantic/hybrid/keyword code search |
| `grep` | Pattern matching with regex support |
| `context` | Task-aware context assembly |
| `related` | Find temporally coupled files |
| `find_refs` | Find symbol definitions and usages |
| `list_symbols` | List all symbols in a file |
| `read_chunk` | Read specific code sections |
| `hotspots` | Find high-churn, high-complexity files |
| `impact` | Predict files affected by a change |
| `review` | Diff-aware context for code review |
| `similar` | Find similar code or duplicate clusters |
| `dependencies` | Show import dependencies for a file |
| `file_history` | Commit history and stats for a file |
| `status` | Index statistics and health |
| `search_beads` | Search indexed issues/tasks |
| `commit_search` | Semantic git commit search |
| `prime` | Get this project overview with live stats |

## Quick Start

```bash
cargo install bobbin
cd your-project
bobbin init && bobbin index
bobbin search "error handling"
```

## Documentation

Full documentation is in the mdbook at `docs/book/`:

- **Guides**: Searching, context assembly, git coupling, multi-repo, tags, hooks, watch
- **CLI Reference**: All 23 commands with options and examples
- **Configuration**: Full config.toml reference with section-by-section guides
- **Architecture**: Storage, embedding pipeline, language support
- **MCP Integration**: Tools reference, client configuration, HTTP mode
