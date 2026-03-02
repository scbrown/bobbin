# Bobbin Tags & Annotations

**Bead**: aegis-gzc4jv
**Author**: aegis/crew/ellie
**Date**: 2026-03-02
**Status**: Draft (rev 2 — incorporates Stiwi feedback)

## Problem

Bobbin injects context based on semantic/keyword relevance, but users and agents
have no way to control _what_ gets injected beyond tuning thresholds. This causes:

1. **Cross-repo noise** — pixelsrc sprite code injected when searching for infra issues
2. **Stale content** — deprecated docs rank high because they're well-written
3. **No filtering** — can't scope search to "architecture docs only"
4. **No feedback** — agents can't signal which injections helped vs wasted budget
5. **No pinning** — can't guarantee critical context always appears

Index groups (aegis-5mi0jj, shipped) solve repo-level scoping. Tags solve
chunk-level and file-level control.

## Design Principles

1. **Pattern-first** — tag via glob patterns, not individual files. Manual per-file
   tagging doesn't scale across 27k+ files.
2. **Convention-driven defaults** — common tags auto-applied from directory structure
   (`tests/` → `test`, `docs/` → `docs`). Zero config for obvious patterns.
3. **Composable with existing scoring** — tags modify the existing RRF pipeline,
   they don't replace it.
4. **Zero-cost default** — untagged chunks behave exactly as today. Tags are opt-in.
5. **Role-scoped effects** — the same tag can mean different things to different roles.
6. **Multiple sources** — built-in conventions, config rules, frontmatter, code
   comments, CLI, agent feedback.
7. **Stryder owns implementation** — this design hands off to bobbin's ranger.

## Tag Model

### Tag Format: Namespaced Tags

Tags use a `namespace:name` format to prevent collision and provide provenance:

```
auto:test           — built-in convention (directory/pattern based)
auto:docs           — built-in convention
auto:config         — built-in convention
auto:internal       — built-in convention
user:canonical      — manually applied via CLI or config rules
user:deprecated     — manually applied
user:security       — manually applied
feedback:hot        — auto-applied from agent usage signals
feedback:cold       — auto-applied from agent non-usage signals
```

Constraints:
- Namespace: `[a-z]+` (one of: `auto`, `user`, `feedback`)
- Name: `[a-z0-9-]+`, max 32 characters
- Full tag max 38 characters (namespace + colon + name)

Short form is allowed in CLI/config — `canonical` expands to `user:canonical`,
`test` resolves to `auto:test` if the built-in exists.

### Built-in Convention Tags (auto namespace)

These are applied automatically at index time based on file path conventions.
No configuration required — they ship with bobbin:

| Pattern | Tag | Rationale |
|---------|-----|-----------|
| `**/test/**`, `**/tests/**`, `**/*_test.*`, `**/*_spec.*` | `auto:test` | Test files |
| `**/docs/**`, `**/*.md`, `**/README*` | `auto:docs` | Documentation |
| `**/internal/**`, `**/private/**` | `auto:internal` | Internal/private code |
| `**/*.toml`, `**/*.yaml`, `**/*.json`, `**/Dockerfile*` | `auto:config` | Config files |
| `**/deprecated/**`, `**/legacy/**` | `auto:deprecated` | Deprecated code |
| `**/vendor/**`, `**/node_modules/**` | `auto:vendored` | Third-party code |
| `**/examples/**`, `**/example/**` | `auto:example` | Example code |

Convention tags can be disabled per-repo in config: `[conventions] disabled = ["auto:vendored"]`

### Tag Sources (Priority Order)

| Source | Scope | Namespace | Example | Applied At |
|--------|-------|-----------|---------|------------|
| Built-in conventions | Pattern → tags | `auto:` | `tests/** → auto:test` | Index time |
| Config rules | Pattern → tags | `user:` | `docs/arch/** → user:canonical` | Index time |
| Frontmatter | Per-file | `user:` | `tags: [canonical, architecture]` | Index time |
| Code comments | Per-chunk | `user:` | `// bobbin:tag security` | Index time |
| CLI manual | File or pattern | `user:` | `bobbin tag add src/auth.rs security` | Stored in tag DB |
| Agent feedback | Per-chunk | `feedback:` | Auto-tagged `feedback:hot` from usage | Post-injection |

When multiple sources assign tags to the same chunk, tags are **unioned** (all
sources contribute, no overrides).

### Tag Storage

#### In LanceDB (chunks table)

Add a `tags` column to the chunks table:

```
chunks table (existing + new):
  id: string
  vector: float[384]
  repo: string
  file_path: string
  ...existing columns...
  tags: string          # NEW — comma-separated sorted namespaced tags, empty string if none
```

Using comma-separated string rather than list type for LanceDB compatibility and
simpler SQL filtering (`tags LIKE '%user:canonical%'`). Tags are sorted
alphabetically to ensure deterministic storage.

#### Tag Rules Store (`.bobbin/tags.toml`)

Separate from `config.toml` to keep tag rules independently versionable:

```toml
# Built-in convention overrides (optional — conventions are on by default)
[conventions]
disabled = []  # e.g. ["auto:vendored"] to disable specific conventions

# Pattern-based tag rules (applied at index time, user: namespace)
[[rules]]
pattern = "docs/deprecated/**"
tags = ["deprecated"]

[[rules]]
pattern = "docs/architecture/**"
tags = ["canonical", "architecture"]

[[rules]]
pattern = "internal/**"
tags = ["internal"]

[[rules]]
pattern = "*.md"
repo = "aegis"
tags = ["ops-docs"]

# Tag effects on scoring — ROLE-SCOPED
#
# Effects can target all roles (default) or specific roles.
# When role is specified, the effect only applies to queries from that role.
# This is the key composability feature: same tag, different meaning per role.

# Global effects (apply to all roles)
[effects.deprecated]
boost = -0.8          # Reduce score by 80% for everyone

[effects.noise]
exclude = true        # Never inject for anyone

[effects.pin]
pin = true            # Always inject regardless of threshold
budget_reserve = 50   # Reserve 50 lines of budget for pinned content

# Role-scoped effects — same tag, different behavior per role
#
# Use [[effects_scoped]] for role-specific overrides.
# Format: tag + role glob + effect.
# Scoped effects take precedence over global effects for matching roles.

[[effects_scoped]]
tag = "canonical"
role = "aegis/*"
boost = 0.5           # Aegis agents: boost canonical content 50%

[[effects_scoped]]
tag = "canonical"
role = "external/*"
boost = 0.2           # External roles: mild boost only

[[effects_scoped]]
tag = "internal"
role = "aegis/*"
boost = 0.0           # Aegis agents: no change (they can see internal)

[[effects_scoped]]
tag = "internal"
role = "external/*"
exclude = true        # External roles: hidden entirely

[[effects_scoped]]
tag = "test"
role = "aegis/crew/sentinel"
boost = 0.3           # Security crew: boost test files (review coverage)

[[effects_scoped]]
tag = "test"
role = "aegis/crew/*"
boost = -0.3          # Other crew: demote test files (usually noise)

# Frontmatter extraction
[frontmatter]
enabled = true
field = "tags"        # YAML field name to extract tags from
# Also checks: bobbin-tags, labels (fallback fields)

# Code comment extraction
[comments]
enabled = true
prefix = "bobbin:tag" # Pattern: // bobbin:tag <tag1> <tag2>
```

#### Role-Scoped Effect Resolution

When a query arrives with a role, effects resolve as:

```
1. Collect all tags on the chunk
2. For each tag, check for scoped effects matching the query role
3. If scoped effect exists → use it (most specific role glob wins)
4. If no scoped effect → fall back to global effect
5. If no global effect → tag has no scoring impact (filter-only)
```

Glob matching for roles uses the same pattern as existing role-based access:
`aegis/*` matches `aegis/crew/ellie`, `aegis/crew/arnold`, etc.
More specific globs win: `aegis/crew/sentinel` beats `aegis/crew/*` beats `aegis/*`.

#### No Separate Manual Tags Store

CLI-applied tags write directly to `tags.toml` as rules. There is no hidden
SQLite database — everything is in one place:

```bash
bobbin tag add src/auth.rs security
# Appends to .bobbin/tags.toml:
#   [[rules]]
#   pattern = "src/auth.rs"
#   tags = ["security"]

bobbin tag add "internal/**" internal
# Appends to .bobbin/tags.toml:
#   [[rules]]
#   pattern = "internal/**"
#   tags = ["internal"]
```

This means:
- **One source of truth** — `tags.toml` has all human/agent-authored tag rules
- **Git-trackable** — tag rules are version-controlled alongside code
- **Hand-editable** — users can review and modify tags in their editor
- **Same format** — whether it's a glob pattern or a specific file path

The only tags NOT in `tags.toml` are `feedback:` namespace tags. These are
computed from agent usage signals and stored directly in the LanceDB `tags`
column, since they're ephemeral, high-volume, and machine-generated. They're
recomputed on each index run from accumulated feedback data in the existing
`.bobbin/metrics.jsonl`.

## Tag Effects on Scoring

### Pipeline Position: Tags SUBSUME Doc Demotion

Tags do NOT apply after the existing doc demotion stage — they **replace** it.
The reasoning: if a doc is tagged `canonical` with boost +0.5, but doc demotion
already reduced its score by 70%, the boost would need to be +2.33 just to break
even. That's unintuitive and makes tags ineffective on docs.

Instead, the tag effects stage merges with and supersedes the existing category
demotion. When a chunk has tag-based scoring effects, those replace the default
category behavior. When no tag effects apply, the existing demotion is unchanged.

Excludes are pushed even earlier — into the search query itself as WHERE clauses,
so we never fetch chunks that would be dropped.

```
Hybrid Search (semantic + keyword)
  ↓
  │ EXCLUDES applied here as WHERE clauses:
  │   - Hard excludes (exclude=true tags) → never fetched
  │   - Role excludes (exclude for querying role) → never fetched
  │   This avoids wasting compute on chunks we'd drop anyway
  ↓
RRF combination
  ↓
Recency boost
  ↓
┌──────────────────────────────────────────────┐
│ CATEGORY + TAG SCORING (MERGED STAGE)        │
│                                              │
│ For each result chunk:                       │
│  1. Resolve tag effects for this chunk       │
│     (role-scoped → global → none)            │
│  2. IF tag effects exist:                    │
│     score *= product(tag boost factors)      │
│     (this REPLACES default category demotion)│
│  3. IF NO tag effects on this chunk:         │
│     apply default category demotion          │
│     (doc/config files: score *= 0.3)         │
│  4. IF pin effect:                           │
│     set aside to reserved budget pool        │
│     (skip threshold check entirely)          │
└──────────────────────────────────────────────┘
  ↓
Budget-constrained assembly
  (pinned chunks injected first from reserved budget,
   then ranked chunks fill remaining budget)
```

### Why This Ordering Matters

| Scenario | Old (tags after demotion) | New (tags replace demotion) |
|----------|--------------------------|----------------------------|
| Doc tagged `canonical` (boost +0.5) | score * 0.3 * 1.5 = **0.45x** | score * 1.5 = **1.5x** |
| Doc tagged `deprecated` (boost -0.8) | score * 0.3 * 0.2 = **0.06x** | score * 0.2 = **0.2x** |
| Doc with no tags | score * 0.3 = **0.3x** | score * 0.3 = **0.3x** (unchanged) |
| Source tagged `pin` | demoted then pinned (wrong rank) | pinned directly (correct) |
| Source tagged `noise` (exclude) | fetched then dropped (wasted) | never fetched (efficient) |

### Boost Math

Boosts are multiplicative and stack. They replace (not augment) category demotion:

```
effective_factor = product(1 + effect.boost for each resolved tag effect)

Example: doc has tags [user:canonical, user:architecture]
  canonical boost = 0.5   → factor = 1.5
  architecture boost = 0.2 → factor = 1.2
  effective = score * 1.5 * 1.2 = score * 1.8

  (Without tags, this doc would get: score * 0.3 from category demotion)
  (With tags, category demotion is skipped — tags are the scoring authority)
```

Negative boosts demote:

```
deprecated boost = -0.8 → factor = 0.2 (80% reduction, similar to demotion)
```

Effective factor is clamped to `[0.01, 10.0]` to prevent zeroing or extreme
inflation.

### Pin Behavior

Pinned chunks bypass the relevance threshold and are injected first:

1. Collect all pinned chunks matching the current query context
2. Reserve `budget_reserve` lines from total budget
3. Inject pinned chunks within reserved budget (ranked by relevance among themselves)
4. Remaining budget goes to normal relevance-ranked results

Pins skip category demotion entirely — a pinned doc gets its raw relevance score
for ranking among other pins.

### Exclude Behavior (Pre-Search Filtering)

Excludes are applied as SQL WHERE clauses during the search query, not post-search:

- `exclude = true` — adds `tags NOT LIKE '%user:noise%'` to query
- Role-scoped exclude — resolved at query time based on the requesting role
- This means excluded chunks are never fetched, never scored, never waste compute

## CLI Interface

### Managing Tags

```bash
# Add tags to files/patterns
bobbin tag add "docs/deprecated/**" deprecated
bobbin tag add src/auth.rs security critical
bobbin tag add --repo aegis "internal/**" internal

# Remove tags
bobbin tag remove "docs/deprecated/**" deprecated
bobbin tag remove src/auth.rs security

# List tags
bobbin tag list                          # All tags in use
bobbin tag list --file src/auth.rs       # Tags on a specific file
bobbin tag list --tag deprecated         # Files with a specific tag
bobbin tag list --stats                  # Tag usage counts

# Show tag effects
bobbin tag effects                       # Current effect rules
```

### Searching with Tags

```bash
# Filter by tag
bobbin search "auth logic" --tag security
bobbin search "deployment" --tag canonical --tag architecture

# Exclude by tag
bobbin search "config" --exclude-tag deprecated --exclude-tag test

# Combined with existing filters
bobbin search "auth" --group infra --tag security --type function
```

### HTTP API

```
GET /search?q=auth+logic&tag=security&exclude_tag=deprecated
GET /context?q=deployment&tag=canonical
GET /tags                              # List all tags
GET /tags?file=src/auth.rs             # Tags for file
GET /tags?tag=deprecated               # Files with tag
```

## Frontmatter Extraction

Markdown files can declare tags in YAML frontmatter:

```markdown
---
title: Authentication Architecture
tags: [canonical, architecture, security]
---

# Authentication Architecture
...
```

All chunks from this file inherit the declared tags. Bobbin extracts tags during
indexing from the configured frontmatter field (default: `tags`).

## Code Comment Extraction

Source files can tag individual chunks via special comments:

```go
// bobbin:tag security critical
func Authenticate(ctx context.Context, token string) (*User, error) {
    // ...
}
```

```python
# bobbin:tag deprecated
def old_auth_handler(request):
    pass
```

```rust
// bobbin:tag internal
pub(crate) fn validate_internal_token(token: &str) -> bool {
    // ...
}
```

The comment must appear on the line immediately before the chunk's first line.
Tags from comments apply only to that specific chunk, not the whole file.

## Agent Feedback Loop

This extends the remaining work from aegis-yqmfs5. Tags provide the storage
layer for feedback signals.

### Feedback Collection (PostToolUse hook)

When the PostToolUse hook fires after Write/Edit/Read:
1. Check which files the agent touched
2. Cross-reference with files that were injected in the last UserPromptSubmit
3. If an injected file was used → record positive signal
4. If an injected file was NOT used in 3+ consecutive sessions → record negative signal

### Auto-Tags from Feedback

```
bobbin_feedback_positive >= 5 sessions → auto-tag "hot"
bobbin_feedback_negative >= 10 sessions → auto-tag "cold"
```

Auto-tags are stored in manual-tags.db with `added_by = "bobbin:feedback"` and
are periodically recomputed. They participate in scoring like any other tag:

```toml
[effects.hot]
boost = 0.3    # 30% boost for frequently useful chunks

[effects.cold]
boost = -0.5   # 50% demotion for frequently ignored chunks
```

### Feedback Metrics

Push to Pushgateway for observability:

```
bobbin_feedback_positive_total{repo="aegis"} 142
bobbin_feedback_negative_total{repo="aegis"} 89
bobbin_feedback_auto_tagged{tag="hot"} 23
bobbin_feedback_auto_tagged{tag="cold"} 15
```

## Indexing Pipeline Changes

Tags are resolved at index time for pattern-based and content-based sources:

```
File Walker
    ↓
Structural Parser (existing)
    ↓
┌──────────────────────────────────┐
│ Tag Resolution (NEW)             │
│                                  │
│ For each chunk:                  │
│  1. Match file against rules     │
│     (pattern → tags)             │
│  2. Extract frontmatter tags     │
│     (markdown files)             │
│  3. Extract comment tags         │
│     (code files)                 │
│  4. Query manual-tags.db         │
│  5. Union all tag sources        │
│  6. Sort and join as CSV string  │
└──────────────────────────────────┘
    ↓
Embedder (existing)
    ↓
LanceDB Storage (now includes tags column)
```

## Migration

### Schema Migration

1. Add `tags` column to LanceDB chunks table (default: empty string)
2. Create `.bobbin/tags.toml` with empty defaults and built-in conventions enabled

### Incremental Adoption

- Existing indexes continue to work (empty tags = no effect)
- Built-in convention tags (`auto:test`, `auto:docs`, etc.) populate on next
  `bobbin index` run — immediate value with zero config
- User tags can be added immediately via CLI or by editing `tags.toml`
- No breaking changes to existing config or CLI

## Performance Analysis

### Index-Time Cost

Tag resolution adds a step between parsing and embedding:

1. **Convention matching**: O(conventions × files) — 7 built-in patterns × 27k files.
   Glob matching short-circuits on first path component, so most files are rejected
   in nanoseconds. Expected overhead: <100ms total for full re-index.

2. **Config rule matching**: O(rules × files) — typically <50 rules. Same glob
   short-circuit behavior. Expected: <50ms.

3. **Frontmatter extraction**: Already parsed by the markdown chunker (pulldown-cmark).
   Tag extraction is a YAML field lookup on existing parsed data. Zero additional I/O.

4. **Tags column write**: One additional string column per chunk in LanceDB. At 141k
   chunks, this adds ~1-2MB to the index (short strings, well-compressed).

**Bottom line**: Tag resolution is CPU-trivial compared to embedding (which dominates
indexing at ~1-2 seconds per batch of 32 chunks). Total overhead: <1% of index time.

### Query-Time Cost

1. **Exclude filtering**: Applied as SQL WHERE clauses in the LanceDB query. These
   are evaluated during the scan, not post-scan — LanceDB handles this natively.
   May slightly reduce result set (fewer rows to score), making search marginally faster.

2. **Tag effect resolution**: For each result chunk (default top 20), resolve effects
   from a small in-memory map (typically <50 entries). O(tags_per_chunk × effects).
   Negligible — microseconds.

3. **Pin collection**: One additional query at context assembly time to find pinned
   chunks. Indexed by the `tags` column. Fast for small pin sets (<10 pins typical).

**Bottom line**: Query-time overhead is dominated by the vector ANN search and FTS
query, both unchanged. Tag processing adds microseconds to a pipeline that takes
50-200ms. No measurable latency impact.

### Storage Cost

- LanceDB `tags` column: ~1-2MB for 141k chunks (compressed strings)
- `tags.toml`: <10KB (human-authored config)
- Feedback data: Reuses existing `metrics.jsonl` — no new storage

## Measuring Impact

### Functional Metrics (Is it working?)

Push to Pushgateway for dashboarding:

```
# Tag coverage — what % of chunks have tags?
bobbin_tagged_chunks_total{namespace="auto"} 42000
bobbin_tagged_chunks_total{namespace="user"} 1500
bobbin_tagged_chunks_total{namespace="feedback"} 800
bobbin_untagged_chunks_total 97390

# Tag effect activity — are tags actually changing results?
bobbin_tag_boosts_applied_total{tag="canonical"} 230
bobbin_tag_excludes_applied_total{tag="noise"} 89
bobbin_tag_pins_injected_total 45

# Injection precision (the key metric)
bobbin_injection_used_total 142          # agent used an injected file
bobbin_injection_unused_total 89         # agent ignored injected files
# Precision = used / (used + unused)

# Cross-repo noise (the problem that started this)
bobbin_injection_cross_repo_total{from="pixelsrc",to="aegis"} 0  # should be 0
```

### Non-Functional Metrics (Is it fast enough?)

```
# Index time delta
bobbin_index_duration_seconds{phase="tag_resolution"} 0.08
bobbin_index_duration_seconds{phase="total"} 45.2

# Query latency delta (histogram)
bobbin_search_duration_seconds_bucket{le="0.1"} 950
bobbin_search_duration_seconds_bucket{le="0.5"} 998
bobbin_search_duration_seconds_bucket{le="1.0"} 1000

# Tag resolution overhead per query
bobbin_tag_resolution_duration_seconds 0.0001  # should be <1ms
```

### Before/After Comparison

To validate the system works, measure these before deploying tags and after:

1. **Cross-repo leak rate**: Count injections where `result.repo != agent.rig`.
   Target: reduce from current ~15% to <2%.
2. **Injection precision**: % of injected chunks that the agent actually references
   in subsequent tool calls. Target: improve from ~40% to >60%.
3. **Budget utilization**: Are we spending budget on useful content? Track
   `useful_lines / budget_lines`. Target: >50%.

## Interaction with Existing Features

| Feature | Interaction |
|---------|-------------|
| **Index groups** | Composable — `--group infra --tag security` |
| **Role access** | Role-scoped effects compose with role `allow/deny` — access controls who can see a repo, tag effects control scoring within visible results |
| **Recency boost** | Tags apply after recency (independent adjustments) |
| **Doc demotion** | Tags apply after demotion (can override: tag canonical on a doc) |
| **Coupling expansion** | Coupled files inherit NO tags from seed (tags are per-chunk) |
| **Provenance bridging** | Bridged chunks inherit NO tags from source |
| **Gate threshold** | Tags don't bypass the gate (except `pin` which does) |
| **Dedup** | Tag changes don't trigger re-injection (only content changes do) |

## Resolved Questions

1. **Tag namespaces?** → YES. Three namespaces: `auto:` (conventions), `user:`
   (manual/config), `feedback:` (machine-generated). Prevents collision, provides
   provenance. Short form expands automatically.

2. **Role-scoped effects?** → YES. `[[effects_scoped]]` allows the same tag to
   mean different things per role. Most specific role glob wins, falls back to
   global `[effects.*]`.

3. **Automated common tags?** → YES. Built-in conventions auto-tag `tests/`,
   `docs/`, `internal/`, etc. with zero config. Ships with bobbin.

4. **Separate CLI tag storage?** → NO. CLI writes directly to `tags.toml`. One
   source of truth, git-trackable, hand-editable. No hidden SQLite.

5. **Pipeline position for boosts/pins?** → Tags SUBSUME doc demotion stage.
   Tagged chunks get tag effects instead of (not on top of) category demotion.
   Excludes pushed to WHERE clauses pre-search.

## Open Questions

1. **Should coupled/bridged chunks inherit tags from their seed?** Current design
   says no — keeps tags explicit. But `pin` on a seed could usefully propagate
   to its coupled files.

2. **Tag-aware embeddings?** Could append tags to the text before embedding so
   that tagged chunks cluster differently in vector space. Likely overkill for v1.

3. **Remote server tag sync?** When using `--server`, should the remote server
   have its own tags.toml, or should client-side tags override? Current design:
   tags live with the index, so the server owns them. Client-side tag rules would
   require a tag-override mechanism in the HTTP API.

4. **Feedback signal granularity?** Current design tracks file-level usage (did
   the agent use the file?). Chunk-level tracking (did the agent use *this specific
   function*?) would be more precise but harder to implement — requires correlating
   tool call line ranges with injected chunk ranges.

## Implementation Plan

### Phase 1: Storage, Conventions & Config (P1)
- Add `tags` column to LanceDB schema
- Implement built-in convention tags (auto-applied from directory patterns)
- Implement `tags.toml` parsing (rules + global effects)
- Pattern-based + convention-based tag resolution at index time
- `bobbin tag list` and `bobbin tag add/remove` CLI (writes to `tags.toml`)
- Baseline metrics: `bobbin_tagged_chunks_total`, `bobbin_untagged_chunks_total`

### Phase 2: Search Integration & Role-Scoped Effects (P1)
- `--tag` and `--exclude-tag` CLI flags
- Global tag effects on scoring (boost, exclude) — subsumes doc demotion stage
- Role-scoped effects (`[[effects_scoped]]`) with glob matching
- Exclude filtering as pre-search WHERE clauses
- HTTP API: `?tag=`, `?exclude_tag=` parameters, `GET /tags` endpoint
- Metrics: `bobbin_tag_boosts_applied_total`, `bobbin_tag_excludes_applied_total`

### Phase 3: Content Extraction (P2)
- Frontmatter tag extraction from markdown YAML
- Code comment tag extraction (`// bobbin:tag <tags>`)
- Before/after measurement: cross-repo leak rate, injection precision

### Phase 4: Feedback Loop (P2)
- PostToolUse feedback collection (which injected files were used?)
- `feedback:hot` / `feedback:cold` auto-tagging from accumulated signals
- Pushgateway metrics: `bobbin_injection_used_total`, `bobbin_injection_unused_total`

### Phase 5: Pin Support (P3)
- `pin` effect with budget reservation
- Pinned chunk injection in hook pipeline (reserved budget, threshold bypass)
- Metrics: `bobbin_tag_pins_injected_total`

## Handoff

This design document goes to **stryder** (bobbin's ranger) for implementation.
File implementation beads with dependency chain matching the phases above.
