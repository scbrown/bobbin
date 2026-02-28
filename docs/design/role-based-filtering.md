# Bobbin Role-Based Repo Filtering (§69)

**Status**: Design spec
**Directive**: §69 — certain repos should only show for certain roles, highly configurable per project/repo
**Bead**: aegis-5w0

## Problem

Bobbin indexes 19 repos. All search results are visible to all callers — agents
and humans alike. Some repos are personal (cv, resume, personal-planning), some
are rig-specific (aegis config), and most are shared infrastructure. There's no
way to scope search results by who's asking.

This applies equally to CLI usage (`bobbin search`, `bobbin context`, etc.) and
HTTP API usage (`/search?q=...`, `/context?q=...`).

## Design

### Caller Identity

Role is resolved in priority order:

1. **CLI flag**: `--role aegis/crew/ian` (global flag on all commands)
2. **Environment variable**: `BOBBIN_ROLE=aegis/crew/ian`
3. **Gas Town env fallback**: `GT_ROLE` or `BD_ACTOR` (auto-detected from agent environment)
4. **HTTP query param**: `?role=aegis/crew/ian` (API callers)
5. **Default**: `default` role (if nothing else is set)

This means agents in Gas Town get role-based filtering automatically — their
`GT_ROLE` is already set by the environment. No flag needed. Humans running
`bobbin search` locally get `default` unless they set `BOBBIN_ROLE=human`.

### CLI Integration

Global flag on the `Cli` struct, same pattern as `--server` and `--metrics-source`:

```rust
/// Role for access filtering (also reads BOBBIN_ROLE, GT_ROLE, BD_ACTOR)
#[arg(long, global = true, env = "BOBBIN_ROLE")]
role: Option<String>,
```

Role resolution in `Cli::run()`:
```rust
fn resolve_role(&self) -> String {
    self.role.clone()
        .or_else(|| std::env::var("BOBBIN_ROLE").ok())
        .or_else(|| std::env::var("GT_ROLE").ok())
        .or_else(|| std::env::var("BD_ACTOR").ok())
        .unwrap_or_else(|| "default".to_string())
}
```

### HTTP Integration

API callers pass `role` as a query parameter. The HTTP handler resolves it the
same way — explicit param wins, then env fallback, then `default`.

```
/search?q=auth+flow&role=aegis/crew/ian
/context?q=fix+login+bug&role=human
```

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
# Default role (no role param, no env): exclude personal repos
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

Filtering happens **post-search, pre-response** in a shared `RepoFilter` that
both CLI and HTTP handlers call. The search engine runs against the full index
(preserving relevance ranking), then results from denied repos are stripped.

```rust
pub struct RepoFilter {
    allowed: Option<HashSet<String>>,  // None = allow all
    denied: HashSet<String>,
}

impl RepoFilter {
    pub fn from_config(config: &AccessConfig, role: &str) -> Self { ... }
    pub fn is_allowed(&self, repo_name: &str) -> bool { ... }
    pub fn filter_results(&self, results: Vec<SearchResult>) -> Vec<SearchResult> { ... }
}
```

This is simpler than partitioned indexes and means the index stays unified.
The performance cost is negligible — filtering a few results from a response
of 10-50 items is trivial.

### Affected Commands & Endpoints

**CLI commands that filter results:**

| Command | Filter on |
|---------|-----------|
| `bobbin search` | result file paths |
| `bobbin grep` | result file paths |
| `bobbin context` | assembled file paths |
| `bobbin related` | file paths |
| `bobbin refs` | file paths |
| `bobbin similar` | file paths |
| `bobbin hotspots` | file paths |
| `bobbin impact` | file paths |
| `bobbin review` | file paths |
| `bobbin prime` | repo stats |
| `bobbin status` | repo stats (when `--role` set) |
| `bobbin hook inject` | injected context |

**HTTP endpoints** — same set, mapped to their route equivalents.

**Not filtered**: `bobbin index`, `bobbin init`, `bobbin watch`, `bobbin serve`,
`bobbin benchmark`, `bobbin calibrate`, `/healthz`, `/metrics`, `/webhook/push`.

### Implementation Plan

1. Add `AccessConfig` + `RoleConfig` structs to `config.rs`
2. Add `RepoFilter` module with role resolution + filtering logic
3. Add `--role` global flag to `Cli` struct with `BOBBIN_ROLE` env
4. Wire `RepoFilter` into CLI commands that return repo-scoped results
5. Add `role` query param to HTTP handler param structs
6. Wire same `RepoFilter` into HTTP handlers
7. Tests: role matching, deny-over-allow, env fallback chain, default behavior, no-config backward compat

### Migration

- No breaking changes. Existing callers without `--role` or env get `default` role.
- If no `[access]` section in config, everything is visible (current behavior).
- Deploy code first (filtering disabled without config), then add `[access]` to config.
- Gas Town agents get filtering for free once `GT_ROLE`/`BD_ACTOR` is in their env.

### Future Extensions

- Token-based auth instead of honor-system role param
- Per-file filtering (not just per-repo)
- Audit log of who searched what
