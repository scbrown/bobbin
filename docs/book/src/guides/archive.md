---
title: Archive Integration
description: Indexing structured markdown records (HLA, Pensieve) for semantic search
tags: [archive, guide]
status: draft
category: guide
related: [config/reference.md, mcp/tools.md]
---

# Archive Integration

Bobbin can index structured markdown records alongside code. This enables semantic search over human directives (HLA), agent memories (Pensieve), or any YAML-frontmatter markdown collection.

## Configuration

Enable archive indexing in `.bobbin/config.toml`:

```toml
[archive]
enabled = true

[[archive.sources]]
name = "hla"
path = "/var/lib/bobbin/repos/hla-records"
schema = "human-intent"
name_field = "source.context"

[[archive.sources]]
name = "pensieve"
path = "/var/lib/bobbin/repos/pensieve/records"
schema = "pensieve"
name_field = "agent"
```

### Source fields

| Field | Required | Description |
|-------|----------|-------------|
| `name` | yes | Label for this source (used as language tag, path prefix, and filter value) |
| `path` | yes | Filesystem path to the records directory |
| `schema` | yes | Substring to match in YAML frontmatter `schema:` field |
| `name_field` | no | Frontmatter field to use as chunk name prefix |

## Record format

Records are markdown files with YAML frontmatter:

```markdown
---
schema: human-intent/v2
id: hi-01ARYZ6S41
timestamp: 2026-02-17T14:32:00Z
author: stiwi
source:
  channel: telegram
  context: dm
---

Deploy bobbin to luvu, not the old CT.
Make sure traefik points to the new host.
```

### Schema matching

The `schema` config value is matched as a **substring** of the frontmatter `schema:` field:
- Config `schema = "human-intent"` matches `schema: human-intent/v2`
- Config `schema = "agent-memory"` matches `schema: agent-memory/v1`

Records without a matching schema are silently skipped.

### Required frontmatter

- `id` — unique record identifier
- `timestamp` — ISO 8601 datetime

All other fields are stored as metadata.

## Chunk naming

The `name_field` controls how chunks are named in search results:

| `name_field` | Frontmatter | Chunk name |
|-------------|-------------|------------|
| `"channel"` | `channel: telegram` | `telegram/hi-01ARYZ6S41` |
| `"agent"` | `agent: aegis/crew/arnold` | `aegis/crew/arnold/pm-01BXYZ7T52` |
| `""` (empty) | — | `hi-01ARYZ6S41` (id only) |

This enables filtering by name prefix in the API (e.g., `filter=telegram`).

## File paths

Chunks get paths like `hla:2026/02/17/hi-01ARYZ6S41.md`. If the filesystem already uses date-partitioned directories (`YYYY/MM/DD/`), those are preserved. Otherwise, the date is extracted from the `timestamp` field.

## Chunk IDs

IDs are deterministic: `SHA256(source:id:timestamp)`. Re-indexing produces the same IDs, so duplicates are automatically deduplicated.

## Indexing

Archive records are indexed as part of `bobbin index`. They're re-indexed completely each run (old chunks deleted, new ones added).

```bash
bobbin index  # Indexes both code and archive sources
```

## Searching archives

### HTTP API

```bash
# Search across all archives
GET /archive/search?q=deploy+failures

# Filter by source
GET /archive/search?q=deploy&source=hla

# Filter by name_field value
GET /archive/search?q=error&source=hla&filter=telegram

# Date range
GET /archive/search?q=ssl&after=2026-03-01&before=2026-03-15

# Recent records
GET /archive/recent?source=pensieve&after=2026-03-01&limit=20

# Single record
GET /archive/entry/hi-01ARYZ6S41
```

### MCP tools

Two MCP tools expose archive search:
- `search_archive` — semantic/keyword search with source and limit filters
- `list_archive_recent` — list recent records by source and date

## Webhook push notifications

Configure a Forgejo webhook to trigger re-indexing on push:

```
POST /webhook/push
```

The server accepts a Forgejo push payload, extracts the repo name, and spawns a background re-index. Response is immediate (`{"status": "accepted"}`).

The `webhook_secret` config field is reserved for future HMAC signature validation but is not currently enforced.

## Use cases

**HLA (Human Intent Archive)**: Store directives from project owners. Agents query HLA to retrieve historical decisions and context for current work.

**Pensieve (Agent Memory)**: Store agent learnings and observations. Prevents re-learning patterns across sessions. Each agent's memories are namespaced by the `agent` name_field.
