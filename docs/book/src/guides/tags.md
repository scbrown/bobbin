# Tags & Effects

Tags let you control how bobbin scores and filters search results. By assigning
tags to chunks via pattern rules, then configuring effects (boost, demote, exclude,
pin), you tune search quality without changing the indexing pipeline.

## Overview

The tag system has three layers:

1. **Rules** — Glob patterns that assign tags to files during indexing
2. **Effects** — Score adjustments applied when tagged chunks appear in results
3. **Scoped Effects** — Role-specific overrides (e.g., boost lifecycle docs only for witness)

Configuration lives in `.bobbin/tags.toml` at the root of your bobbin data directory.

## Tags Configuration

### Rules

Rules match file paths and assign tags:

```toml
[[rules]]
pattern = "**/CHANGELOG.md"
tags = ["type:changelog"]

[[rules]]
pattern = "**/ansible/roles/*/tasks/main.yml"
tags = ["domain:iac", "criticality:high"]
repo = "goldblum"  # Optional: only apply when indexing this repo
```

- `pattern` — Standard glob pattern matched against relative file paths
- `tags` — List of tag strings to assign (convention: `namespace:value`)
- `repo` — Optional repo scope (only applies during indexing of that repo)

> **Glob pattern note:** Patterns are matched against paths relative to the repo
> root (e.g., `snapshots/ian/2026-03-12.md`, not the absolute path). The `**/`
> prefix matches both root-level and nested paths — `**/CHANGELOG.md` matches
> both `CHANGELOG.md` and `docs/CHANGELOG.md`.

### Effects

Effects modify scores when tagged chunks appear in search results:

```toml
[effects."type:changelog"]
boost = -0.6      # Demote: score *= (1 + boost) = 0.4

[effects."auto:init"]
exclude = true     # Remove entirely from results

[effects."criticality:high"]
boost = 0.2        # Boost: score *= 1.2

[effects."feedback:hot"]
boost = 0.3
pin = true         # Always include, bypass relevance threshold
budget_reserve = 20  # Reserve 20 lines of budget for pinned chunks
```

Score formula: `final_score = raw_score * product(1 + boost)` for all matching tags,
clamped to `[0.01, 10.0]`.

| Field | Type | Description |
|-------|------|-------------|
| `boost` | float | Score multiplier. Positive = boost, negative = demote. |
| `exclude` | bool | Remove chunks with this tag from results entirely. |
| `pin` | bool | Bypass relevance threshold; always include if budget allows. |
| `budget_reserve` | int | Lines of budget reserved for pinned chunks. |

### Scoped Effects

Override global effects for specific roles:

```toml
# Globally demote lifecycle docs
[effects."domain:lifecycle"]
boost = -0.3

# But boost them for witness role
[[effects_scoped]]
tag = "domain:lifecycle"
role = "*/witness"
boost = 0.2

# Exclude internal docs for external users
[[effects_scoped]]
tag = "type:internal"
role = "external/*"
exclude = true
```

The `role` field supports glob patterns. When a request includes a role
(via `--role` flag or `BOBBIN_ROLE` env var), scoped effects override
global effects for matching roles.

## Built-in Tags

Bobbin automatically assigns some tags during indexing:

| Tag | Applied to |
|-----|-----------|
| `auto:init` | Go `init()` functions |
| `auto:test` | Test functions (Go, Rust, Python, JS) |
| `auto:docs` | Documentation files |

## Tag Naming Conventions

Use `namespace:value` format for clarity:

| Namespace | Purpose | Examples |
|-----------|---------|----------|
| `auto:` | Auto-assigned by bobbin | `auto:test`, `auto:init` |
| `type:` | Document/chunk type | `type:changelog`, `type:design`, `type:eval` |
| `role:` | Agent instruction files | `role:claude-md`, `role:agents-md` |
| `domain:` | Domain/topic area | `domain:iac`, `domain:lifecycle`, `domain:comms` |
| `criticality:` | Importance level | `criticality:high` |
| `feedback:` | Feedback-driven scoring | `feedback:hot`, `feedback:cold` |

## Example: Reducing Noise

A common pattern is to identify noisy document types and demote them:

```toml
# Problem: Changelog entries surface in unrelated queries
[effects."type:changelog"]
boost = -0.6  # Demote to 40% of original score

[[rules]]
pattern = "**/CHANGELOG.md"
tags = ["type:changelog"]

# Problem: Eval task YAML files match cargo/build queries
[effects."type:eval"]
boost = -0.5

[[rules]]
pattern = "**/eval/tasks/**"
tags = ["type:eval"]
repo = "bobbin"

# But un-demote for the developer who works on evals
[[effects_scoped]]
tag = "type:eval"
role = "aegis/crew/stryder"
boost = 0.0
```

## Applying Changes

After modifying `tags.toml`:

1. **Reindex** affected repos to apply new tag rules to chunks:
   ```bash
   bobbin index /path/to/data --repo <name> --source /path/to/repo --force
   ```

2. **Restart the server** if running in HTTP mode (tags config is loaded at startup):
   ```bash
   sudo systemctl restart bobbin
   ```

Tag effects are applied during context assembly (`/context` endpoint), so
restarting the server picks up both new effect weights and new tag assignments
from the reindex.

> **Note:** The `/search` endpoint returns raw relevance scores without tag
> effects. To verify that tag boosts/demotions are working, test with the
> `/context` endpoint or the `bobbin context` CLI command.

## Debugging Tags

To see which tags are assigned to search results, use the JSON output:

```bash
bobbin search "your query" --json | jq '.results[].tags'
```

Check tag coverage after indexing:
```
✓ Indexed 942 files (12270 chunks)
  Tags: 41096 tagged, 21327 untagged chunks
```

High untagged counts are normal — not every chunk needs tags. Focus rules on
document types that cause noise (changelogs, templates, test files) or that
need boosting (critical configs, design docs).

To filter search results by tag:
```bash
bobbin search "your query" --tag type:guide        # Include only guides
bobbin search "your query" --exclude-tag type:eval  # Exclude eval artifacts
```
