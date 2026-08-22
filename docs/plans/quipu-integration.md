# 🪢 Quipu Integration Plan

> **Update (2026-08-22, strider):** the quipu pin moved to **0.3.23, rev `37bfc06a`** (current main)
> with `features = ["shacl"]`, `default-features = false` (onnx still off — verified via `cargo tree -i ort`).
> The upstream SHACL blocker described below is **closed**: the chrono clash was dissolved by bumping
> lancedb 0.17 → 0.27 (arrow 53 → 57, the pairing quipu itself builds against), so write-time SHACL
> validation is now compiled in and `knowledge_knot` reports `shacl_validated: true` honestly
> (`KNOWLEDGE_SHACL_ENABLED`, `src/mcp/server.rs`). The bump also made `replace_snapshot` real on the
> embedded store (the chunk-push probe in `src/knowledge/chunks.rs` passes) and strict `/knot` graph
> routing real (the quarantine routing probe in `src/knowledge/quarantine.rs` passes). The Phase-1 row
> and the chrono/"blocked upstream" text below are kept as history but are superseded by this note.
>
> On `knowledge_validate` (Phase-3 row): it is still **not shipped**, but the reason changed. The
> upstream blocker is gone — validation could now be exposed — and in the meantime every write path
> (`knowledge_knot`, chunk/coupling/quarantine pushes) validates inline and surfaces SHACL refusals as
> errors, so a standalone validate tool is a convenience, not a gap in enforcement. The live MCP
> surface is now five tools: `knowledge_context`, `knowledge_query`, `knowledge_knot`,
> `knowledge_inferred_extract`, `knowledge_reconcile_mentions` (`src/mcp/server.rs`).
>
> **Implementation status (2026-08-17, Claude):** 🟡 **Partial — no longer dark. Two phases genuinely incomplete.**
> Re-measured per phase against the source; supersedes the 2026-07-23 banner below, whose central claim is
> now stale.
>
> | Phase | State | Evidence |
> |---|---|---|
> | 1. Crate dependency | ✅ Built | `Cargo.toml:135`, pinned `=0.2.0` at rev `7f984b4e`; `knowledge = ["dep:quipu"]` at `:145` |
> | 2. Shared embedding pipeline | ✅ **Built** (2026-08-17) | `src/knowledge/embedding.rs` implements `quipu::embedding::EmbeddingProvider` over bobbin's `Embedder`; `open_quipu_store` attaches it on every open. The trait and `Store::set_embedding_provider` already existed upstream — an earlier note here claimed they needed writing, which was wrong and came from grepping bobbin rather than quipu. |
> | 3. MCP tool surface | 🟡 **Three of four** (2026-08-17) | `knowledge_context`, `knowledge_query` and now `knowledge_knot` exist. `knowledge_validate` is **blocked upstream** and deliberately not shipped — see the SHACL note below. |
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
> So GH #56's un-darking gate is closed. Phase 2 and Phase 3's write half landed on 2026-08-17.
>
> **What remains is one upstream blocker, and it is more serious than a missing tool.**
> bobbin depends on quipu with `default-features = false`, and `shacl` is one of quipu's *default*
> features — so write-time SHACL validation was never compiled in. `tool_knot`'s validation is
> `#[cfg(feature = "shacl")]` (quipu `src/mcp/mod.rs:452`), which means **knowledge writes are accepted
> unvalidated**. It cannot simply be switched on:
>
> ```text
> quipu/shacl -> rudof_lib -> shapes_converter   requires chrono ^0.4.42
> lancedb     -> arrow-array 53.4.1              requires chrono >=0.4.34, <0.4.40
> ```
>
> Irreconcilable without moving off arrow 53, and it cannot even be declared as an opt-in cargo feature,
> because a `quipu/shacl` edge in a feature definition makes the resolver pull that tree whether or not
> the feature is enabled. So `knowledge_knot` reports `shacl_validated: false` on every write rather than
> letting a caller assume, and `knowledge_validate` is not shipped at all — a validator that cannot
> validate would be worse than its absence.

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
