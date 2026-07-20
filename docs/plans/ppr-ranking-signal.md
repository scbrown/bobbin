# 🧠 Personalized PageRank Ranking Signal

> HippoRAG ranks by graph structure. Bobbin ranks by hybrid similarity and
> *expands* via graph. This plan closes that gap: a Personalized PageRank (PPR)
> signal that lets graph connectivity influence the ranking itself.

**Status:** BUILT IN SOURCE — DARK IN EVERY SHIPPED BINARY · **Origin:** agent (strider) · **Date:** 2026-06-21
**Updated:** 2026-07-15 (strider) · **Tracking:** `bobbin-jdlkh` (P1)
**Provenance:** Derived from comparison of [HippoRAG](https://github.com/OSU-NLP-Group/HippoRAG)
against Bobbin + Quipu, prompted by Stiwi.

> ⚠️ **READ THIS BEFORE TRUSTING ANYTHING BELOW.** This plan was written as a pitch and the code
> was subsequently BUILT and MERGED (`src/search/ppr.rs`, wired at `src/search/context.rs:1074`
> and `:1127`). It has **never run**. `knowledge` is the crate's only cargo feature and it is
> enabled by **no build path at all** — not `release.yml`, not `ci.yml`, not the `justfile`. So
> the whole Quipu integration (MCP surface, coupling exporter, PPR reranking) is compiled out of
> every shipped binary, and CI has never even compiled it.
>
> **Every present-tense claim below describes SOURCE, not any artifact a user has run.** In
> particular the "Quipu export" row's *"Already mirrors coupling edges into Quipu RDF"* is TRUE IN
> SOURCE and FALSE IN EVERY RELEASE — `push_coupling_to_quipu` sits behind
> `#[cfg(feature = "knowledge")]` (`src/cli/index.rs:632`), so the live coupling graph is EMPTY.
> That sentence is what sent people hunting for a broken exporter instead of a dead feature flag.
> See `bobbin-jdlkh` for the mechanism and the fix sequence.

## Motivation

HippoRAG's core trick is neurobiological: seed a knowledge graph with the
entities a query mentions, run **Personalized PageRank** over the entity graph,
and let multi-hop connectivity surface passages that are *structurally* relevant
even when they are not *textually* similar. The graph traversal **is** the
ranking. That is precisely what makes it strong at multi-hop "how does X relate
to Y through Z" questions.

Today Bobbin uses the graph only for **expansion**, never for **ranking**:

- Retrieval ranks via Reciprocal Rank Fusion (RRF) over semantic + keyword hits.
- Git coupling, import dependencies, and symbol edges exist but only *add* coupled
  chunks after the fact — they never re-weight the primary results.

We already own all the raw material for a PPR signal. This plan specifies how to
turn latent graph structure into a ranking input without sacrificing the things
that make Bobbin/Quipu better than HippoRAG (local-first, deterministic, strict,
fast).

## What we have today

### Bobbin retrieval pipeline (insertion points)

| Stage | File | Notes |
|-------|------|-------|
| RRF fusion | `src/search/hybrid.rs:129-191` (`combine_with_recency`) | Semantic + keyword merged by `chunk_id`; post-sort, pre-recency is the clean fusion seam |
| Score-adjust chain | `src/search/context.rs:972-1010` | Tag effects → recency → repo affinity; new multiplier slots in here |
| Score struct | `src/search/types.rs:76-87` (`SearchResult`) | Add `graph_proximity: f32` field |
| Coupling compute | `src/index/git.rs:117-200` | Co-change matrix → `FileCoupling` (SQLite) |
| Coupling expand | `src/search/context.rs:652-714` (`expand_coupling`) | Currently expansion-only |
| Quipu export | `src/knowledge/coupling.rs:21-62` (`push_coupling_to_quipu`) | ⚠️ Mirrors coupling edges into Quipu RDF **in source only** — gated by `#[cfg(feature="knowledge")]` at its sole call site (`src/cli/index.rs:632-635`), and that feature is enabled by no build path, so this has never run and the live graph is EMPTY. See `bobbin-jdlkh`. |
| Knowledge expand | `src/search/context.rs:723-844` (`expand_knowledge`, `#[cfg(feature="knowledge")]`) | Calls `quipu::tool_context` |

**Three latent edge sets, none used in ranking:**

1. **Git temporal coupling** — `coupling` table (SQLite). Co-change frequency.
2. **Import dependencies** — `dependencies` table (LanceDB). File→file imports.
3. **Chunk edges** — `chunk_edges` table (LanceDB). Symbol-level: Implements,
   ImplFor, Tests, Extends.

Together these are an adjacency matrix waiting to happen.

### Quipu graph engine (where PPR belongs)

| Capability | File | Notes |
|------------|------|-------|
| Graph projection | `src/graph.rs:42-132` (`project`) | EAVT → `petgraph::DiGraph<i64,i64>`; type + predicate filters |
| In-degree | `src/graph.rs:135-149` | Trivial centrality |
| SCC / shortest path | `src/graph.rs:152-204` | `kosaraju_scc`, `astar` |
| Impact BFS | `src/impact.rs:79-153` | Depth + predicate filtered entity walk |
| `tool_project` MCP | `src/graph.rs:206-286` | Algorithm dispatch — extend with `"ppr"` |
| petgraph | `Cargo.toml:61` | v0.7 |

**PageRank is explicitly noted-but-unimplemented** in `graph.rs` (comment, line 2).
Quipu is the graph engine; this is its natural home.

## Core idea

Two distinct signals, often conflated — we want **both**, separately:

- **Global PageRank** = *authority*. Query-independent. "This file/entity is
  structurally central." Precompute once per index; cheap at query time.
- **Personalized PageRank** = *query-relevant connectivity*. Seed the random-walk
  restart distribution with the query's top hybrid hits; nodes well-connected to
  the seeds score high. This is the HippoRAG analog.

Fuse the resulting per-node score into ranking as a bounded multiplier alongside
recency and repo-affinity.

## The central decision: which graph, whose engine

Three candidate graphs to run PPR over:

1. **Quipu RDF knowledge graph** — entities + facts. Multi-hop *knowledge*.
2. **Bobbin code graph** — coupling + imports + symbol edges. Multi-hop *code*.
3. **Unified** — push the code graph into Quipu (we already mirror coupling) so
   one store, one engine, one PPR covers both.

### Recommendation

**Build the PPR primitive once in Quipu; consume it from two places.**

- **Engine in Quipu** (`src/graph.rs`): a *pure* function
  `personalized_pagerank(edges, seeds, damping, iters, tol) -> Vec<(node, score)>`.
  No `Store` coupling in the math — takes node IDs + weighted edges + a seed
  distribution. This keeps it reusable and unit-testable, and respects the
  architecture: **Quipu owns graph algorithms, Bobbin owns retrieval fusion.**

- **Consumer A — Knowledge multi-hop (Quipu-native):** expose via `tool_project`
  (`"algorithm": "ppr"`) and wire as a re-ranker in the context pipeline
  (`src/context/mod.rs`) and `tool_unified_search`. Direct HippoRAG analog for
  the knowledge graph. Self-contained, low-risk, ships first.

- **Consumer B — Code multi-hop (Bobbin):** Bobbin builds a unified code graph
  from its three edge sets and calls the same pure PPR function (reused under the
  existing `knowledge` feature), seeded by the top-k hybrid hits, then fuses the
  score into RRF. Without the `knowledge` feature, code-graph PPR is simply
  absent and ranking degrades gracefully to today's behavior.

This avoids a second PPR implementation, keeps the math in the graph engine, and
lets both knowledge and code benefit. We deliberately do **not** require pushing
the full code graph into the RDF store (Option 3) for v1 — it's heavier and
forces the `knowledge` feature for core ranking. Revisit if a unified store
proves valuable.

## Advantages

- **Closes the one capability gap** the HippoRAG comparison surfaced: structural,
  multi-hop relevance as a *ranking* input, not just expansion.
- **Reuses existing assets** — three edge sets already indexed; petgraph already
  a dependency; coupling already mirrored to Quipu.
- **Keeps our differentiators** — stays local, deterministic, no LLM in the
  retrieval loop. PPR is pure linear algebra, fully reproducible (unlike
  HippoRAG's LLM-extracted, noisy graph).
- **Two payoffs from one primitive** — knowledge multi-hop *and* code multi-hop.
- **Tunable & reversible** — a bounded multiplier behind a config flag; weight 0
  = today's ranking. Easy A/B via the eval framework.
- **Authority signal as a freebie** — global PageRank over imports/coupling is a
  useful "central file" prior even before personalization.

## Disadvantages & risks

- **Latency.** Sub-100ms is a Bobbin promise. Query-time PPR over a large graph
  with restart can blow the budget. *Mitigations:* run power-iteration on the
  **local subgraph** around seeds (k-hop induced), cap iterations (~20) with
  early-stop tolerance, precompute global PageRank offline, and gate
  personalization to a bounded neighborhood. Treat <100ms as a hard test, not a
  hope.
- **Graph quality / noise.** Coupling has hubs (lockfiles, `mod.rs`, CI configs)
  that distort walks. *Mitigations:* the existing noise filters
  (`context.rs:1156-1234`), degree-capping / hub down-weighting, and the
  `>50-file commit` skip already in `git.rs`.
- **Cold start.** Sparse history / fresh repos → thin graph → weak signal.
  *Mitigation:* weight the signal by graph density; fall back to RRF-only.
- **Edge heterogeneity.** Coupling (temporal), imports (structural), symbol edges
  (semantic) are different beasts. Naively merged they muddy the walk.
  *Mitigation:* per-edge-type weights, tuned via calibration sweep.
- **Feature-gate coupling.** Code-graph PPR riding the `knowledge` feature means
  two code paths to maintain. Accept for v1; the pure-function design keeps the
  surface small.
- **Evaluation cost.** Hard to prove a win without multi-hop eval cases. We need
  fixtures that are *structurally* but not *textually* related (see open
  questions).

## Performance budget

| Phase | Target | Strategy |
|-------|--------|----------|
| Offline global PageRank | per-index, amortized | Full power-iteration over whole graph at index time; persist scores |
| Query-time PPR | < 30ms of the 100ms budget | Induced k-hop subgraph around seeds; ≤20 iterations; early stop |
| Fusion | negligible | One multiply per result in existing score chain |

## Phased rollout

### Phase 1 — Quipu PPR primitive (foundation)
- Implement pure `personalized_pagerank()` + `pagerank()` in `quipu/src/graph.rs`.
- Power iteration; configurable damping (0.85), max iters, tolerance; seed vector.
- Unit tests using existing fixtures (`graph.rs:289-413`): convergence on
  cyclic + acyclic subgraphs; verify against in-degree baseline ordering.
- Expose `"algorithm": "ppr"` in `tool_project` (`graph.rs:211-286`).
- **No Bobbin changes.** Standalone, testable, mergeable.

### Phase 2 — Quipu knowledge re-ranking
- Wire PPR into `src/context/mod.rs` hybrid path and `tool_unified_search`
  (`context/tools.rs`): seed with text/vector hits, re-rank linked entities.
- Add `rank_by_ppr: bool` to `tool_impact` for "most important downstream".

### Phase 3 — Bobbin code graph + offline global PageRank
- Build a unified in-memory code graph from coupling + dependencies + chunk_edges
  (new `src/search/graph.rs` or `src/index/graph.rs`).
- Compute global PageRank at index time; persist a `page_rank: f32` per
  chunk/file. Add `graph_proximity` to `SearchResult` (`types.rs:76-87`).

### Phase 4 — Bobbin query-time PPR fusion
- Seed PPR with top-k hybrid hits; compute over induced subgraph via the Quipu
  primitive (under `knowledge` feature).
- Fuse as bounded multiplier in the score chain (`context.rs:1000-1010`), behind
  `ppr_weight` config (default 0 → no-op until validated).
- Graceful degradation when feature/graph absent.

### Phase 5 — Evaluation & calibration
- Add multi-hop eval cases (structural-not-textual relatedness).
- Calibration sweep over `ppr_weight` + per-edge-type weights
  (reuse `calibration-sweep-results.md` harness).
- Gate on injection-quality metrics; ship default weight only if it wins.

## Standalone Quipu value: episodes → PageRank

PPR is not a Bobbin-only signal borrowed by Quipu. For Quipu as a standalone
knowledge graph it fulfils a **capability already in the design** (see alignment
below) and turns the episode-accretion model into emergent importance.

### What episodes are

An **episode** (`src/episode/mod.rs`) is the unit of agent-extracted knowledge:

| Field | Meaning |
|-------|---------|
| `name` | Episode identifier |
| `episode_body` | Optional raw text the extraction came from |
| `source` | Provenance actor (e.g. `aegis/ellie`) → `prov:wasGeneratedBy` |
| `group_id` | Partition / namespace (multi-project) |
| `nodes[]` | Entities: `name`, `type`, `description`, `properties{}` |
| `edges[]` | Relationships: `source`, `target`, `relation` |
| `shapes` | Optional SHACL validated at write time |

On ingest: optional **entity resolution** (dedupe against existing entities by
embedding similarity; `strict_mode` rejects near-duplicates) → convert to Turtle
→ **SHACL gate** → write via `ingest_rdf` with a **bitemporal** timestamp and
provenance. Every fact traces back to an episode.

### The episodes that come in (Gas Town / aegis)

- **Operational events** — `node-1-rebuild`: node recovered, edge `rebuilt_on`.
- **Bead work** — task completion: entities = bead, files, services; edges =
  `touches`, `depends_on`, `resolved_by`.
- **Infra observations** — topology: `traefik runsOn node-4`, `forgejo runsOn node-1`.
- **Deploy episodes** — `deploy-v3`: app version bump, `runs_on` host.
- **Code archaeology** — `CodeModule` / `CodeSymbol` entities, import edges.

Each episode is a small subgraph stitched into the growing graph. Over many
episodes, entities **accrete** connectivity — `node-4` referenced by dozens of
deploy/rebuild episodes becomes highly connected.

### How they're queried today

SPARQL (structured), bitemporal time-travel (`valid_at` / `as_of_tx`), vector +
hybrid search, the context pipeline / `unified_search` (seed + link expansion),
impact BFS (+ counterfactual `speculate`), and graph projection (`in_degree`,
SCC, shortest path).

**The gap:** every one of these treats connectivity as flat. Vector search
ignores structure entirely; impact BFS orders by *hops*, not importance; link
expansion keeps neighbors by a crude score. Nothing answers *which entities
matter most* in the accreted episode graph, or *which are most relevant to a
query through multi-hop connection*.

### Where PageRank plays in

Episode accretion means the **link structure itself encodes importance** — and
PageRank reads it:

- **Global PageRank = emergent importance.** No single episode declares `node-4`
  central; it becomes central because dozens of episodes point at it. PageRank
  surfaces that emergent property. Powers a "top entities" panel, node sizing in
  the web UI graph explorer, per-type centrality in the schema inspector.
- **Personalized PageRank = query-relevant multi-hop.** Seed with the entities a
  query matches (vector/text), walk the episode graph, rank neighbors by
  connectivity to the seeds. "traefik issues" surfaces `node-4`, the cert service,
  the bead that last touched it — even when not textually similar.

Feeds existing surfaces: **context pipeline** (rank which expanded links to keep
in budget), **impact** (order blast radius by importance not depth), and an
**episode-aware** delta ("which entities did this episode make more central?")
that ties provenance to importance.

Plus two things only Quipu can do:

- **Temporal PageRank** — run `as_of` past times: "`node-4`'s importance rose after
  the rebuild episodes." Importance *trajectory*, not just a snapshot.
- **Counterfactual PageRank** — via `speculate()`: "if we retire `node-1`, how
  does importance redistribute?"

### Alignment with existing Quipu PageRank ideas

This **fulfils intent already written down** — it is not a bolt-on:

- `docs/design/vision.md` §9 "Graph Projection for Algorithms" explicitly lists
  **PageRank** among target algorithms, and says *"Results write back as triples."*
- `docs/design/vision.md` Projection trait sketch includes
  `fn page_rank(&self, config: PageRankConfig) -> HashMap<NodeId, f64>;`.
- `docs/design/vision.md` "Stolen From" table: *"Graph algorithms (PageRank,
  Louvain, etc.) | Neo4j GDS | **Pure functions over Projection API**."*
- `src/graph.rs:2` module doc names PageRank as an intended algorithm.
- README / CHANGELOG / book all advertise "centrality", but only `in_degree()`
  ships — a documented capability gap.

Two alignments matter for the build:

1. **Pure functions over the Projection API** — exactly the plan's recommended
   shape (`personalized_pagerank(edges, seeds, …)`, no `Store` coupling). The
   architecture proposed here already matches Quipu's stated design philosophy.
2. **Results write back as triples** — a Quipu-native twist HippoRAG lacks. A
   PageRank run can persist `quipu:pageRank` scores as **bitemporal facts**:
   queryable in SPARQL, time-travelable, and usable as inputs to datalog rules.
   PageRank becomes *part of the knowledge graph*, not an ephemeral computation.

## Open questions (for ian / Stiwi)

1. **Scope v1 to knowledge only (Phases 1-2), or push through to code fusion
   (Phases 3-4)?** Knowledge-only is lower-risk and self-contained.
2. **Global vs personalized first?** Global PageRank (authority) is cheaper and
   may deliver most of the value as a simple central-file prior.
3. **Per-edge-type weights** — learn from feedback loop, or hand-tune via sweep?
4. **Eval fixtures** — do we have multi-hop cases today, or must we author them?
   Without them we can't prove the signal earns its latency.

## Pitch

Recommend filing as a P3 pitch bead split along phase boundaries — Phase 1
(Quipu primitive) is independently valuable and unblocks everything else:

```
bd create "PPR ranking signal: Quipu personalized_pagerank primitive" -t task -p P3 -l pitch
bd create "PPR ranking signal: Bobbin code-graph fusion" -t task -p P3 -l pitch
```
