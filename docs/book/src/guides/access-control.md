# Access Control (RBAC)

Bobbin supports role-based access control to restrict which repos and file paths are visible in search results. This is especially useful in multi-agent environments like Gas Town, where different agents should only see repos relevant to their domain.

## How It Works

Filtering happens **post-search, pre-response**: the search engine runs against the full index, then results from denied repos/paths are stripped. This keeps relevance ranking intact while enforcing access boundaries.

All commands that return repo-scoped results are filtered: `search`, `grep`, `context`, `related`, `refs`, `similar`, `hotspots`, `impact`, `review`, and `hook inject`. The same filtering applies to HTTP API endpoints.

## Role Resolution

Your role is resolved in priority order:

| Priority | Source | Example |
|----------|--------|---------|
| 1 | `--role` CLI flag | `bobbin search --role human "auth"` |
| 2 | `BOBBIN_ROLE` env var | `export BOBBIN_ROLE=human` |
| 3 | `GT_ROLE` env var | Set by Gas Town automatically |
| 4 | `BD_ACTOR` env var | Set by Gas Town automatically |
| 5 | Default | `"default"` |

Gas Town agents get filtering for free — their `BD_ACTOR` is already set.

## Configuration

Add an `[access]` section to `.bobbin/config.toml`:

```toml
[access]
# When true, repos not in any deny list are visible to all roles.
# When false, repos must be explicitly allowed. Default: true.
default_allow = true

# Human sees everything
[[access.roles]]
name = "human"
allow = ["*"]

# Default role: exclude personal repos
[[access.roles]]
name = "default"
deny = ["cv", "resume", "personal-planning"]

# Aegis crew: infra-focused repos
[[access.roles]]
name = "aegis/crew/*"
allow = ["aegis", "bobbin", "gastown", "homelab-mcp", "goldblum", "hla-records"]
deny = ["cv", "resume", "personal-planning"]

# Specific agent override
[[access.roles]]
name = "aegis/crew/ian"
allow = ["aegis", "bobbin", "personal-planning"]
deny = []

# Path-level restrictions within allowed repos
[[access.roles]]
name = "bobbin/*"
allow = ["bobbin", "homelab-mcp"]
deny_paths = ["harnesses/*/CLAUDE.md", "crew/*/CLAUDE.md"]
```

## Role Matching

Roles match hierarchically — most specific wins:

1. **Exact match**: `aegis/crew/ian` matches role named `aegis/crew/ian`
2. **Wildcard match**: `aegis/crew/ian` matches `aegis/crew/*` (prefix `/*`)
3. **Less specific wildcard**: `aegis/polecats/alpha` matches `aegis/*`
4. **Default fallback**: if no pattern matches, uses role named `default`
5. **No config**: if no `[access]` section exists, everything is visible

## Deny Precedence

- **Deny always beats allow**: `deny = ["secret-repo"]` wins even with `allow = ["*"]`
- **deny_paths**: glob patterns that block specific file paths within allowed repos
- **default_allow**: controls visibility of repos not mentioned in any rule

## Examples

### Lock down everything, grant explicitly

```toml
[access]
default_allow = false

[[access.roles]]
name = "human"
allow = ["*"]

[[access.roles]]
name = "default"
allow = ["bobbin"]
```

Agents with no role match see only `bobbin`. Humans see everything.

### Block sensitive paths

```toml
[[access.roles]]
name = "default"
deny = ["cv"]
deny_paths = ["harnesses/*/CLAUDE.md", ".env*", "secrets/**"]
```

### HTTP API usage

```text
GET /search?q=auth+flow&role=aegis/crew/ian
GET /context?q=fix+login&role=human
```

## No Config = No Filtering

If there's no `[access]` section in config.toml, RBAC is completely disabled and all repos are visible to all callers. This is the default for backward compatibility.
