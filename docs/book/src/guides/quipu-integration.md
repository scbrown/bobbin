---
title: Quipu Integration
description: Using Quipu's knowledge graph alongside Bobbin's code search for unified context
tags: [quipu, knowledge-graph, guide]
status: draft
category: guide
related: [mcp/tools.md, architecture/overview.md, guides/context-assembly.md]
---

# Quipu Integration

Bobbin optionally integrates with [Quipu](https://github.com/scbrown/quipu), a knowledge graph that stores structured facts as an EAVT (Entity-Attribute-Value-Time) log. When enabled, Bobbin's MCP server exposes Quipu's tools alongside its own, giving AI agents access to both code search and structured knowledge in a single session.

## Enabling the Integration

Quipu is gated behind the `knowledge` Cargo feature:

```bash
cargo build --features knowledge
```

This pulls in Quipu as a git dependency. Without the feature flag, Bobbin compiles and runs normally with no Quipu code included.

## Configuration

When the `knowledge` feature is enabled, add a `[knowledge]` section to `.bobbin/config.toml`:

```toml
[knowledge]
enabled = true
store_path = ".bobbin/knowledge.db"
schema_path = "schemas/"          # SHACL shapes for validation
auto_embed = true                 # embed entities using Bobbin's ONNX pipeline
```

## MCP Tools

With Quipu enabled, `bobbin serve` exposes two additional MCP tools:

| Tool | Description |
|------|-------------|
| `knowledge_context` | Semantic search over knowledge graph entities. Pass a natural language query and get back the most relevant entities. |
| `knowledge_query` | Run SPARQL SELECT queries directly against the knowledge graph. |

These appear alongside Bobbin's existing tools (search, grep, context, etc.) in a single MCP server.

### Example: knowledge_context

Ask for knowledge entities related to a topic:

```json
{
  "tool": "knowledge_context",
  "arguments": {
    "query": "authentication flow",
    "limit": 10
  }
}
```

### Example: knowledge_query

Run a SPARQL query against the graph:

```json
{
  "tool": "knowledge_query",
  "arguments": {
    "sparql": "SELECT ?s ?p ?o WHERE { ?s ?p ?o } LIMIT 10"
  }
}
```

## Architecture

```text
┌─────────────────────────────────────────────────────┐
│                    Agent / Claude Code               │
│                                                      │
│  MCP Tools:                                          │
│    search, context, grep, refs, ...    (Bobbin)      │
│    knowledge_context, knowledge_query  (Quipu)       │
└──────────────────────┬──────────────────────────────┘
                       │
        ┌──────────────┼──────────────┐
        │              │              │
   ┌────┴────┐    ┌────┴────┐   ┌────┴────┐
   │ Bobbin  │    │ Unified │   │  Quipu  │
   │  Code   │    │ Context │   │Knowledge│
   │ Search  │    │ Pipeline│   │  Graph  │
   └────┬────┘    └────┬────┘   └────┬────┘
        │              │              │
   ┌────┴────┐         │         ┌────┴────┐
   │ LanceDB │         │         │ SQLite  │
   │ vectors │         │         │  EAVT   │
   │ + FTS   │         │         │+ vectors│
   └─────────┘         │         └─────────┘
                       │
              ┌────────┴────────┐
              │  ONNX Embedder  │
              │ (shared session)│
              └─────────────────┘
```

Key design decisions:

- **Feature-gated**: Quipu is optional (`--features knowledge`). Bobbin works without it.
- **Async bridge**: Quipu is synchronous; Bobbin is async. Calls bridge via `tokio::task::spawn_blocking()`.
- **Shared embeddings**: Both systems use the same ONNX model session for vector generation.
- **Single MCP server**: One `bobbin serve` process exposes both Bobbin and Quipu tools.

## Integration Roadmap

The integration is being built in phases. See [docs/plans/quipu-integration.md](https://github.com/scbrown/bobbin/blob/main/docs/plans/quipu-integration.md) for the full plan.

| Phase | Status | Description |
|-------|--------|-------------|
| 1. Crate dependency | Done | Quipu as git dep, feature-gated behind `knowledge` |
| 2. Shared embedding pipeline | Planned | Shared ONNX session via `EmbeddingProvider` trait |
| 3. MCP tool surface | Done | `knowledge_context` and `knowledge_query` tools wired in |
| 4. Unified search results | Planned | Merge code + knowledge results with normalized scores |
| 5. Knowledge-aware context | Planned | Context assembly expanded with knowledge graph facts |

## See Also

- [Architecture Overview](../architecture/overview.md)
- [MCP Tools Reference](../mcp/tools.md)
- [Context Assembly](context-assembly.md)
- [Quipu Integration Plan](https://github.com/scbrown/bobbin/blob/main/docs/plans/quipu-integration.md)
