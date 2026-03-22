---
title: Archive Integration
description: Indexing structured markdown records from external systems
tags: [archive, guide]
status: draft
category: guide
related: [config/reference.md, guides/searching.md]
---

# Archive Integration

Bobbin can index structured markdown records alongside code — agent memories, communication logs, HLA records, or any collection of markdown files with YAML frontmatter. Archive records are searchable via the same search and context APIs as code.

## Configuration

Archive sources are configured in `.bobbin/config.toml`:

```toml
[archive]
enabled = true
webhook_secret = ""  # Optional: Forgejo webhook auth token

[[archive.sources]]
name = "pensieve"
path = "/var/lib/bobbin/archives/pensieve"
schema = "agent-memory"
name_field = "agent"

[[archive.sources]]
name = "hla"
path = "/var/lib/bobbin/archives/hla"
schema = "human-intent"
name_field = "channel"
```

### Source fields

| Field | Type | Description |
|-------|------|-------------|
| `name` | string | Source label — used as language tag in chunks and as a search filter |
| `path` | string | Filesystem path to the directory of markdown records |
| `schema` | string | YAML frontmatter value to match (e.g., `"agent-memory"`) — files without this in frontmatter are skipped |
| `name_field` | string | Optional frontmatter field used to prefix chunk names (e.g., `"channel"` → `"telegram/{record_id}"`) |

## Record Format

Archive records are markdown files with YAML frontmatter:

```markdown
---
schema: agent-memory
id: mem-2026-0322-abc
timestamp: 2026-03-22T12:00:00Z
agent: stryder
tags: [bobbin, search-quality]
---

## Context

Discovered that tag effects only apply via /context endpoint, not /search.
The CLI returns raw LanceDB scores without boosts.
```

The frontmatter must contain the `schema` value matching your source config. Other fields (`id`, `timestamp`, etc.) are extracted as metadata.

### Field handling

- **`id`** — Record identifier (used in chunk ID generation)
- **`timestamp`** — Parsed for date-based file path grouping (`YYYY/MM/DD/`)
- **`source:`** block — Nested keys are flattened (e.g., `source:\n  channel: telegram` becomes field `channel`)
- **Chunk IDs** — Generated via `SHA256(source:id:timestamp)` for deduplication

## Searching Archives

Archive records appear in regular search results. Filter by source name:

```bash
bobbin search "agent memory about search quality" --repo pensieve
```

### HTTP API

| Endpoint | Description |
|----------|-------------|
| `GET /archive/search?q=<query>&source=<name>&limit=10` | Search archive records |
| `GET /archive/entry/{id}` | Fetch a single record by ID |
| `GET /archive/recent?days=30&source=<name>` | Recent records with optional date range |

### Web UI

Toggle "Include archive" in the Search tab to merge archive results into code search.

## Webhook Integration

For automatic re-indexing when archive sources are updated via git push:

```toml
[archive]
webhook_secret = "your-secret-token"
```

Configure a Forgejo/Gitea push webhook pointing to `POST /webhook/push`. When a push event matches a configured repo, bobbin triggers an incremental re-index of that source.

## Use Cases

- **Agent memories** (pensieve): Index agent context snapshots for cross-agent search
- **Communication logs** (HLA): Index human-agent interaction records
- **Knowledge bases**: Index structured documentation collections
- **Incident records**: Index postmortem and investigation reports

## Indexing

Archive sources are indexed alongside code during `bobbin index`. The `--force` flag re-indexes all records:

```bash
bobbin index /var/lib/bobbin --force
```

Records are chunked like markdown files — headings create chunk boundaries, with frontmatter metadata preserved as chunk attributes.
