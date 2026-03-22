# Context Bundles: Stable, Addressable Knowledge Anchors for Bobbin

**Author**: stryder (aegis/crew)
**Date**: 2026-03-22
**Status**: Draft

## Problem

Search is ephemeral. An agent searches "how does context assembly work", gets good
results, but that knowledge graph evaporates. The next agent asks the same question —
cold start again. Documentation that references implementation details goes stale
because it points at file paths, not concepts.

Additionally, agents working in an unfamiliar codebase have no way to get a project
map — they either get a wall of CLAUDE.md or run a dozen exploratory searches hoping
to land on the right files. There's no structured way to say "here's the landscape,
now pick what you need."

## Solution: Context Bundles

A **bundle** is a named, hierarchical, keyword-bound grouping of files and chunks that
represents a concept, feature, or subsystem. Bundles are:

- **Addressable**: Stable `bundle:<name>` URIs that work in docs, beads, and CLAUDE.md
- **Hierarchical**: Nested via `/` naming (e.g., `context/pipeline`, `context/tag-effects`)
- **Keyword-bound**: Guaranteed to surface when specific terms are searched
- **Progressive**: Three disclosure levels (map → outline → deep)
- **Live**: Always resolve to current file state, never go stale

```
bobbin
├── context       → "Assembles relevant code for agent prompts"
│   ├── pipeline  → "5-phase assembly: seed → coupling → bridge → filter → budget"
│   ├── tags      → "Tag-based scoring, pinning, and access control"
│   └── budget    → "Line quota tracking and allocation"
├── search        → "Hybrid semantic + keyword search"
│   ├── lance     → "LanceDB vector store backend"
│   └── rrf       → "Reciprocal Rank Fusion scoring"
├── hook          → "CLI injection into agent prompts"
└── tags          → "Glob-based classification + scoring effects"
```

## Design

### 1. Bundle Definition

Bundles are defined in `tags.toml` alongside existing tag rules, sharing the same
config file and load path. This avoids a new config format and leverages the existing
tag resolution infrastructure.

```toml
# tags.toml — bundle definitions

[[bundles]]
name = "context"
description = "Assembles relevant code for agent prompts"
keywords = ["context assembly", "context injection", "budget", "progressive disclosure"]
tags = ["domain:context-assembly"]       # membership via existing tags
files = ["src/search/context.rs"]        # explicit file membership (optional)
# children are implicit: any bundle named "context/*" is a child

[[bundles]]
name = "context/pipeline"
description = "5-phase assembly: seed → coupling → bridge → filter → budget"
tags = ["domain:context-assembly"]
files = ["src/search/context.rs"]
docs = ["docs/design/context-pipeline.md"]

[[bundles]]
name = "context/tags"
description = "Tag-based scoring, pinning, and access control"
files = ["src/tags.rs"]
docs = ["docs/guides/tags-playbook.md"]

[[bundles]]
name = "hook"
description = "CLI injection into agent prompts"
files = ["src/cli/hook.rs"]
includes = ["context", "tags"]  # pulls in these bundles at L2 deep dive
keywords = ["hook injection", "PostToolUse", "UserPromptSubmit"]

# Cross-repo bundle: spans bobbin + aegis + gastown
[[bundles]]
name = "reactor-alerts"
description = "Alert pipeline: Prometheus → reactor → IRC bridge"
repos = ["aegis", "bobbin"]
files = [
    "aegis:deploy/reactor/reactor.py",
    "aegis:deploy/aegis-irc/main.go",
    "aegis:deploy/alertmanager/config.yml",
    "bobbin:src/cli/hook.rs",              # injection path that delivers context
]
keywords = ["reactor", "alerts", "prometheus", "IRC bridge"]
tags = ["domain:monitoring", "domain:comms"]
```

#### Bundle fields

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `name` | string | yes | Unique bundle identifier. `/` creates hierarchy. |
| `description` | string | yes | One-line summary (shown at L0). |
| `keywords` | string[] | no | Search terms that trigger this bundle. |
| `tags` | string[] | no | Tag-based membership (chunks with these tags belong). |
| `files` | string[] | no | File paths. Bare paths are repo-relative; `repo:path` for cross-repo. |
| `docs` | string[] | no | Documentation files. Same `repo:path` syntax for cross-repo. |
| `includes` | string[] | no | Other bundles pulled in at L2 deep dive. |
| `repos` | string[] | no | Repos this bundle spans. Omit = all repos (tag/keyword match). |

Membership is the **union** of tag-matched chunks + explicit files + docs. This allows
bundles to be as precise (explicit files only) or as broad (tag-based) as needed.

**Hierarchy is implicit from naming**: any bundle named `X/Y` is a child of `X`.
At L0, `bobbin bundle list` builds the tree by splitting on `/`. No explicit
`children` field needed — the naming convention IS the relationship. Renaming or
adding a sub-bundle automatically updates the tree.

**Cross-repo bundles** are first-class. Many real subsystems span repositories —
the alert pipeline touches aegis (reactor, IRC bridge), bobbin (context injection),
and goldblum (ansible deployment). A bundle captures this cross-cutting concern
as a single addressable unit.

File references use `repo:path` syntax:

```toml
files = [
    "src/tags.rs",                       # bare path → current repo (or any repo if no repo scope)
    "aegis:deploy/reactor/reactor.py",   # explicit repo prefix
    "gastown:internal/nudge/nudge.go",   # another repo
]
```

The `repos` field declares which repos the bundle spans. This serves two purposes:
1. **Search scoping**: When querying with `bundle=X`, search is limited to these repos
2. **Validation**: `bobbin bundle check` can verify all referenced files exist

When `repos` is omitted, the bundle matches across all indexed repos via tags/keywords.
When present, it constrains both file resolution and search queries.

At query time, cross-repo bundles work identically to single-repo bundles — the
`extra_filter` SQL clause just includes multiple repo conditions:

```sql
WHERE (repo = 'aegis' AND file_path IN (...)) OR (repo = 'bobbin' AND file_path IN (...))
```

This maps directly onto bobbin's existing multi-repo index — LanceDB already stores
repo as a column on every chunk. No new infrastructure needed.

### 2. Progressive Disclosure Levels

Three levels map to the existing `ContentMode` enum, applied at the bundle level:

| Level | ContentMode | What you get | Budget cost | API param |
|-------|-------------|-------------|-------------|-----------|
| **L0: Map** | `None` | Bundle names + descriptions + children | ~1 line/bundle | `level=0` |
| **L1: Outline** | `Preview` | Key files, entry point names, first 3 lines | ~10 lines/file | `level=1` |
| **L2: Deep** | `Full` | Full chunks, coupled files, bridged docs, includes | Full budget | `level=2` |

**L0** is a simple config read — no search, no embedding, near-zero cost. An agent
can map an entire project for the cost of reading a TOML file.

**L1** runs `list_symbols` on member files and returns file paths with symbol names
and preview content. Costs ~100 lines for a typical bundle.

**L2** runs the full `ContextAssembler` pipeline but scoped to bundle membership files.
The `extra_filter` already supports SQL WHERE clauses — bundle membership becomes a
filter predicate on file paths and tags.

### 3. Keyword Binding (Perma-Search Links)

Bundles register keywords that guarantee they appear in search results:

```toml
[[bundles]]
name = "context"
keywords = ["context assembly", "injection", "budget"]
```

**Integration with existing `keyword_repos`**: The current `KeywordRepoRule` maps
keywords → repo names for search scoping. Bundle keywords extend this: when a query
matches a bundle's keywords, the bundle is returned as a **pinned group** alongside
organic search results.

Implementation: extend `HooksConfig::resolve_keyword_repos()` to also return matched
bundle names. The hook injection path (`hook.rs`) already calls this function — it
gains bundle awareness for free.

**Keyword matching behavior**:
- Case-insensitive substring match (same as existing `keyword_repos`)
- Bundle appears as a collapsed group at the top of results
- Organic results still appear below (bundle doesn't replace them)
- If multiple bundles match, all are shown (ordered by keyword specificity)

### 4. Search & Filter Integration

#### 4a. Bundle as search filter

New `bundle` parameter on `/search` and `/context` endpoints:

```
GET /search?q=assembly&bundle=context          # search within bundle files only
GET /context?q=pipeline&bundle=context&level=2  # deep context scoped to bundle
GET /context?bundle=context&level=0             # map only (no query needed)
```

Implementation: bundle membership resolves to a set of file paths and tags. This
becomes an `extra_filter` SQL clause:

```sql
-- Bundle "context" with tags=["domain:context-assembly"] and files=["src/search/context.rs"]
WHERE (tags LIKE '%domain:context-assembly%' OR file_path IN ('src/search/context.rs'))
```

This reuses the existing `extra_filter` mechanism in `ContextConfig` — no new query
path needed.

#### 4b. Bundle as search result

When a search or context query matches a bundle's keywords, the response includes a
`bundles` field:

```json
{
  "query": "how does context injection work",
  "bundles": [
    {
      "name": "context",
      "description": "Assembles relevant code for agent prompts",
      "match": "keyword:context injection",
      "children": ["context/pipeline", "context/tags", "context/budget"],
      "file_count": 5,
      "doc_count": 3,
      "drill": "bobbin bundle show context"
    }
  ],
  "files": [ /* ... existing organic results ... */ ],
  "summary": { /* ... */ }
}
```

This is additive — existing response structure unchanged, `bundles` is a new optional
field.

#### 4c. Bundle listing and exploration

New CLI commands and MCP tools:

```bash
bobbin bundle list                     # List all bundles (L0 map)
bobbin bundle list --repo aegis        # Bundles for a specific repo
bobbin bundle show context             # Show bundle outline (L1)
bobbin bundle show context --deep      # Full context (L2)
bobbin bundle show context/pipeline    # Sub-bundle drill-down

# Agent-created bundles
bobbin bundle create "reactor-alerts" \
  --description "Alert pipeline: Prometheus → reactor → IRC" \
  --files "deploy/reactor/reactor.py,deploy/aegis-irc/main.go" \
  --tags "domain:monitoring,domain:comms" \
  --keywords "reactor,alerts,prometheus"

bobbin bundle add "reactor-alerts" --file "deploy/reactor/config.yaml"
bobbin bundle remove "reactor-alerts" --file "old-file.py"
```

MCP tool wrappers:

| Tool | Maps to | Description |
|------|---------|-------------|
| `bobbin_bundles` | `bundle list` | List all bundles (L0) |
| `bobbin_bundle` | `bundle show <name>` | Show bundle (L1 default, L2 with `--deep`) |
| `bobbin_context?bundle=X` | existing `/context` + filter | Deep context scoped to bundle |
| `bobbin_search?bundle=X` | existing `/search` + filter | Search within bundle |

### 5. Addressable References (`bundle:` URI scheme)

Bundles get stable IDs that work in documentation, beads, and CLAUDE.md:

```markdown
<!-- In a design doc -->
See [bundle:context/pipeline] for the full assembly flow.

<!-- In CLAUDE.md -->
Before modifying tag effects, review [bundle:tags/effects].

<!-- In a bead description -->
Related: bundle:reactor-alerts
```

These are **live links** — they resolve to the current state of the bundle, not a
snapshot. When `context.rs` gets refactored into multiple files, updating the bundle
definition once keeps all references valid.

**Resolution**: Any tool that renders markdown can resolve `bundle:` URIs by calling
`bobbin bundle show <name>`. The MCP integration makes this available to any agent.

### 6. Context Injection Integration

The hook injection path (`hook.rs`) currently:

1. Reads stdin prompt
2. Runs hybrid search
3. Calls `ContextAssembler.assemble()`
4. Formats and outputs context

Bundle integration adds a **pre-search** step:

```
1. Read stdin prompt
2. [NEW] Check prompt against bundle keywords → matched_bundles
3. Run hybrid search (existing)
4. [NEW] If matched_bundles, reserve budget for bundle core files
5. Call ContextAssembler.assemble() with bundle-aware budget allocation
6. [NEW] Annotate output with bundle metadata (name, drill command)
7. Format and output context
```

**Budget allocation with bundles**:

Without bundles: 300 lines spread across whatever search finds.

With bundle match: Reserve a configurable percentage (default 40%) for bundle core
files, remaining 60% for organic discovery. This prevents important files from being
budget-starved by noisy organic results.

```toml
# config.toml
[hooks]
bundle_budget_reserve = 0.4  # 40% of budget reserved for matched bundles
```

**Output format change** (additive):

```
Bobbin found 8 relevant files (5 direct, 2 coupled, 1 bridged, 8/300 budget lines):

📦 bundle:context — "Assembles relevant code for agent prompts" (5 files)
   → `bobbin bundle show context` for full context

=== Source Files ===
--- src/search/context.rs:675-861 run_hybrid_search ---
...
```

The bundle annotation is a 2-line addition to the injection header. It tells the agent:
"there's a curated bundle for this topic if you want structured context instead of
ad-hoc search results."

### 7. Storage

**Config-defined bundles** (curated): Stored in `tags.toml` under `[[bundles]]`.
Loaded at startup alongside tag rules. No new storage backend.

**Agent-created bundles** (dynamic): Stored in a `bundles` table in `metadata.db`
(the existing SQLite metadata store). Schema:

```sql
CREATE TABLE bundles (
    name TEXT PRIMARY KEY,
    description TEXT NOT NULL,
    keywords TEXT,           -- JSON array
    tags TEXT,               -- JSON array
    files TEXT,              -- JSON array
    docs TEXT,               -- JSON array
    children TEXT,           -- JSON array
    includes TEXT,           -- JSON array
    repo TEXT,
    created_by TEXT,         -- agent identifier
    created_at TEXT,         -- ISO 8601
    updated_at TEXT          -- ISO 8601
);
```

Agent-created bundles are merged with config-defined bundles at runtime. Config-defined
bundles take precedence (config is authoritative, agent bundles are supplementary).

### 8. Bundle Freshness & Staleness Detection

Bundles track member files. If a member changes significantly since the bundle was
last reviewed, flag it:

```
⚠ bundle:context — stale: context.rs modified 3 days ago (new symbols added)
   Run `bobbin bundle refresh context` to update
```

Implementation: compare file modification timestamps (already available from
`MetadataStore`) against the bundle's `updated_at`. A configurable staleness threshold
(default: 7 days) triggers the warning.

`bobbin bundle refresh <name>` re-scans member files, updates the description
(optionally via LLM summarization), and resets the timestamp.

### 9. Auto-Discovery from Coupling Data

Bobbin already computes temporal coupling (files that co-change). Clusters of strongly
coupled files are natural bundle candidates:

```bash
bobbin bundle suggest                    # Analyze coupling graph for clusters
bobbin bundle suggest --repo aegis       # Scope to one repo
bobbin bundle suggest --threshold 0.5    # Minimum coupling strength
```

Output:

```
Suggested bundles (3 clusters found):

1. "reactor-alerts" (coupling: 0.85, 4 files)
   deploy/reactor/reactor.py, deploy/aegis-irc/main.go,
   deploy/alertmanager/config.yml, services/prometheus/rules/aegis.rules
   Suggested keywords: reactor, alerts, prometheus

2. "den-family" (coupling: 0.78, 3 files)
   deploy/den-tg/bot.py, deploy/den-svc/calendar.py, deploy/den-svc/config.yaml
   Suggested keywords: den, telegram, family, calendar

→ Accept: bobbin bundle create "reactor-alerts" --from-suggestion 1
```

This leverages the existing `GitAnalyzer` coupling computation — the new code is
cluster detection on the coupling graph (simple connected-components or threshold-based
grouping).

### 10. Bundle Inheritance & Composition

Bundles can include other bundles via the `includes` field:

```toml
[[bundles]]
name = "hook"
includes = ["context", "tags"]  # pulls these in at L2
files = ["src/cli/hook.rs"]
```

At **L0/L1**: `hook` is its own entity. At **L2**: the deep dive also includes files
from `context` and `tags` bundles, because you can't fully understand hook injection
without understanding what it's injecting.

Budget allocation for includes: included bundle files share the 60% organic budget
(not the 40% reserve). They're treated as supplementary context, not core.

### 11. Bundle-Scoped Feedback

The feedback system already tracks `injection_id`. When an injection includes bundle
files, store the bundle name:

```sql
ALTER TABLE injections ADD COLUMN bundle TEXT;  -- NULL if no bundle matched
```

Over time, aggregate feedback by bundle:

```bash
bobbin bundle stats context
# bundle:context — 42 injections, 38 useful (90%), 2 noise, 2 unrated
# Most requested sub-bundles: pipeline (18), tags (12), budget (8)
# Suggested additions: config.rs (appeared in 8 organic results alongside bundle)
```

The "suggested additions" come from files that frequently appear in organic results
when the bundle is also matched — evidence that they belong in the bundle.

### 12. Perma-Links: Short Slugs for Bundles

Every bundle gets a short, stable, human-readable slug that serves as its permanent
identifier. These slugs are the bundle's public API — referenceable anywhere without
requiring changes to external systems.

```
bobbin://b/context-pipeline     # full URI form
b:context-pipeline              # short form (for docs, chat, bead descriptions)
```

#### Slug format

- Lowercase alphanumeric + hyphens: `[a-z0-9-]+`
- Max 40 characters
- Hierarchy via prefix convention: `context-pipeline`, `context-tags`, `context-budget`
- Auto-generated from bundle name (`context/pipeline` → `context-pipeline`)
- Can be overridden: `slug = "ctx-pipe"` in bundle config

#### Executable perma-links

Every slug is directly executable — an agent or human can go from reference to
full progressive-disclosure context in one command:

```bash
bobbin bundle show b:context-pipeline          # L1 outline (default)
bobbin bundle show b:context-pipeline --deep   # L2 full context
bobbin bundle show b:context-pipeline --map    # L0 children only
```

The `b:` prefix is optional in bobbin commands but required in prose to avoid
ambiguity:

```bash
bobbin bundle show context-pipeline            # works (implicit b:)
```

### 13. Beads Integration (Zero Beads Changes)

**Principle**: Bobbin owns bundles. Beads stays untouched. Integration uses existing
bead primitives — labels and description text — not schema changes.

#### 13a. Bundle slugs in bead labels

Use bead labels to associate work with bundles:

```bash
bd create "Fix bridge budget overflow" -t bug -p 1 -l b:context-pipeline
bd create "Tag effects not applying" -t bug -p 2 -l b:context-tags
```

Labels are free-form strings in beads — no schema change needed. The `b:` prefix
is a convention that bobbin (and agents) recognize. An agent seeing `b:context-pipeline`
in a bead's labels knows to run:

```bash
bobbin bundle show context-pipeline
```

#### 13b. Bundle slugs in bead descriptions

Reference bundles in bead description prose:

```bash
bd create "Add rate limiting to context API" -t task -p 2 \
  --description "The /context endpoint (b:context-pipeline) needs rate limiting.
See b:hook for how the CLI calls this."
```

Any agent reading this bead can copy-paste the slug into a bobbin command. The
description is self-documenting — `b:context-pipeline` tells you both *what* to
look at and *how* to look at it.

#### 13c. Bobbin-side bead tracking

Bobbin tracks the relationship from its side. When a bundle is used during work
on a bead (detected via agent context — the injection sees bead IDs in the prompt),
bobbin records the association:

```sql
-- In bobbin's metadata.db (NOT in beads)
CREATE TABLE bundle_usage (
    id INTEGER PRIMARY KEY,
    bundle_slug TEXT NOT NULL,
    bead_id TEXT,                -- e.g., "aegis-h8x" (extracted from prompt context)
    agent TEXT,
    session_id TEXT,
    used_at TEXT,               -- ISO 8601
    injection_id TEXT           -- links to injection record
);
```

Over time, this builds a knowledge graph:

```bash
bobbin bundle stats context-pipeline
# b:context-pipeline — 42 injections across 8 beads
#   Beads: aegis-h8x (closed), aegis-j2k (closed), aegis-m9p (open), ...
#   Agents: stryder (18), polecats (24)
#   Feedback: 38 useful (90%), 2 noise
#   Suggested additions: config.rs (appeared in 8 organic results alongside bundle)
```

#### 13d. Planning molecules create bundles

When `mol-idea-to-plan` researches an idea and identifies key files, it creates
a bundle and references the slug in the beads it files:

```bash
# Molecule creates bundle during planning
bobbin bundle create "den-tg-integration" \
  --description "Family Telegram bot: calendar sync, notifications, ride tracking" \
  --files "deploy/den-tg/bot.py,deploy/den-svc/calendar.py" \
  --keywords "den,telegram,family,calendar"

# Molecule files beads with bundle label
bd create "Calendar sync for den-tg" -t task -p 2 -l b:den-tg-integration
bd create "Notification routing" -t task -p 2 -l b:den-tg-integration
```

Every polecat that picks up a subtask sees the label, runs the bundle command,
gets curated context. Zero beads changes. The bundle IS the shared context.

#### 13e. Bobbin search finds beads by bundle

When bobbin's `search_beads` MCP tool runs, it can cross-reference bundle slugs
in labels and descriptions:

```bash
bobbin search-beads "context assembly"
# Finds beads matching text AND beads labeled b:context-pipeline
# (because the bundle's keywords include "context assembly")
```

This enriches bead search with bundle keyword vocabulary — a bead labeled
`b:context-pipeline` is discoverable by any of that bundle's keywords, even if
the bead title doesn't mention those terms.

## Integration Points Summary

| Existing System | How Bundles Plug In |
|----------------|---------------------|
| **Tags** (`tags.toml`) | Bundle definitions live alongside tag rules. Tag-based membership. |
| **keyword_repos** | Extended to also match bundle keywords. Same resolution path. |
| **ContextAssembler** | Bundle membership → `extra_filter`. Budget reservation for matched bundles. |
| **ContentMode** | L0=None, L1=Preview, L2=Full — already exists, applied at bundle level. |
| **Hook injection** | Pre-search keyword check → bundle annotation in output header. |
| **`/context` API** | New `bundle` + `level` params. Response gains `bundles` field. |
| **`/search` API** | New `bundle` param for scoped search. |
| **Feedback** | `bundle` column on injections table. Bundle-level stats. |
| **Coupling** | Auto-discovery: cluster coupled files → suggested bundles. |
| **MCP tools** | New `bobbin_bundles`, `bobbin_bundle` tools. Existing tools gain `bundle` param. |
| **GroupConfig** | Bundles are conceptual groups (by topic); groups are repo-level groups. Complementary. |
| **Beads** | `b:` slug labels + description references. Zero schema changes. |
| **Molecules** | Planning molecules create bundles during research, label beads with slugs. |
| **Formulas** | Bundle-aware planning formula: research → bundle → beads with labels. |

## Eval Plan: F1 Impact of Targeted Bundles

### Objective

Measure whether bundle-scoped context improves precision and recall (F1) compared to
unscoped search. The key test: **create a bundle from a solved issue, then show it
improves retrieval for a later related issue** — proving bundles accumulate reusable
knowledge.

### Core Eval Strategy: Before/After with Real Issues

We use actual historical issues from our codebase where we know the ground-truth files.
The sequence:

1. **Pick a solved issue** (e.g., `aegis-3lw8zu`: globstar pattern bug in tags.rs)
2. **Create a bundle** from the files that were actually needed to fix it
3. **Pick a later related issue** (e.g., `aegis-243a2h`: another tags.rs pattern fix)
4. **Run retrieval three ways**: baseline (no bundle), bundle-scoped, bundle-injected
5. **Compare F1**: Did the bundle from issue 1 help find the right files for issue 2?

This tests the real value proposition: **knowledge from past work accelerates future
work on the same subsystem**.

### Eval Task Format

```yaml
id: bundle-eval-001
description: |
  Tags pattern matching: test whether a bundle created from the globstar fix
  (aegis-243a2h, commit 80c43c1) helps retrieve files for a later tags issue.
language: rust
tags: [bundle-eval, tags, pattern-matching]

# The "solved" issue that produced the bundle
source_issue:
  id: "aegis-243a2h"
  title: "Globstar patterns fail to match root-relative paths"
  commit: "80c43c1"

# Bundle created from that solved issue
bundle:
  slug: "tags-resolution"
  description: "Tag resolution: glob pattern matching, rule application, effect computation"
  files:
    - src/tags.rs
    - src/search/context.rs    # apply_tag_effects
    - src/config.rs            # TagsConfig loading
  keywords: ["tags", "glob", "pattern", "tag resolution", "tag effects"]

# The "later" issue we test retrieval on
target_issue:
  id: "aegis-ezhqml"
  title: "Gate threshold too high after tag boost interaction"
  query: "tag effects not applying correctly to gate threshold"

# Ground truth: files an expert would need for the target issue
ground_truth_files:
  - src/tags.rs                  # resolve_effect, tag boost logic
  - src/search/context.rs        # apply_tag_effects, gate check
  - src/config.rs                # gate_threshold, HooksConfig

# Files that should NOT appear (noise)
noise_files:
  - src/cli/hook.rs              # injection path, not relevant to gate scoring
  - src/storage/feedback.rs      # feedback system, irrelevant
```

### Metrics

For each eval task, compare three retrieval modes:

| Mode | Description | What it tests |
|------|-------------|---------------|
| **Baseline** | `/context?q=<target_query>` | How good is organic search alone? |
| **Bundle-scoped** | `/context?q=<target_query>&bundle=<slug>` | Does scoping to bundle files improve precision? |
| **Bundle-injected** | Organic search + bundle keyword match | Does the bundle supplement organic results? |

Per mode:

- **Precision**: `|retrieved ∩ ground_truth| / |retrieved|`
- **Recall**: `|retrieved ∩ ground_truth| / |ground_truth|`
- **F1**: Harmonic mean
- **Budget efficiency**: Lines on ground-truth files / total lines used
- **Noise ratio**: `|retrieved ∩ noise_files| / |retrieved|`

### Eval Task Matrix (Real Issues)

| ID | Source Issue (bundle from) | Target Issue (test on) | Bundle | Tests |
|----|---------------------------|----------------------|--------|-------|
| `bundle-eval-001` | aegis-243a2h (globstar fix) | aegis-ezhqml (gate threshold) | `tags-resolution` | Tags subsystem knowledge transfer |
| `bundle-eval-002` | aegis-3lw8zu (noise filter) | deploy/ noise bug (commit 1265229) | `hook-injection` | Hook + context assembly knowledge |
| `bundle-eval-003` | Bridge budget fix | Lance DB corruption recovery | `search-backend` | Search infra knowledge |
| `bundle-eval-004` | (no bundle) | aegis-ezhqml (gate threshold) | — | **Control**: baseline without any bundle |
| `bundle-eval-005` | aegis-qalm1v (dir nav injection) | Automated message skip (aegis-3lw8zu) | `hook-injection` | Cross-issue in same subsystem |
| `bundle-eval-006` | Tags v2 deploy | Tags v5 deploy (pensieve paths) | `tags-deploy` | Deployment procedure knowledge |

### Expected Outcomes

**Hypothesis 1: Subsystem bundles improve precision.**
A bundle from a solved tags issue should eliminate noise files (feedback.rs, hook.rs)
when retrieving for another tags issue. Expected: +15-25% precision over baseline.

**Hypothesis 2: Bundles improve recall for cross-file fixes.**
Fixes that span multiple files (tags.rs + context.rs + config.rs) are hard for organic
search — it often finds 2 of 3. A bundle that captured all 3 from a prior fix should
achieve near-100% recall. Expected: +10-20% recall over baseline.

**Hypothesis 3: Bundle-injected mode is the sweet spot.**
Pure bundle-scoped may miss edge cases. Pure organic may miss core files. The combined
mode (organic + bundle reservation) should have the best F1. Expected: highest F1 of
all three modes.

**Hypothesis 4: Knowledge compounds.**
For eval tasks where the source and target issues are in the same subsystem, the
improvement should be larger than for loosely related issues. This validates that
bundles capture subsystem knowledge, not just file lists.

### Runner Integration

```bash
# Run a single eval task in all three modes
python3 -m runner.cli run-bundle-eval bundle-eval-001

# Output:
# bundle-eval-001: Tags pattern matching → gate threshold
#   Baseline:        P=0.60  R=0.67  F1=0.63  Budget=45%  Noise=0.20
#   Bundle-scoped:   P=0.90  R=0.67  F1=0.77  Budget=72%  Noise=0.00
#   Bundle-injected: P=0.82  R=1.00  F1=0.90  Budget=68%  Noise=0.09

# Run all bundle eval tasks
python3 -m runner.cli run-bundle-eval --all

# Compare across all tasks
python3 -m runner.cli score --bundle-eval --compare-modes
# Outputs aggregate F1 delta table
```

The scorer reads ground-truth from the YAML, queries bobbin's API in each mode,
computes set intersection metrics, and produces a comparison table.

### Bootstrapping: Creating the First Bundles for Eval

Before running evals, we need bundles to test with. These are created by examining
our actual fix history:

```bash
# From aegis-243a2h (globstar fix) — we know the files involved
bobbin bundle create "tags-resolution" \
  --description "Tag resolution: glob matching, rule application, effect computation" \
  --files "src/tags.rs,src/search/context.rs,src/config.rs" \
  --keywords "tags,glob,pattern,tag resolution,tag effects,scoped effects"

# From aegis-3lw8zu (noise filter fix)
bobbin bundle create "hook-injection" \
  --description "Hook injection: prompt processing, noise filtering, context formatting" \
  --files "src/cli/hook.rs,src/search/context.rs" \
  --keywords "hook,injection,noise filter,automated message,prompt"
```

This is intentionally manual for v1 — the auto-discovery feature (Phase 7) would
generate these automatically from coupling data.

## Implementation Phases

### Phase 1: Bundle Config + CLI (Small)
- Add `[[bundles]]` parsing to `TagsConfig` in `tags.rs`
- `BundleConfig` struct: name, description, keywords, tags, files, docs, children, includes
- `bobbin bundle list` and `bobbin bundle show <name>` CLI commands
- L0 (map) and L1 (outline with `list_symbols`) output
- **No search changes yet** — this is pure config + display

### Phase 2: Search Integration (Medium)
- `bundle` param on `/context` and `/search` endpoints
- Bundle membership → `extra_filter` SQL clause
- `level` param controlling ContentMode per bundle query
- `bundles` field in API response when keywords match
- Keyword matching in `HooksConfig` extended for bundles

### Phase 3: Hook Injection Integration (Medium)
- Pre-search bundle keyword check in `hook.rs`
- Budget reservation for matched bundles
- Bundle annotation in injection output header
- `bundle` column in `injections` table for feedback tracking

### Phase 4: Agent-Created Bundles (Medium)
- `bundles` table in metadata.db
- `bobbin bundle create/add/remove` CLI commands
- Merge agent-created + config-defined at runtime
- MCP tool wrappers

### Phase 5: Beads Integration via Labels (Small)
- Convention: `b:<slug>` labels on beads (zero beads changes)
- `bundle_usage` table in bobbin metadata.db for tracking associations
- `bobbin bundle stats` aggregates usage across beads
- Document the `b:` label convention in agent guides

### Phase 6: Bundle-Aware Planning Formula (Medium)
- New formula: `mol-plan-with-bundles` (or extend `mol-idea-to-plan`)
- Research phase creates a bundle from discovered files
- Bead-filing phase labels all beads with `b:<slug>`
- Progressive disclosure: polecat runs `bobbin bundle show <slug>` on pickup
- Formula template includes bundle creation + labeling steps

### Phase 7: Auto-Discovery + Freshness (Stretch)
- Coupling-based cluster detection for `bundle suggest`
- Staleness detection and `bundle refresh`
- Bundle-scoped feedback stats

### Phase 8: Eval Framework (Parallel with Phase 1-2)
- Write `bundle-*.yaml` eval tasks with real historical issues
- Extend runner with `--mode` param (baseline/bundle-scoped/bundle-injected)
- Before/after eval: create bundle from solved issue, test on related later issue
- F1 scoring with ground-truth file comparison
- Comparison table output

## Bundle-Aware Planning Formula

A Gas Town formula that incorporates bundles into the planning→execution pipeline.
This is the full integration story: an idea enters the system, the formula researches
it, creates a bundle capturing the knowledge, and files beads labeled with the slug.

### Formula: `mol-plan-with-bundles`

```
Idea → Research → Bundle Creation → PRD → Plan → Beads (labeled b:<slug>)
```

**Step 1: Research** (existing pattern from `mol-idea-to-plan`)
- Bobbin search for relevant files
- Code exploration, dependency analysis
- Identify the file set that matters

**Step 2: Bundle Creation** (new)
- From the research phase, create a bundle capturing discovered files
- Generate keywords from the research queries that produced good results
- Slug auto-generated from the plan title

```bash
bobbin bundle create "<plan-slug>" \
  --description "<one-line from PRD>" \
  --files "<discovered files>" \
  --keywords "<research queries that worked>"
```

**Step 3: PRD + Plan** (existing pattern)
- PRD references the bundle: "Implementation context: b:<slug>"
- Plan steps reference specific sub-bundles or files within the bundle

**Step 4: Bead Filing** (existing pattern + bundle label)
- Each bead gets labeled `b:<slug>`
- Bead descriptions reference the bundle for implementation context

```bash
bd create "<task title>" -t task -p 2 -l "b:<slug>"
```

**Result**: Every polecat that picks up a bead from this plan can run
`bobbin bundle show <slug>` and get the exact context the planning agent
assembled — no cold start, no redundant exploration.

### Formula Placement

This formula belongs in the **gastown** repo (where other formulas live). It extends
`mol-idea-to-plan` rather than replacing it — the bundle creation step slots in
between research and PRD generation.

Alternatively, it could be a standalone formula (`mol-bundle-plan`) that wraps
`mol-idea-to-plan` and adds the bundle step. This avoids modifying the existing
formula and lets teams opt in.

## Open Questions

1. **Bundle versioning**: Should bundles track version history? Probably not initially —
   git history of `tags.toml` provides this implicitly for config-defined bundles.

2. **Maximum bundle size**: Should we cap files per bundle? Large bundles defeat the
   purpose (they become "search everything"). Suggested limit: 20 files per bundle,
   use hierarchy for larger subsystems.

3. **LLM-generated descriptions**: At L1, should we use the embedding model to generate
   a natural-language summary of the bundle's purpose from its member files? Could be
   expensive but very useful for onboarding.

4. **Formula ownership**: Should `mol-plan-with-bundles` live in gastown (formula home)
   or bobbin (bundle owner)? Gastown makes sense for the formula runtime, but bobbin
   needs to provide the `bundle create` primitive.

5. **Slug collision**: What happens when two agents create bundles with the same
   auto-generated slug? Options: fail loudly, auto-suffix, merge.
