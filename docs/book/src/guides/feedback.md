---
title: Feedback System
description: How agent ratings improve search quality over time
tags: [feedback, hooks, guide]
status: draft
category: guide
related: [guides/hooks.md, guides/tags.md, config/hooks.md]
---

# Feedback System

Bobbin's feedback system tracks which injected context agents actually use, collects explicit ratings, and uses that data to auto-tag chunks as "hot" (frequently useful) or "cold" (frequently ignored). Over time, this shifts search rankings toward code that matters.

## How it works

### Implicit signals (automatic)

The PostToolUse hook tracks which injected files agents interact with:

1. After each UserPromptSubmit injection, bobbin records which files were injected
2. When the agent calls Write/Edit/Read on an injected file, that's a **positive signal**
3. If injected files go unused across 3+ consecutive sessions, that's a **negative signal**

No agent action needed — signals accumulate automatically.

### Explicit ratings

Agents can rate injections directly. Every `feedback_prompt_interval` injections (default: 5), bobbin prompts the agent to rate the last injection.

Valid ratings:

| Rating | Meaning |
|--------|---------|
| `useful` | The injected context helped with the task |
| `noise` | Irrelevant to what I was working on |
| `harmful` | Actively misleading or confusing |

Submit via HTTP API:

```
POST /feedback
{
  "injection_id": "inj-abc123",
  "rating": "useful",
  "reason": "Found the auth middleware I needed"
}
```

Or via MCP tool: `bobbin_feedback_submit`.

The `agent` field is auto-detected from `GT_ROLE` or `BD_ACTOR` env vars.

## Auto-tagging: hot and cold

Feedback signals are converted to tags during indexing:

| Condition | Tag assigned | Effect |
|-----------|-------------|--------|
| 5+ sessions with positive signals | `feedback:hot` | `boost = 0.3` (30% score increase) |
| 10+ sessions with negative signals | `feedback:cold` | `boost = -0.5` (50% score decrease) |

These tag effects are applied during context assembly (see [Tags & Effects](tags.md)). The `/search` endpoint does **not** apply boost/demotion — only `/context` and hook injection do.

## Configuration

```toml
[hooks]
# Prompt agents for feedback every N injections (0 = disabled)
feedback_prompt_interval = 5
```

## Storage schema

Feedback data lives in `.bobbin/feedback.db` (SQLite):

**injections** — what bobbin injected:
- `injection_id`, `session_id`, `agent`, `query`, `files_json`, `chunks_json`, `total_chunks`, `budget_lines`

**feedback** — agent ratings:
- `injection_id`, `agent`, `rating` (useful/noise/harmful), `reason`, `timestamp`

**lineage** — traces feedback to fixes:
- `action_type` (access_rule, tag_effect, config_change, code_fix, exclusion_rule)
- Links feedback IDs to beads and commits that addressed the issue

## Feedback stats

Query aggregate stats via:

```
GET /feedback/stats
```

Returns: `total_injections`, `total_feedback`, `useful`/`noise`/`harmful` counts, `actioned`/`unactioned` (feedback with/without linked fixes).

## Quality improvement loop

```
Agent receives injection
    ↓
PostToolUse → positive/negative signal recorded
    ↓
After threshold sessions → feedback:hot or feedback:cold tag
    ↓
Tag effects adjust scoring in context assembly
    ↓
Future agents see better-ranked results
```

The loop is self-reinforcing: useful code rises, noisy code sinks, and the system adapts to actual usage patterns without manual tuning.
