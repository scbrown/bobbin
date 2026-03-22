---
title: Feedback System
description: How agent ratings improve search quality over time
tags: [feedback, guide]
status: draft
category: guide
related: [guides/hooks.md, config/hooks.md]
---

# Feedback System

Bobbin tracks what context it injects and lets agents rate it. Over time, feedback data drives automatic quality improvements — files that are consistently useful get boosted, files that are consistently noise get demoted.

## How It Works

### 1. Injection Tracking

Every time bobbin injects context via hooks, it records:

- **injection_id** — Unique identifier for this injection
- **query** — The prompt that triggered the injection
- **files/chunks** — What was injected (paths, chunk IDs, content)
- **agent** — Which agent received the injection
- **session_id** — The Claude Code session

### 2. Feedback Collection

Agents rate injections as `useful`, `noise`, or `harmful`:

| Rating | Meaning |
|--------|---------|
| `useful` | The injected code was relevant and helpful |
| `noise` | The injection was irrelevant to the task |
| `harmful` | The injection was actively misleading |

Feedback is collected automatically via the `feedback_prompt_interval` config:

```toml
[hooks]
feedback_prompt_interval = 5  # Prompt every 5 injections (0 = disabled)
```

Every N injections, bobbin prompts the agent to rate up to 3 unrated injections from the current session.

### 3. Feedback Tags

Accumulated feedback generates tags on files:

| Tag | Meaning |
|-----|---------|
| `feedback:hot` | File is frequently rated as useful |
| `feedback:cold` | File is frequently rated as noise |

These tags are loaded during indexing and merged into chunk tags. Combined with tag effects:

```toml
[effects."feedback:hot"]
boost = 0.3    # Boost frequently-useful files
pin = true     # Always include if budget allows

[effects."feedback:cold"]
boost = -0.3   # Demote frequently-noisy files
```

## HTTP API

| Endpoint | Method | Description |
|----------|--------|-------------|
| `/feedback` | POST | Submit a rating: `{injection_id, agent, rating, reason}` |
| `/feedback` | GET | List feedback with filters: `?rating=noise&agent=stryder&limit=50` |
| `/feedback/stats` | GET | Aggregated stats: total injections, ratings by type, coverage |
| `/injections/{id}` | GET | View injection detail: query, files, formatted output, feedback |
| `/feedback/lineage` | POST | Record an action that resolves feedback |
| `/feedback/lineage` | GET | List resolution actions |

## Lineage Tracking

When feedback leads to a fix (new tag rule, access control change, code fix), record it via lineage:

```json
{
  "action_type": "tag_effect",
  "description": "Added type:changelog demotion to reduce changelog noise",
  "commit_hash": "abc123",
  "bead": "aegis-xyz",
  "agent": "stryder"
}
```

Valid action types: `access_rule`, `tag_effect`, `config_change`, `code_fix`, `exclusion_rule`.

Lineage records link feedback to concrete improvements, creating an audit trail of search quality evolution.

## Web UI

The **Feedback** tab in the web UI shows:

- Injection and feedback counts with coverage percentage
- Rating breakdown (useful/noise/harmful)
- Filterable feedback list by rating and agent
- Click any feedback record to see the full injection detail: what was injected, the query, files included, and all ratings

## Best Practices

1. **Keep feedback_prompt_interval low** (3-5) during initial deployment to build up data quickly
2. **Review noise patterns** in the Feedback tab — repeated noise on the same files indicates missing tag rules or exclusions
3. **Use lineage** to track what you changed in response to feedback — this closes the loop
4. **Check feedback:hot/cold tags** after reindexing to verify the system is learning from ratings
