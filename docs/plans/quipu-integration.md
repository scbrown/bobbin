# 🪢 Quipu Integration Plan

> **Implementation status (2026-08-17, Claude):** 🟡 **Partial — no longer dark. Two phases genuinely incomplete.**
> Re-measured per phase against the source; supersedes the 2026-07-23 banner below, whose central claim is
> now stale.
>
> | Phase | State | Evidence |
> |---|---|---|
> | 1. Crate dependency | ✅ Built | `Cargo.toml:135`, pinned `=0.2.0` at rev `7f984b4e`; `knowledge = ["dep:quipu"]` at `:145` |
> | 2. Shared embedding pipeline | ❌ **Not built** | `EmbeddingProvider` has **zero occurrences** anywhere in `src/`. The trait-in-quipu / impl-in-bobbin design below was never implemented. |
> | 3. MCP tool surface | 🟡 **Half** | `knowledge_context` (`src/mcp/server.rs:2678`) and `knowledge_query` (`:2790`) exist. `knowledge_knot` and `knowledge_validate` — both in the Phase 3 table below — have **zero occurrences**. Read is wired; write and validate are not. |
> | 4. Unified search | ✅ Built | 61 `knowledge` references in `src/search/context.rs`, plus `src/http/handlers/search.rs` |
> | 5. Knowledge-aware assembly / coupling export | ✅ Built | `src/knowledge/coupling.rs` (190 lines) |
>
> **The darkness is resolved.** The previous banner's load-bearing claim — "sits behind the `knowledge`
> feature that **no build path enables**" — no longer holds:
>
> - `.github/workflows/ci.yml` runs `cargo check`, `cargo test` and `cargo clippy` **both** with
>   `--features knowledge` and without (lines 38, 44, 50 vs 40, 46, 52), so the integration is compiled
>   and tested on every run, and the default build's `cfg(not(knowledge))` arms are covered too.
> - `.github/workflows/release.yml:128` builds release binaries **with** `--features knowledge`, annotated
>   there as REQUIRED rather than optional.
>
> Verified locally: `cargo check --features knowledge` compiles clean.
>
> So GH #56's un-darking gate is closed, and what remains of this epic is **Phase 2 in full and Phase 3's
> write half** — not the whole integration. See `bobbin-di7` for the specification that follows from this.

> **Graph-export substrate (2026-07-23, billy — additive to harding's block above):**
> Phase 4's "query a **selected subset** of graphs" (and later federation) rests on
> Quipu's named-graph (quad) support, quipu #36. Its store layer shipped; the SPARQL
> query surface (`GRAPH <iri>` / `GRAPH ?g`) that lets a consumer read one named
> subgraph is added in [quipu#49](https://github.com/scbrown/quipu/pull/49). So the
> export substrate is landing upstream even while this integration stays feature-dark
> (GH #56).

> Bobbin holds the thread (code context). Quipu ties knots of structured meaning into it.

## Goal

Integrate Quipu as Bobbin's knowledge graph subsystem, enabling unified
search across code chunks and knowledge entities.

## Current State

- **Bobbin**: semantic code indexer — LanceDB vectors, ONNX embeddings, MCP tools, HTTP API
- **Quipu**: knowledge graph — EAVT fact log, SPARQL, SHACL, vector search, MCP tools, REST API
- **Integration**: zero. Quipu is not a Bobbin dependency. No shared code paths.

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

## Integration Phases

### Phase 1: Crate Dependency 🔗

Add quipu as a git dependency in Bobbin's Cargo.toml. Feature-gated
behind `knowledge` feature so Bobbin compiles without quipu.

```toml
[dependencies]
quipu = { git = "https://github.com/scbrown/quipu", optional = true }

[features]
knowledge = ["dep:quipu"]
```

**Key decision**: Quipu is sync, Bobbin is async. Bridge with
`tokio::task::spawn_blocking()` for Quipu calls from async Bobbin code.

### Phase 2: Shared Embedding Pipeline 🧠

Bobbin already has ONNX embeddings (`src/index/embedder.rs`). Quipu
needs the same embeddings for vector search. Rather than duplicating:

1. Define `EmbeddingProvider` trait in Quipu
2. Implement it in Bobbin wrapping existing `Embedder`
3. Pass `Arc<dyn EmbeddingProvider>` to Quipu's Store

This lets Quipu auto-embed entities using Bobbin's model session.

### Phase 3: MCP Tool Surface 🔧

Register Quipu's MCP tools alongside Bobbin's in the MCP server:

| Tool | Source | Purpose |
|------|--------|---------|
| `knowledge_query` | Quipu | SPARQL queries against knowledge graph |
| `knowledge_context` | Quipu | Knowledge entities for a topic |
| `knowledge_knot` | Quipu | Write facts to the graph |
| `knowledge_validate` | Quipu | SHACL validation |

Wire into Bobbin's `serve` command — single MCP server, both tool sets.

### Phase 4: Unified Search Results 🔍

When a user searches, merge results from both sources:

1. Bobbin code search → code chunks with scores
2. Quipu vector search → knowledge entities with scores
3. Normalize scores (different ranges)
4. Interleave by relevance
5. Return unified results with `source: "code" | "knowledge"` tag

### Phase 5: Knowledge-Aware Context Assembly 📚

Enhance Bobbin's `context` command to include knowledge graph facts:

- If code mentions an entity name, expand with knowledge context
- If a function calls a service, include service topology from graph
- Budget-aware: knowledge context competes for the same token budget

## Dependencies

| Bobbin Task | Depends On |
|------------|------------|
| Crate linkage | Quipu CI green, stable API |
| Shared embeddings | `EmbeddingProvider` trait in Quipu (qp-sbu.2) |
| MCP tools | Crate linkage |
| Unified search | MCP tools, shared embeddings |
| Knowledge-aware context | Unified search |

## Config

```toml
# .bobbin/config.toml
[knowledge]
enabled = true
store_path = ".bobbin/knowledge.db"
schema_path = "schemas/"          # SHACL shapes
auto_embed = true                 # embed entities using Bobbin's ONNX pipeline
```

## Risk Mitigation

- **Compile time**: Quipu pulls in oxrdf, spargebra, rudof — heavy. Feature-gate.
- **SQLite conflicts**: Both use rusqlite. Pin same version.
- **API instability**: Quipu is 0.1.0. Pin to git commit until 0.2.0.

## Open Questions

1. Should Quipu's REST server (`quipu-server`) be merged into Bobbin's HTTP
   server, or stay separate? (Leaning: merge — one port, one process.)
2. LanceDB for both code vectors and knowledge vectors, or keep separate
   stores? (Leaning: separate — different schemas, different lifecycles.)
3. How do we handle Quipu schema migrations when Bobbin updates?
