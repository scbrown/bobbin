# Bobbin + Beads Integration Plan

**Epic:** bo-flq4v
**Status:** Planning
**Priority:** P1
**Date:** 2026-02-14

## Overview

Index beads (Dolt-backed issue tracker) data into bobbin's LanceDB vector store
so agents can semantically search across issues, comments, and context alongside code.

## Motivation

Agents currently search beads linearly via `bd list` / `bd show`. With vector indexing:
- `bobbin search "disk pressure"` finds both code AND related beads
- UserPromptSubmit hooks auto-inject relevant beads as context
- Cross-rig search: find beads across aegis, gastown, bobbin from one query

## Architecture

```
Dolt (dolt.svc:3306)          Bobbin Index (LanceDB)
┌──────────────────┐          ┌─────────────────────┐
│ beads_aegis      │──fetch──→│ chunks table         │
│ beads_gastown    │          │  code chunks (existing)
│ beads_bobbin     │          │  beads chunks (NEW)  │
└──────────────────┘          │  file_path: beads:*  │
                              └──────┬──────────────┘
                                     │ hybrid search
                              ┌──────▼──────────────┐
                              │ MCP: search_beads    │
                              │ Hook: auto-inject    │
                              └─────────────────────┘
```

## Data Flow

### Indexing (write path)

Two triggers:
1. **Batch:** `bobbin index --include-beads` — full re-index from Dolt
2. **Incremental:** beads post-write hook calls `bobbin index-bead <id>` (preferred)

### Query (read path)

1. Agent query → embedder → 384-dim vector
2. Hybrid search (semantic + keyword) on LanceDB, filtered by chunk_type
3. Return structured results with bead ID, title, priority, status, snippet

## Chunk Schema

Each bead becomes a Chunk in LanceDB:

| Field | Value | Example |
|-------|-------|---------|
| id | SHA256("beads:{rig}:{bead_id}") | `a1b2c3...` |
| file_path | `beads:{rig}:{bead_id}` | `beads:aegis:aegis-0a9` |
| chunk_type | `issue` | — |
| chunk_name | issue title | `"TLS cert expiry audit"` |
| content | title + description + comments | full text for embedding |
| language | `beads` | — |
| repo | rig name | `aegis` |

## Phases

### Phase 1: Core Indexing (bo-1y7c2, bo-86l9q, bo-hbgnl)

1. **bo-1y7c2:** Add `ChunkType::Issue` to `src/types.rs`
   - New enum variant, Display impl, parse_chunk_type() update
   - ~30 lines changed across 2-3 files

2. **bo-86l9q:** Create `src/index/beads.rs`
   - Add `mysql_async` dependency to Cargo.toml
   - Connect to Dolt via MySQL protocol
   - Query issues table: id, title, description, status, priority, assignee
   - Query comments table: body, author, created_at
   - Convert to Vec<Chunk> with comments concatenated
   - Config section: `[beads]` in .bobbin/config.toml

3. **bo-hbgnl:** Wire into indexing pipeline
   - `--include-beads` CLI flag on `bobbin index`
   - Or `beads.enabled = true` in config
   - Incremental: track `beads_last_sync` in SQLite meta table
   - Only fetch beads updated since last sync

### Phase 2: MCP Tool (bo-37k7w)

4. **bo-37k7w:** Add `search_beads` MCP tool
   - Request: query, priority?, status?, assignee?, rig?, limit?
   - Hybrid search filtered by chunk_type = "issue"
   - Live Dolt query for current priority/status (may differ from indexed)
   - Response: bead_id, title, priority, status, relevance_score, snippet

### Phase 3: Hook Integration (TBD)

5. Integrate into UserPromptSubmit hook
   - Search beads alongside code when agent submits prompt
   - Include relevant issues in injected context
   - Gate by relevance threshold (avoid noise)

### Phase 4: Beads Auto-Trigger (beads-side)

6. `bd hooks` feature in beads CLI
   - Post-write hook fires `bobbin index-bead --id ${BEAD_ID}`
   - Configurable per-workspace
   - Makes indexing fully automatic

## Configuration

```toml
# .bobbin/config.toml
[beads]
enabled = true
host = "dolt.svc"
port = 3306
user = "root"
databases = ["beads_aegis", "beads_gastown", "beads_bobbin"]

[beads.index]
include_comments = true
include_closed = false       # skip closed beads by default
max_age_days = 90           # skip very old beads
```

## Dependencies

- Dolt server at dolt.svc (192.168.7.236:3306) — already running
- `mysql_async` Rust crate for Dolt queries
- Bobbin embedding model (all-MiniLM-L6-v2) — already bundled
- Beads table schema (issues, comments) — stable

## Testing

- Unit: mock Dolt responses, verify Chunk creation
- Integration: connect to dolt.svc, index real beads, verify search
- E2E: `bobbin index --include-beads && bobbin search "cert expiry"` finds cert beads

## Risks

- Dolt connection from bobbin build environment (needs network access to dolt.svc)
- Beads schema changes could break indexer (mitigate: version check on connect)
- Large comment threads may exceed embedding token limit (mitigate: truncate/split)
