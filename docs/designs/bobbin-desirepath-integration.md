# Design: Bobbin Integration with Desirepath

**Bead**: aegis-hq2j86
**Author**: stryder
**Date**: 2026-03-05
**Status**: Draft

## Problem

Agents fail 13,800+ tool calls per month. These failures are captured by desirepath
(~/.dp/desires.db) but the data sits unused. Bobbin could index this failure data
and inject corrective context when agents hit similar errors, reducing wasted tokens
and improving agent autonomy.

## Data Source

**desirepath** tracks Claude Code tool failures via PostToolUseFailure hooks.

- **Location**: `~/.dp/desires.db` (SQLite, ~14K records/month)
- **REST API**: `http://localhost:7273`
- **Tables**:
  - `desires` — failed tool calls (tool_name, tool_input, error, source, session_id, cwd, timestamp, category)
  - `invocations` — all tool calls with error flag (includes turn_id, turn_sequence, turn_length)
  - `aliases` — correction rules (from_name → to_name mappings for common mistakes)

### Data Profile (2026-02-09 to 2026-03-05)

| Category | Count | % | Bobbin Action |
|----------|-------|---|---------------|
| Bash failures | 12,836 | 93% | CLI correction, diagnostic injection |
| Read failures | 499 | 3.6% | Navigation context |
| MCP failures | 440 | 3.2% | Health/fallback injection |
| Other | 49 | 0.4% | — |

### Top Failure Patterns

| Pattern | Failures/month | Root Cause |
|---------|---------------|------------|
| gt/bd CLI misuse | 1,107 | Wrong flags, nonexistent commands |
| Git push rejected | 334 | Didn't pull before push |
| BD flag guessing | 364 | --assign vs --assignee, --comment vs --append-notes |
| Read on directories | 162 | Agents don't know to use ls |
| File not found | 218 | Wrong path guesses |
| Not a git repo | 188 | Wrong cwd |
| MCP server down | 440 | Infrastructure, not agent error |

## What Bobbin Should Index

### Source 1: Aliases Table (correction rules)

The `aliases` table contains pre-built correction mappings:

```
--assign  → --assignee (-a)    [bd flag]
--owner   → --assignee (-a)    [bd flag]
bd note X → bd update X --append-notes  [command rewrite]
```

**Index as**: Correction chunks with tags `desirepath:alias`, `tool:bd` or `tool:gt`.
**Chunk format**: One chunk per alias with the correction rule, example usage, and error message.

### Source 2: Failure Patterns (aggregated)

Don't index raw desires (too many, too noisy). Instead, aggregate into pattern chunks:

```
Pattern: "gt deacon pending" (175 failures)
This command doesn't exist. The deacon has no "pending" subcommand.
To check deacon status, use: gt rig status deacon
```

**Index as**: Pattern chunks with tags `desirepath:pattern`, frequency metadata.
**Refresh**: Nightly during reindex. Aggregate desires by error signature, keep top 50 patterns.

### Source 3: Agent-Specific Failure Profiles

Per-agent failure summaries (top 5 error types per workspace):

```
Agent: aegis/crew/ellie (1,042 failures)
- gt mol squash failures: 638 (refinery squash with wrong flags)
- Read on directories: 29
- File not found: 29
```

**Index as**: Profile chunks with tags `desirepath:profile`, `agent:<name>`.
**Use**: Inject agent-specific tips at session start.

## How Indexing Works

### New Indexer: `bobbin index --source desirepath`

Add a desirepath indexer alongside the existing git-based indexer:

1. **Connect** to `~/.dp/desires.db` (or via REST API)
2. **Aggregate** desires into pattern chunks (group by error signature)
3. **Generate** correction chunks from aliases table
4. **Generate** agent profile chunks from cwd distribution
5. **Embed** and store in LanceDB with repo="desirepath"

### Refresh Strategy

- **Incremental**: On each PostToolUseFailure, desirepath already stores the event.
  Bobbin doesn't need real-time indexing — nightly is sufficient.
- **Nightly reindex**: Add to `/opt/bobbin/reindex.sh`:
  ```bash
  bobbin index /var/lib/bobbin --repo desirepath --source desirepath
  ```

### Chunk Schema

```
repo: "desirepath"
path: "patterns/<error-signature>.dp"  or  "aliases/<from_name>.dp"
tags: "desirepath:pattern,tool:bash"  or  "desirepath:alias,tool:bd"
content: <aggregated pattern text>
metadata: { frequency: N, last_seen: timestamp, agents_affected: [...] }
```

## Search Queries This Enables

### 1. PostToolUseFailure Context Injection

When an agent gets a tool failure, bobbin can search desirepath patterns:

```
Query: "bd create --assign unknown flag"
→ Injects: "Use --assignee (-a) not --assign. Example: bd create 'title' -a aegis/crew/stryder"
```

This is the highest-value integration: inject corrections at the moment of failure.

### 2. Session Start Tips

At SessionStart, query agent-specific failure profile:

```
Query: "desirepath profile aegis/crew/ellie"
→ Injects: "Common mistakes in your workspace: use --assignee not --assign, pull before push"
```

### 3. Diagnostic Queries

Agents can explicitly search failure patterns:

```
bobbin search "what errors happen with gt mail send"
→ Shows: top failure patterns, correct syntax, common pitfalls
```

## Implementation Plan

### Phase 1: Alias-Based Corrections (P1)

1. Add `desirepath` source type to bobbin indexer config
2. Read aliases table → generate correction chunks
3. Wire into PostToolUseFailure hook: on error, search desirepath repo for corrections
4. Inject correction as context (1-3 lines, low budget cost)

**Estimated impact**: ~250 failures/month prevented (aliases cover --assign, --owner, bd note)

### Phase 2: Pattern Aggregation (P2)

1. Aggregate desires by error signature → pattern chunks
2. Nightly reindex with frequency/recency weighting
3. Search patterns on PostToolUseFailure for broader coverage
4. Tag with `feedback:hot`/`feedback:cold` based on whether corrections help

**Estimated impact**: ~800 failures/month informed (top 50 patterns)

### Phase 3: Agent Profiles (P3)

1. Per-agent failure summaries
2. Inject at SessionStart as "tips for this workspace"
3. Auto-update based on overlap tracking (did the tip prevent the error?)

**Estimated impact**: Harder to measure, but improves agent onboarding.

## Risks

- **Noise**: Injecting corrections for every failure could be noisy. Mitigate with budget limits (max 3 lines per correction) and feedback:cold tagging.
- **Stale patterns**: Desires accumulate but gt/bd CLIs evolve. Patterns may reference old errors. Mitigate with recency weighting and nightly refresh.
- **Privacy**: desires.db contains tool_input with full commands. Don't index raw commands — only aggregate patterns.

## Acceptance Criteria

1. `bobbin index --source desirepath` reads aliases and generates correction chunks
2. PostToolUseFailure hook searches desirepath repo when error matches known pattern
3. Correction injection is <=3 lines and tagged for feedback tracking
4. Nightly reindex includes desirepath pattern aggregation
5. At least the 3 existing aliases are indexed and injected on matching errors
