# Knowledge-Aware Bundles: Code Entities in the Knowledge Graph

> Bundles are curated knowledge, not index data. They belong in Quipu.

## Thesis

When Bobbin indexes a codebase, it produces ephemeral artifacts: chunks,
embeddings, coupling scores. These can be rebuilt from source at any time.
Bundles are different — they represent human (or agent) decisions about what
belongs together and why. They cannot be rebuilt. They are knowledge.

This design promotes code artifacts to first-class entities in Quipu's
knowledge graph. Files, symbols, sections, and chunks become nodes alongside
infrastructure entities like containers, services, and deployments. Bundles
become named subgraphs — stable, queryable, temporally versioned collections
that span code and operations.

The result: an agent can orient itself by querying the graph. "What relates
to traefik?" returns code files, the container it runs on, the services it
routes to, recent incidents, and the design doc that explains why it was
configured this way. All from one graph traversal.

## Problem

Today's model has three gaps:

1. **Bundles don't belong in an index.** Bobbin's `.bobbin/index.db` and
   LanceDB stores are rebuildable caches. Bundles in `tags.toml` are curated
   knowledge sitting in the wrong layer. If you nuke the index and rebuild,
   bundles survive only because they're in a separate TOML file — not because
   the architecture protects them.

2. **Code and operations are disconnected.** An agent working on traefik
   config sees files and symbols. It has no idea that traefik runs on the
   dns container, routes to grafana and gitea, or that last week's cert
   renewal broke routing for 2 hours. That operational context lives in a
   separate knowledge graph with no links to code.

3. **Agents can't orient top-down.** Without a unified graph, agents must
   build context bottom-up: search for files, read them, infer relationships.
   This is slow, error-prone, and loses cross-cutting knowledge that doesn't
   live in any single file.

## Design

### Entity Model

Bobbin pushes three categories of code entities into Quipu after indexing.
Each uses tree-sitter or markdown parsing to extract structural relationships.

#### Tree-sitter Entities (Source Code)

Tree-sitter ASTs provide structural relationships for free:

```text
CodeModule  (file-level entity, one per source file)
  --defines-->  CodeSymbol  (functions, structs, traits, enums, impls)
  --imports-->  CodeModule  (use/import statements)

CodeSymbol
  --calls-->     CodeSymbol  (call sites within function bodies)
  --returns-->   CodeSymbol  (return type references)
  --contains-->  CodeSymbol  (nested definitions: impl blocks, inner functions)
```

Entity IRIs follow the pattern:
```
bobbin:code/{repo}/{path}              # CodeModule
bobbin:code/{repo}/{path}::{symbol}    # CodeSymbol
```

Cross-repo relationships emerge through **post-index reconciliation**.
When Bobbin indexes a repo, it extracts import specifiers as unresolved
strings (e.g., `use quipu::Store` becomes an edge to the literal
`"quipu::Store"`). These are pushed to Quipu as dangling references.
After any repo sync, a reconciliation pass resolves them:

1. Query all entities with unresolved import edges
2. Parse each import specifier (language-specific: Rust use paths, Python
   module paths, Go import paths, JS/TS specifiers)
3. SPARQL match against `CodeSymbol` entities across all repos:
   ```sparql
   SELECT ?target WHERE {
     ?target a bobbin:CodeSymbol ;
             bobbin:name "Store" ;
             bobbin:definedIn ?mod .
     ?mod    bobbin:repo "quipu" .
   }
   ```
4. Exactly one match → assert resolved `imports` edge between entities
5. Zero matches → leave dangling (target repo not yet indexed)
6. Multiple matches → flag ambiguous, leave unresolved for human review

This is idempotent — rerun after any repo reindex and new cross-repo
links appear. It also handles the bootstrapping case: when repo B hasn't
been indexed yet, dangling refs from repo A sit in Quipu waiting. Once
repo B syncs, the next reconciliation pass resolves them automatically.

An agent working in bobbin can then traverse into quipu's type
definitions through the graph — no special cross-repo syntax needed.

#### Markdown Entities (Documentation)

Markdown structure maps to a heading-based entity hierarchy:

```text
Document  (file-level entity, one per .md file)
  --contains-->  Section      (heading + content until next same/higher heading)
  --has_frontmatter-->  ...   (metadata from YAML frontmatter)

Section
  --contains-->  Section      (subsections via heading depth)
  --contains-->  CodeExample  (fenced code blocks, language-tagged)
  --contains-->  Definition   (structured lists defining terms, configs, API params)

Section
  --documents-->  CodeSymbol   (prose that explains a code entity)
  --references-->  *           (inline links, bead mentions, entity refs)

CodeExample
  --illustrates-->  CodeSymbol  (snippet matches an indexed symbol)
```

The `documents` relationship is the critical bridge between prose and code.
An agent looking at `ContextAssembler` can traverse to the section that
explains *why* it works the way it does. Bobbin's existing bridge mode
(doc-to-source via git blame) can feed these relationships automatically.

Entity IRIs:
```
bobbin:doc/{repo}/{path}               # Document
bobbin:doc/{repo}/{path}#section-slug  # Section
```

#### Operational Entities (Already in Quipu)

The aegis ontology already contains:
```text
LXCContainer, ProxmoxNode, SystemdService, WebApplication,
DatabaseService, Rig, CrewMember, CLI, Directive, DesignDoc
```

With relationships: `runs_on`, `depends_on`, `connects_to`, `managed_by`,
`monitors`, `routes_to`, `owns`, `deployed_on`.

The design adds cross-domain relationships that link code to operations:

```text
CodeModule   --deploys-->      LXCContainer
CodeModule   --configures-->   SystemdService
CodeSymbol   --implements-->   WebApplication
Document     --documents-->    LXCContainer
ServiceEvent --caused_by-->    CodeSymbol  (incident correlation)
```

### Incident-to-Code Correlation

When a service event triple lands in Quipu:
```
WebApplication/traefik  --status-->  degraded  (valid_from: 2026-04-04T15:00)
```

An agent can traverse the graph:
1. `WebApplication/traefik` <--`configures`-- `CodeModule/deploy/traefik.toml`
2. Which `CodeSymbol` entities in that module were modified recently?
3. Cross-reference with `git_commit` episodes from the same time window
4. Surface the code change that likely caused the disruption

This is a graph query, not a search. It follows typed relationships with
temporal constraints. Quipu's bitemporal model means the agent can ask
"what was the state of traefik's code dependencies at 14:55, five minutes
before the incident?"

### Bundle as Named Subgraph

A bundle is a first-class entity in Quipu with edges to its members:

```text
Bundle/traefik-config
  rdf:type          bobbin:Bundle
  rdfs:label        "traefik-config"
  bobbin:description "Traefik reverse proxy configuration and routing"
  bobbin:created_by  aegis:CrewMember/ellie
  prov:wasGeneratedBy aegis:Episode/...

  --contains-->  CodeModule/aegis/deploy/traefik.toml
  --contains-->  CodeSymbol/aegis/src/traefik/config.rs::TraefikRoutes
  --contains-->  Document/aegis/docs/runbooks/traefik.md
  --contains-->  Section/aegis/docs/design/dns-migration.md#traefik-routing
  --includes-->  Bundle/dns-infrastructure   (sub-bundle)
  --depends_on--> Bundle/cert-management     (bundle dependency)
```

Because bundles and infrastructure entities are peers in the same graph,
enrichment is just traversal depth:

| Depth | What you get |
|-------|-------------|
| 0 | Bundle metadata (name, description, creator) |
| 1 | Direct members: code files, symbols, doc sections |
| 2 | Infrastructure: containers, services, other bundles the members relate to |
| 3+ | Operational context: recent episodes, incidents, deployment history |

#### Bundle Capabilities Beyond Context

Bundles as subgraphs unlock capabilities beyond context retrieval:

- **Skill scoping.** A skill like `/deploy` attaches to `Bundle/traefik-infra`.
  An agent has that skill only when its working set intersects the subgraph.

- **Tool authorization.** Destructive tools (restart service, modify config)
  gate on bundle membership. An agent touching `Bundle/dns-config` gets
  access to dns.lan management tools.

- **Role inheritance.** `Bundle/monitoring` carries
  `bobbin:role aegis/crew/sentinel`. Agents working within that subgraph
  inherit sentinel's permissions and operational context.

- **Blast radius.** An agent's working set IS a subgraph. It can see and
  affect entities within the bundle + N hops. Natural sandboxing derived
  from graph topology, not manual ACL configuration.

- **Temporal versioning.** Because Quipu is bitemporal, every bundle state
  is preserved. "What did the traefik bundle contain before the DNS
  migration?" is a `valid_at` query.

### Sync Lifecycle: Bobbin Index to Quipu

After `bobbin index` completes, code entities flow to Quipu:

```text
bobbin index
  ├── Extract chunks via tree-sitter / markdown parser  (existing)
  ├── Generate embeddings via ONNX                       (existing)
  ├── Write to LanceDB + SQLite                          (existing)
  └── [NEW] Push code entities to Quipu
        ├── Diff against previous sync (incremental)
        ├── Assert new/changed entities + relationships
        ├── Retract removed entities (temporal close, not delete)
        └── Validate via SHACL before write
```

#### Incremental Sync

Full re-sync on every index is wasteful. The sync should be incremental:

1. Bobbin tracks a `last_sync_tx` — the Quipu transaction ID of the last push
2. After indexing, compare current chunks against the previous sync state
3. Only push diffs: new entities, changed relationships, retracted files
4. Quipu's retraction model handles deletions gracefully — closed with
   `valid_to`, not erased, so historical queries still work

#### SHACL Validation Gate

Code entities are validated against SHACL shapes before write. This prevents
malformed entities from polluting the graph. Shapes define:

- Required properties for `CodeModule` (path, repo, language)
- Required properties for `CodeSymbol` (name, kind, parent module)
- Cardinality constraints (a symbol has exactly one parent module)
- Allowed relationship types between entity categories

Validation failures are logged but don't block the sync — code entities are
derived data, and a validation failure indicates a shape mismatch to fix,
not corrupt source code.

### Context Assembly v2

The context pipeline gains a knowledge expansion phase:

```text
Current:  seed → pin → coupling → bridge → budget
New:      seed → pin → coupling → bridge → [knowledge] → budget
```

#### Knowledge Expansion Phase

For each seed chunk in the pipeline:

1. Look up the chunk's file/symbol in Quipu (by IRI)
2. If found, traverse N hops to discover related entities
3. Convert knowledge entities to `KnowledgeChunk` items
4. Score by relationship distance (closer = higher relevance)
5. Add to the candidate pool before budget allocation

Budget allocation splits between code and knowledge:

```toml
[knowledge]
context_budget_pct = 15    # Reserve 15% of context budget for knowledge
max_hops = 2               # Traversal depth for entity expansion
```

The budget percentage is configurable. For pure code tasks, agents can
set it to 0. For operational tasks (incident response, deployment), they
can increase it.

#### Unified Output

Context results carry a `source` discriminator:

```rust
pub enum ContextSource {
    Code,           // Bobbin chunk from index
    Knowledge,      // Quipu entity/fact from graph
    Bridged,        // Doc→source via blame provenance
}
```

### Dual-Layer Architecture

Bobbin's index and Quipu's graph are parallel computations over the same
codebase. They answer fundamentally different questions:

- **Quipu** (structure layer): "What entities *relate to* this entity?"
- **Bobbin** (similarity layer): "What code is *similar to* this query?"

Neither subsumes the other. The index is not just a source of entities to
push into the graph — it remains a live computation that enriches graph
queries at runtime. The architecture is dual-layer by design.

```text
┌─────────────────────────────────────────────────┐
│              Agent / Context Query               │
└──────────────────────┬──────────────────────────┘
                       │
          ┌────────────┼────────────┐
          │                         │
   ┌──────┴──────┐          ┌──────┴──────┐
   │   Bobbin    │          │    Quipu    │
   │ Similarity  │◄────────►│  Structure  │
   │   Layer     │  fused   │   Layer     │
   ├─────────────┤ ranking  ├─────────────┤
   │ Embeddings  │          │ EAVT facts  │
   │ Coupling    │          │ SPARQL      │
   │ FTS index   │          │ Subgraphs   │
   │ Feedback    │          │ Provenance  │
   │ Hotspots    │          │ Temporality │
   └─────────────┘          └─────────────┘
```

#### Embedding Ownership: Bobbin is the Authority

Embeddings are derived data — rebuildable from source text at any time.
By the core principle of this design (rebuildable data belongs in the
index layer, durable knowledge belongs in the graph), **all embeddings
live in Bobbin's LanceDB**. Quipu stores no embeddings of its own when
the knowledge feature is enabled.

Quipu delegates vector search to Bobbin via the `EmbeddingProvider` trait
(`Arc<dyn EmbeddingProvider>`), which is already designed for this. Since
quipu is linked as a crate dependency of bobbin (behind the `knowledge`
feature flag), delegation is an in-process function call — no network
hop, no serialization overhead.

```text
Quipu needs vector search:
  → Calls EmbeddingProvider::embed_batch() via Arc<dyn>
  → Bobbin's LanceDB performs ANN search (~1-5ms)
  → Results returned in-process
  → Quipu uses results for graph entry / neighbor ranking
```

This means:
- **Entity sync** pushes only facts and relationships to Quipu — no
  embedding data, keeping transactions lean
- **One LanceDB instance** serves both chunk-level search (Bobbin's
  existing use case) and entity-level search (new knowledge use case)
- **Rebuilding embeddings** doesn't require touching the knowledge graph
- **Quipu's SQLite vector fallback** remains for standalone mode (no
  Bobbin), using brute-force cosine similarity for small deployments

The granularity difference is handled by LanceDB schema:
- Bobbin's `chunks` table: chunk-level embeddings (sub-file granularity)
- Bobbin's `entities` table: entity-level embeddings (CodeModule,
  CodeSymbol, Section, Bundle — one embedding per knowledge graph entity)

Both tables share the same ONNX model (all-MiniLM-L6-v2, 384-dim) and
the same `Embedder` instance.
```

#### Semantic Search as Graph Entry Point

An agent doesn't always start from a named entity. Often it has a natural
language query: "how does context assembly handle coupling expansion?"

Bobbin's vector search finds the top chunks by embedding similarity. Those
chunks map to entities in Quipu. The agent enters the graph from
semantically relevant nodes and traverses outward. **Bobbin is the front
door to the graph.**

```text
"certificate renewal failing"
  → Bobbin vector search → top 5 chunks
  → Map chunks to Quipu entities (by IRI)
  → Traverse: CodeSymbol/renew_cert --configures--> SystemdService/certbot
                                    --depends_on--> LXCContainer/dns
  → Agent has: relevant code + operational topology
```

Without the embedding layer, the agent would need to know the exact entity
name to start traversal. With it, fuzzy natural language queries become
graph entry points.

#### Embedding-Guided Graph Traversal

When expanding from a bundle, Quipu returns ALL neighbors at each hop.
That's potentially hundreds of entities — too many to include in context.

Bobbin's embeddings rank which neighbors are most relevant to the current
query. The graph provides candidate structure; embeddings provide relevance
scoring within that structure.

```text
Expand Bundle/traefik-config at depth 2:
  → Quipu returns 47 entities within 2 hops
  → Bobbin scores each entity's content against the query embedding
  → Top 10 by semantic relevance make it into context
  → Agent gets structurally connected AND semantically relevant results
```

This is more powerful than either layer alone:
- Pure graph traversal returns too many irrelevant neighbors
- Pure semantic search misses structurally important but lexically distant code
- Combined: structure narrows candidates, semantics ranks them

#### Coupling Scores as Weighted Graph Edges

Git co-change coupling is a signal that exists nowhere in source code or
ASTs. Two files might have no import relationship but always change
together — a hidden dependency that only history reveals.

Bobbin pushes coupling scores to Quipu as weighted edges:

```text
CodeModule/src/search/context.rs
  --co_changed_with {score: 0.82}--> CodeModule/src/cli/context.rs
  --co_changed_with {score: 0.45}--> CodeModule/src/search/hybrid.rs
```

These edges enrich graph algorithms:

- **Shortest path** weighted by coupling finds the strongest change
  propagation routes, not just the shortest structural path
- **PageRank** over coupling-weighted edges identifies files that are
  central to change patterns, not just central to the import graph
- **Community detection** on coupling edges discovers natural module
  boundaries that may differ from the directory structure
- **Impact analysis** becomes: structural distance (graph) + coupling
  strength (weighted edges) + semantic similarity (embeddings) — three
  independent signals fused into one prediction

#### Semantic Anomaly Detection

Comparing embedding distance against graph distance reveals structural
gaps and stale relationships:

| Embedding Distance | Graph Distance | Signal |
|-------------------|----------------|--------|
| Close | Far/disconnected | **Missing relationship.** These entities are semantically similar but nobody has linked them. Candidate for `bundle suggest` or auto-discovery. |
| Far | Close (direct edge) | **Stale or wrong relationship.** An edge exists but the content has diverged. Flag for review. |
| Close | Close | Healthy — structure matches semantics. |
| Far | Far | Unrelated — no action needed. |

This gives `bundle suggest` a new signal beyond coupling clusters: "these
entities should probably be in the same bundle because their embeddings
are similar, even though they have no structural relationship yet."

It also enables graph hygiene: periodic scans for anomalies surface
relationships that need human attention.

#### Content-Aware Graph Queries

SPARQL excels at structural queries. Embeddings excel at semantic matching.
The combination enables queries neither can answer alone:

> "Entities structurally related to traefik AND semantically similar to
> 'certificate renewal'"

Query pattern:
1. Quipu: `SELECT ?entity WHERE { aegis:traefik ?rel ?entity }` → structural neighbors
2. Bobbin: score each neighbor's content against "certificate renewal" embedding
3. Return: neighbors ranked by semantic relevance

This is the query pattern for incident correlation. A service event
mentions "certificate renewal failure." The agent queries for traefik's
structural neighbors (code files, configs, services), then ranks them by
semantic similarity to the incident description. The most relevant code
surfaces without the agent knowing which file handles certs.

#### Feedback Loop into Graph Quality

Bobbin's feedback system rates context injections as useful, noise, or
harmful. When knowledge entities from Quipu are included in context:

- **Useful ratings** reinforce the entity and its relationships — increase
  their weight in future context assembly
- **Noise ratings** suggest the entity was included but irrelevant — the
  traversal path that reached it may need pruning or the relationship
  may be too loose
- **Harmful ratings** flag the entity for review — its content may be
  stale, its relationships wrong, or its provenance suspect

Over time, the graph gets better through agent usage. Entities that
consistently produce helpful context gain authority. Entities that produce
noise lose ranking weight. The feedback store in Bobbin (per-file scores)
maps directly to entity quality scores in Quipu.

```text
Bobbin feedback: "src/traefik/config.rs injection was useful"
  → Map to entity: CodeModule/aegis/src/traefik/config.rs
  → Increment entity quality score in Quipu
  → Next traversal: this entity ranks higher as a neighbor
```

### Primitive Synergy Summary

| Bobbin Primitive | Quipu Primitive | Combined Capability |
|-----------------|-----------------|---------------------|
| Vector embeddings | Entity IRIs | Semantic search as graph entry point |
| Embedding scoring | Graph traversal | Relevance-guided neighbor expansion |
| Git coupling scores | Weighted RDF edges | Coupling-aware graph algorithms |
| Embedding distance | Graph distance | Anomaly detection (missing/stale edges) |
| Keyword FTS | SPARQL FILTER | Content-aware structural queries |
| Feedback scores | Entity quality | Usage-driven graph quality improvement |
| Tree-sitter chunking | Episode ingest | AST nodes become graph entities |
| ONNX embedder (384-dim) | Vector storage | Shared `EmbeddingProvider`, single model |
| Bridge mode (blame) | `prov:wasGeneratedBy` | Closed provenance loops |
| Impact analysis | Graph projection | Three-signal impact prediction |
| Hotspot detection | Temporal history | Churn + incident correlation |
| `ContextBundle` shape | `KnowledgeContext` shape | Unified interleaved output |

#### Provenance Loops

Bobbin's bridge mode traces docs to source via git blame. Quipu's episode
provenance traces entities to their source documents. Together they form
closed loops:

```text
CodeFile (in Bobbin index)
  → git blame → CommitEntry → changed DocumentSection
  �� episode provenance → KnowledgeEntity created from that section
  → entity facts → relates back to CodeFile
```

An agent can follow the loop in either direction: from code to the knowledge
that explains it, or from knowledge to the code that implements it.

#### Graph Projection for Impact Analysis

Bobbin's impact analysis currently uses coupling + semantic similarity.
With code entities in Quipu, it gains graph topology as a third signal:

```text
ImpactScore = w1 * coupling_score       (git co-change frequency)
            + w2 * semantic_similarity   (embedding cosine distance)
            + w3 * graph_distance        (shortest path in Quipu)
```

Quipu's `ProjectedGraph` (petgraph DiGraph) enables PageRank over code
entities — identifying structurally central files that changes ripple
through. Combined with coupling weights on edges, this finds impact paths
that pure structural or pure historical analysis would miss.

### Bundle CRUD

Bundle operations go through Bobbin's CLI but write to Quipu:

| Command | Behavior |
|---------|----------|
| `bobbin bundle create` | Creates `Bundle` entity + edges in Quipu |
| `bobbin bundle add` | Asserts new `contains` edges |
| `bobbin bundle remove` | Retracts `contains` edges (temporal close) |
| `bobbin bundle show` | SPARQL query: traverse from bundle, render results |
| `bobbin bundle show --deep` | Deeper traversal (2+ hops), includes infra entities |
| `bobbin bundle suggest` | Coupling clusters + graph topology → suggested subgraphs |
| `bobbin bundle check` | Validate all member IRIs still resolve in index |

#### Bundle Suggest Evolution

Current `bundle suggest` uses git coupling clusters. With Quipu, it adds
a second signal: graph topology.

```text
Coupling signal:  "these files change together frequently"
Graph signal:     "these files' symbols relate to the same service entity"
Combined:         intersection or union, configurable
```

An agent can also create bundles on the fly during work — linking the docs
it read, the code it changed, and the infrastructure entities involved.
The reflective formula's knowledge-capture step becomes: create a bundle
subgraph capturing the session's working set.

### Degraded Mode

When the knowledge feature is disabled (`[knowledge] enabled = false`):

- Bundles fall back to `tags.toml` (flat TOML definitions, no graph)
- Context assembly skips the knowledge phase
- Tag rules, effects, and the tag ontology continue to work
- `bundle suggest` uses coupling only
- No code entities are pushed to Quipu

This is not a migration concern — it's permanent. Some deployments of Bobbin
(open source users without Quipu) will always run in degraded mode. The tag
system and flat bundles remain the non-knowledge path.

### Migration: tags.toml Bundles to Quipu

For deployments enabling knowledge for the first time:

1. `bobbin migrate-bundles` reads existing `[[bundles]]` from tags.toml
2. For each bundle, creates a `Bundle` entity in Quipu with:
   - `contains` edges to `CodeModule`/`CodeSymbol` entities (from `files`/`refs`)
   - `includes` edges to other `Bundle` entities (from `includes`)
   - `depends_on` edges (from `depends_on`)
   - Keywords, description, slug as entity properties
3. Tags and effects remain in tags.toml (search-time config, not knowledge)
4. Bundle definitions are removed from tags.toml after successful migration
5. A `migrated_from` provenance edge links each bundle to its tags.toml origin

### SHACL Shapes for Code Entities

```turtle
@prefix bobbin: <https://bobbin.dev/ontology#> .
@prefix sh:     <http://www.w3.org/ns/shacl#> .

bobbin:CodeModuleShape a sh:NodeShape ;
    sh:targetClass bobbin:CodeModule ;
    sh:property [
        sh:path bobbin:filePath ;
        sh:minCount 1 ; sh:maxCount 1 ;
        sh:datatype xsd:string ;
    ] ;
    sh:property [
        sh:path bobbin:repo ;
        sh:minCount 1 ; sh:maxCount 1 ;
        sh:datatype xsd:string ;
    ] ;
    sh:property [
        sh:path bobbin:language ;
        sh:minCount 1 ; sh:maxCount 1 ;
        sh:datatype xsd:string ;
    ] .

bobbin:CodeSymbolShape a sh:NodeShape ;
    sh:targetClass bobbin:CodeSymbol ;
    sh:property [
        sh:path bobbin:name ;
        sh:minCount 1 ; sh:maxCount 1 ;
        sh:datatype xsd:string ;
    ] ;
    sh:property [
        sh:path bobbin:symbolKind ;
        sh:minCount 1 ; sh:maxCount 1 ;
        sh:in ( "function" "struct" "trait" "enum" "impl" "method"
                "class" "interface" "module" "type" "const" ) ;
    ] ;
    sh:property [
        sh:path bobbin:definedIn ;
        sh:minCount 1 ; sh:maxCount 1 ;
        sh:class bobbin:CodeModule ;
    ] .

bobbin:BundleShape a sh:NodeShape ;
    sh:targetClass bobbin:Bundle ;
    sh:property [
        sh:path rdfs:label ;
        sh:minCount 1 ; sh:maxCount 1 ;
        sh:datatype xsd:string ;
    ] ;
    sh:property [
        sh:path bobbin:description ;
        sh:maxCount 1 ;
        sh:datatype xsd:string ;
    ] ;
    sh:property [
        sh:path bobbin:contains ;
        sh:minCount 1 ;
        sh:or (
            [ sh:class bobbin:CodeModule ]
            [ sh:class bobbin:CodeSymbol ]
            [ sh:class bobbin:Document ]
            [ sh:class bobbin:Section ]
        ) ;
    ] .

bobbin:DocumentShape a sh:NodeShape ;
    sh:targetClass bobbin:Document ;
    sh:property [
        sh:path bobbin:filePath ;
        sh:minCount 1 ; sh:maxCount 1 ;
        sh:datatype xsd:string ;
    ] .

bobbin:SectionShape a sh:NodeShape ;
    sh:targetClass bobbin:Section ;
    sh:property [
        sh:path bobbin:heading ;
        sh:minCount 1 ; sh:maxCount 1 ;
        sh:datatype xsd:string ;
    ] ;
    sh:property [
        sh:path bobbin:headingDepth ;
        sh:minCount 1 ; sh:maxCount 1 ;
        sh:datatype xsd:integer ;
    ] .
```

## Implementation Phases

This design builds on the existing quipu-integration phases:

| Phase | What | Depends On |
|-------|------|-----------|
| **3a** | Register Quipu MCP tools in bobbin serve | bobbin-28n (done) |
| **3b** | Define code entity SHACL shapes | aegis-uito |
| **4a** | Code entity sync (tree-sitter → Quipu) | Phase 2 (shared embedder, qp-xlz) |
| **4b** | Markdown entity sync (sections → Quipu) | Phase 4a |
| **4c** | Unified search (code + knowledge) | bobbin-69s |
| **5a** | Bundle migration (tags.toml → Quipu) | Phase 4a |
| **5b** | Bundle CRUD via Quipu | Phase 5a |
| **5c** | Knowledge expansion in context pipeline | Phase 4c + 5b |
| **5d** | Bundle capabilities (skills, roles, authorization) | Phase 5b |
| **6** | Incident correlation (service events → code) | Phase 4a + operational entities |

## Open Questions

1. **Entity granularity.** Do we push every tree-sitter node, or only
   "interesting" ones (functions, structs, traits)? Inner blocks, local
   variables, and anonymous closures are probably noise. Need a filtering
   heuristic — likely based on `ChunkType` (the set Bobbin already extracts).

2. **Embedding ownership.** Code entity embeddings live in Quipu's vector
   store (for graph-side search) AND Bobbin's LanceDB (for code search).
   Duplication is acceptable (different query patterns), but the ONNX session
   must be shared to avoid double memory cost.

3. **Cross-repo entity resolution.** Decided: post-index reconciliation in
   Quipu. See "Tree-sitter Entities" section. Remaining question: how to
   handle language-specific import resolution (Rust use paths vs Python
   dotted modules vs Go import paths). Likely needs a pluggable resolver.

4. **Bundle ownership model.** Who can modify a bundle? Options:
   (a) any agent (current tags.toml behavior), (b) creator + designated
   roles, (c) governed by the bundle's own role assignment. This matters
   for the authorization use case.

5. **Feedback loop.** When an agent rates a knowledge injection as
   noise/harmful via Bobbin's feedback system, should that feed back into
   Quipu entity quality scores? Could downweight entities that consistently
   produce unhelpful context.
