# Plan: Smart Dispatch — Tool-Aware Hook Responses

_Strider — 2026-02-27_

## Problem

Bobbin's context injection fires **once** per eval task. In Claude Code `-p` mode,
`UserPromptSubmit` triggers on the single initial prompt only — the agent's subsequent
17-83 turns get zero injection. `PostToolUse` is configured but not firing in evals
(investigation needed). Even when hooks do fire, they always run the same
`inject-context` regardless of what the agent just did.

Meanwhile, the agent achieves F1=0.91 avg using **explicit** bobbin commands (search,
refs, related) — the tool works, but the passive injection system isn't leveraging it.

## Insight

The agent's tool calls are **intent signals**. When it runs `grep -r "Stmt::Import"`,
it's telling us what it's looking for. When it edits `foo.rs`, it's telling us what
changed. Bobbin should respond with the *right* command for each situation, not a
generic search-everything injection.

grep and find are bobbin's **competitors**. Every grep is a chance for bobbin to
show it can find things grep can't. Every edit is a chance to surface the test files
and snapshots that injection keeps missing.

## Design

### Tool -> Bobbin Response Matrix

| Agent tool | Bobbin responds with | Rationale |
|---|---|---|
| **Edit/Write** `foo.rs` | `bobbin related foo.rs` | Co-changing files: tests, snapshots, configs. #1 gap in evals. |
| **Edit/Write** `foo.rs` | `bobbin refs <changed-symbol>` | Callers and consumers of what just changed. Always valuable. |
| **Bash** `grep "pattern" ...` | `bobbin search "pattern"` | Semantic search for same intent. Finds what keyword misses. |
| **Bash** `find ... -name "*.rs"` | `bobbin search <extracted-intent>` | Parse find args, search semantically. |
| **Read** `foo.rs` | `bobbin refs <top-symbols>` | Show where this file's exports are used. Proactive navigation. |
| **Glob** `**/test*` | `bobbin search <pattern>` | Semantic file discovery alongside glob. |

### Always-Show vs Conditional

| Response type | Policy | Why |
|---|---|---|
| `related` after Edit | **Always show** | Co-changing files are the biggest eval gap |
| `refs` after Edit | **Always show** | Knowing callers is always valuable |
| `search` after grep/find | **Conditional** — dedup-gated | Skip if same results were recently shown |
| `refs` after Read | **Conditional** — limit to key symbols | Could be noisy on large files |

### Noise Control (existing capabilities)

- **Session dedup**: skip if result hash matches recent injection
- **Gate threshold**: filter low-relevance results (score < gate)
- **Budget cap**: total injected lines capped at configured budget
- **Frequency**: don't re-inject `related` for same file within N turns

### Input Parsing

The `PostToolUse` hook receives JSON on stdin with `tool_name` and `tool_input`.
For Bash commands, we parse the command string:

```
grep -r "pattern" path/  →  extract "pattern" → bobbin search "pattern"
find . -name "*.test.*"  →  extract "test" intent → bobbin search "test files"
rg "symbol" --type rust   →  extract "symbol" → bobbin search "symbol"
```

For Edit/Write, we get the file path directly. For symbol extraction from edits,
we diff the old/new content and find changed function/struct/impl names.

## Priority Order

| Priority | Task | Impact | Effort |
|---|---|---|---|
| **P0** | Fix PostToolUse not firing in eval `-p` mode | Unblocks everything | Small (investigation) |
| **P1** | Edit → `bobbin related <file>` | Biggest eval gap (tests/snapshots) | Medium |
| **P1** | Bash(grep) → `bobbin search <query>` | Competitive advantage over raw grep | Medium |
| **P2** | Edit → `bobbin refs <symbol>` | Always valuable, needs symbol extraction | Medium |
| **P2** | Read → `bobbin refs <symbols>` | Proactive but needs file parsing | Medium |
| **P3** | Bash(find) → `bobbin search <intent>` | Nice-to-have, harder to parse | Small |

## Implementation

### Phase 1: Fix PostToolUse in evals (P0) — RESOLVED

**Investigation complete (2026-02-27)**. PostToolUse DOES fire in `-p` mode.
Confirmed by direct testing: simple echo hooks fire for every tool call.

**Root cause of zero metrics**: The current `run_post_tool_use_inner` exits
silently at line 1632 (`if coupled.is_empty() && symbols.is_empty()`) before
emitting metrics. The eval files (deeply nested, newly created) have no coupling
data and no matching symbols. The hook runs but has nothing to report.

**Not a Claude Code bug — a bobbin design gap.** The fix is Phase 2: make the
hook always do a semantic search (`bobbin related`) rather than relying solely on
pre-existing coupling/symbol data.

**Key debugging insight**: PostToolUse/PostToolUseFailure events do NOT appear
in Claude Code's `--output-format stream-json` stream. Only bobbin's own
`*_metrics.jsonl` shows whether hooks actually fired.

### Phase 2: Smart dispatch for Edit (P1) — SHIPPED (b4ddfe1)

**Shipped 2026-02-27.** PostToolUse now runs full hybrid search via
ContextAssembler on every Edit/Write. Uses calibrated config cascade.
FTS index reuse via `replace(false)` keeps latency <1s.

### Phase 3: Grep/Search competitor response (P1) — SHIPPED (bdd0ce9)

**Shipped 2026-02-27.** Extended PostToolUse dispatch to intercept:
- `Bash(grep/rg/find)` — parses command string, extracts search pattern
- `Grep` tool — extracts pattern directly
- `Glob` tool — extracts glob intent, converts to semantic query

Matcher updated from `Write|Edit` to `Write|Edit|Bash|Grep|Glob`.
Includes proper flag parsing, quoted string support, and regex cleanup.
Output framed as "Bobbin Semantic Matches" with the original command shown.

### Phase 4: Refs integration (P2)

Add symbol-aware responses:

1. After Edit: diff old/new, extract changed symbol names
2. After Read: parse file, identify top-level symbols
3. Run `bobbin refs <symbol>` for each
4. Always show (refs are always valuable), but limit to top 3 symbols

## Metrics

Track per-eval:
- `dispatch_events`: count by type (related, search, refs)
- `dispatch_hit_rate`: fraction where dispatched results overlap ground truth files
- `competitor_commands`: grep/find count vs bobbin search count
- `turns_to_edit_with_dispatch` vs `turns_to_edit_without` (A/B comparison)

## Evidence from Eval Data (Feb 27)

| Task | Injection P/R | Bobbin cmds | grep/find | 1st Edit | F1 |
|---|---|---|---|---|---|
| ruff-001 | 0/0 | 3 | 0 | 25 | 1.00 |
| ruff-002 | 0.07/0.50 | 3 | 0 | 20 | 0.57 |
| ruff-003 | 0/0 | 3 | 22 | **83** | 1.00 |
| ruff-004 | 0/0 | 3 | 0 | 17 | 1.00 |
| ruff-005 | 0/0 | 2 | 0 | 21 | 1.00 |

ruff-003 is the case study: agent used bobbin early, fell back to 22 grep/find
calls, didn't edit until turn 83. Smart dispatch would have intercepted those
greps with semantic searches, potentially halving time-to-edit.

## Risks

- **Latency**: each dispatch adds ~2s (search/related). Mitigate with async, caching.
- **Noise**: too many injections overwhelm the agent. Mitigate with dedup + budget.
- **Parsing fragility**: grep/find command extraction may miss edge cases. Start simple.
- **PostToolUse may not work in -p mode**: ~~if it's a Claude Code limitation~~ RESOLVED.
  PostToolUse works in -p mode. The remaining issue was Gas Town's `CLAUDE_CONFIG_DIR`
  redirecting settings away from `~/.claude/settings.json`. Fixed 2026-02-28 by adding
  hooks to all account-specific settings files under `~/.claude-accounts/`.
