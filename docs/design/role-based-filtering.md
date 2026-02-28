# Bobbin Role-Based Repo Filtering (§69)

**Status**: Design spec
**Directive**: §69 — certain repos should only show for certain roles, highly configurable per project/repo
**Bead**: aegis-5w0

## Problem

Bobbin indexes 19 repos. All search results are visible to all callers — agents
and humans alike. Some repos are personal (cv, resume, personal-planning), some
are rig-specific (aegis config), and most are shared infrastructure. There's no
way to scope search results by who's asking.

## Design

### Caller Identity

Callers pass a `role` query parameter on search endpoints:

```
/search?q=auth+flow&role=aegis/crew/ian
/search?q=deployment&role=human
/context?q=fix+login+bug&role=bobbin/polecats/alpha
```

If no `role` is provided, the `default` role is used. This preserves backward
compatibility — existing callers see everything they saw before.

### Config Format

New `[access]` section in `config.toml`:

```toml
[access]
# When true, repos not listed in any rule are visible to all roles.
# When false, repos must be explicitly granted. Default: true (open by default).
default_allow = true

# Role definitions. Each role has allow/deny lists.
# Patterns support globs: "stiwi/*" matches all repos under stiwi/.
# Deny takes precedence over allow.

[[access.roles]]
name = "human"
# Human sees everything — no restrictions
allow = ["*"]

[[access.roles]]
name = "default"
# Default role (no role param): exclude personal repos
deny = ["personal-planning", "cv", "resume"]

[[access.roles]]
name = "aegis/*"
# Aegis agents see infra + aegis-specific repos
allow = ["aegis", "bobbin", "gastown", "homelab-mcp", "orchestrator", "beads", "goldblum", "tapestry", "shanty", "hla-records"]

[[access.roles]]
name = "bobbin/*"
# Bobbin workers see bobbin + its dependencies
allow = ["bobbin", "aegis", "homelab-mcp"]
```

### Role Matching

Roles match hierarchically using prefix matching:

1. Exact match: `aegis/crew/ian` matches role `aegis/crew/ian`
2. Wildcard: `aegis/crew/ian` matches role `aegis/*`
3. Default: if no role matches, use `default` role
4. No config: if no `[access]` section exists, all repos visible (backward compat)

Most specific match wins. If multiple patterns match at the same depth, merge
their allow/deny lists (deny wins on conflict).

### Filtering Point

Filtering happens **post-search, pre-response**. The search engine runs against
the full index (preserving relevance ranking), then results from denied repos
are stripped before returning to the caller.

This is simpler than partitioned indexes and means the index stays unified.
The performance cost is negligible — filtering a few results from a response
of 10-50 items is trivial.

### Affected Endpoints

All endpoints that return repo-scoped results:

| Endpoint | Filter on |
|----------|-----------|
| `/search` | `file_path` repo prefix |
| `/grep` | `file_path` repo prefix |
| `/context` | assembled file paths |
| `/repos` | repo list |
| `/repos/{name}/files` | repo name |
| `/related` | file paths |
| `/refs` | file paths |
| `/similar` | file paths |
| `/hotspots` | file paths |
| `/impact` | file paths |
| `/review` | file paths |
| `/prime` | repo stats |

Endpoints NOT affected: `/healthz`, `/status`, `/metrics`, `/beads`,
`/archive/*`, `/webhook/push`, `/commands`.

### Implementation Plan

1. Add `AccessConfig` struct to `config.rs` with `default_allow`, `roles` vec
2. Add `RoleResolver` that takes a role string, matches against config, returns allowed/denied repo set
3. Add `role` query param to `SearchParams` and other param structs
4. Add filtering middleware or helper that strips denied repos from results
5. Update `/repos` endpoint to respect role filtering
6. Tests: role matching, deny-over-allow, default behavior, no-config backward compat

### Migration

- No breaking changes. Existing callers without `role` param get `default` role.
- If no `[access]` section in config, everything is visible (current behavior).
- Deploy config change separately from code change — code deploys first with
  filtering disabled, then config enables it.

### Future Extensions

- Token-based auth instead of honor-system role param
- Per-file filtering (not just per-repo)
- Audit log of who searched what
- Role inheritance (aegis/crew/ian inherits from aegis/*)
