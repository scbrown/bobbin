# Micro-UI Framework: Pluggable Knowledge Views for Quipu Integration

> Bobbin already has eyes — this design gives it knowledge vision.

## Status

- **Author**: polecat chrome (bobbin-fal)
- **Date**: 2026-04-05
- **Status**: Design (awaiting keeper review)
- **Depends on**: Quipu integration phases 1-3 (crate dependency, shared
  embeddings, MCP tool surface)

## Problem

Bobbin's existing web UI (`src/http/ui.html`, ~1500 lines of embedded vanilla JS)
renders code search results, bundles, tags, context assembly, and beads — but has
zero knowledge graph visibility. When Quipu integration lands, users will have
entity embeddings, knowledge graph edges, SHACL validation status, and temporal
entity history — all invisible in the current UI.

The gap is architectural, not cosmetic. Bobbin's UI is a monolithic HTML file with
no extension points. Adding knowledge views requires either:
1. Bloating `ui.html` with Quipu-specific rendering (brittle, couples systems), or
2. A plugin architecture where knowledge backends register view components

This design chooses (2).

## Design Goals

1. **Pluggable knowledge views** — Quipu (or any future knowledge backend) registers
   renderers via a Rust trait. Bobbin's UI dispatches to them without knowing
   implementation details.
2. **Server-side rendered** — Askama templates, htmx for interactivity. No JS build
   step. Consistent with Bobbin's zero-dependency-frontend philosophy.
3. **Composable micro-components** — Small, reusable UI primitives (entity cards,
   edge lists, mini-graphs) that work in multiple page contexts.
4. **Progressive enhancement** — The existing `ui.html` SPA continues to work.
   Knowledge views are new routes/tabs that coexist with the current UI. No
   breaking changes.
5. **Dark, monospace, terminal-aesthetic** — Matches the existing design system
   (CSS variables: `--bg`, `--surface`, `--accent`, `--mono`).

## Architecture

### Overview

```text
                  ┌──────────────────────────┐
                  │     bobbin serve --ui     │
                  │        (Axum 0.8)         │
                  └─────────┬────────────────┘
                            │
            ┌───────────────┼───────────────┐
            │               │               │
      ┌─────┴─────┐  ┌─────┴─────┐  ┌──────┴──────┐
      │ Existing   │  │ Knowledge │  │  JSON API   │
      │ SPA (/)    │  │ Views     │  │ (existing)  │
      │ ui.html    │  │ /kb/*     │  │ /search etc │
      └───────────┘  └─────┬─────┘  └─────────────┘
                           │
                  ┌────────┴────────┐
                  │ KnowledgeViewSet│ (trait)
                  └────────┬────────┘
                           │
                  ┌────────┴────────┐
                  │ QuipuViewSet    │ (impl)
                  │ (feature-gated) │
                  └─────────────────┘
```

### The `KnowledgeViewSet` Trait

The core abstraction. Each knowledge backend implements this trait to provide
HTML fragments for entities, edges, search results, and graph neighborhoods.

```rust
// src/ui/views.rs

use axum::response::Html;

/// A knowledge entity for rendering.
/// Decoupled from any specific backend's internal types.
pub struct ViewEntity {
    pub iri: String,
    pub label: String,
    pub entity_type: String,
    pub properties: Vec<(String, String)>,
    pub validation_status: Option<ValidationBadge>,
}

pub enum ValidationBadge {
    Valid,
    Warnings(u32),
    Errors(u32),
}

pub struct ViewEdge {
    pub predicate: String,
    pub target_iri: String,
    pub target_label: String,
    pub target_type: String,
}

pub struct ViewSearchHit {
    pub entity: ViewEntity,
    pub score: f32,
    pub snippet: Option<String>,
}

/// Trait for pluggable knowledge view rendering.
///
/// Implementors provide HTML fragments. Bobbin handles layout, headers,
/// navigation, and CSS — the knowledge backend only renders content.
pub trait KnowledgeViewSet: Send + Sync {
    /// Render a single entity as a card (inline in search results, bundle views, etc.)
    fn render_entity_card(&self, entity: &ViewEntity) -> String;

    /// Render an entity's full detail page (linked from cards)
    fn render_entity_detail(&self, iri: &str) -> Option<String>;

    /// Render a list of edges from/to an entity
    fn render_edge_list(&self, iri: &str) -> Option<String>;

    /// Render a 1-hop graph neighborhood as an interactive widget
    /// Returns HTML + inline JS for Cytoscape.js rendering
    fn render_mini_graph(&self, iri: &str, max_hops: u32) -> Option<String>;

    /// Render search results with knowledge annotations
    fn render_search_results(&self, hits: &[ViewSearchHit]) -> String;

    /// Render a temporal sparkline for entity history
    fn render_timeline(&self, iri: &str) -> Option<String>;

    /// Provider name (shown in UI badges: "quipu", "custom", etc.)
    fn provider_name(&self) -> &str;
}
```

### Why a Trait, Not Templates Alone

The alternative is Askama templates that Bobbin owns, rendering Quipu data
directly. The trait approach is better because:

1. **Quipu can evolve its rendering independently** — New entity types, new
   visualization modes, new validation details don't require Bobbin changes.
2. **Multiple backends** — If someone writes a different knowledge backend
   (Neo4j, TypeDB, plain RDF), they implement the same trait.
3. **Testable** — Mock implementations for UI tests without Quipu dependency.

The cost is an extra layer of indirection. Worth it for a feature-gated
optional dependency.

### Route Structure

New routes under `/kb/` prefix, mounted only when a `KnowledgeViewSet` is
registered:

```
GET /kb/                         # Knowledge dashboard (entity stats, recent, graph overview)
GET /kb/entity/{iri}             # Entity detail page
GET /kb/entity/{iri}/graph       # Mini-graph widget (htmx partial)
GET /kb/entity/{iri}/edges       # Edge list (htmx partial)
GET /kb/entity/{iri}/timeline    # Temporal sparkline (htmx partial)
GET /kb/search?q=...             # Knowledge-only search
GET /kb/bundle/{name}            # Bundle's knowledge subgraph
```

These return full HTML pages (with Bobbin's layout wrapper) or HTML fragments
(for htmx requests, detected via `HX-Request` header).

### Integration with Existing UI

The existing SPA at `/` gets a new "Knowledge" tab in the navbar, visible only
when the knowledge feature is enabled. This tab either:
- Navigates to `/kb/` (full page), or
- Loads `/kb/` content via htmx into the SPA's main area

The simpler approach (full page navigation to `/kb/`) is recommended for v1.
The SPA and knowledge views share the same CSS design system but don't need to
be the same rendering paradigm.

Existing search results gain an "entity" badge when a result matches a
knowledge entity. Clicking the badge navigates to `/kb/entity/{iri}`.

### Existing Endpoints: Knowledge Annotations

Enhance existing JSON endpoints with optional knowledge data (additive,
non-breaking):

```
GET /search?q=...&include_knowledge=true
  → SearchResponse gains optional `knowledge_entities: Vec<EntityAnnotation>`

GET /bundles/{name}?include_knowledge=true
  → BundleDetailResponse gains optional `knowledge_subgraph: SubgraphSummary`

GET /context?q=...&include_knowledge=true
  → ContextResponse gains optional `knowledge_context: Vec<KnowledgeSnippet>`
```

These are JSON-only extensions. The `/kb/` routes handle HTML rendering.

## Micro-UI Primitives

Small, composable HTML components. Each is an Askama template partial (or a
`fn(&ViewEntity) -> String` in the trait impl).

### 1. Entity Card

Compact card shown inline in search results, bundle views, context previews.

```
┌─────────────────────────────────────────────┐
│ [CodeSymbol]  bobbin::search::HybridSearch  │
│                                        ✓    │
│ Trait for combined semantic + keyword search │
│ repo: bobbin  │  4 edges  │  modified: 3d   │
└─────────────────────────────────────────────┘
```

- Type badge (color-coded: CodeSymbol=cyan, CodeModule=accent, Bundle=green, Section=amber)
- IRI as monospace label
- Validation badge (✓ / ⚠3 / ✗2) if SHACL status available
- One-line description (from `rdfs:comment` or entity text, truncated)
- Metadata footer: repo, edge count, recency

### 2. Edge List

Directional relationship list for an entity's connections.

```
 ── outgoing ──────────────────────────
 defines → HybridSearch::search()     [CodeSymbol]
 imports → lancedb::Table             [CodeModule]
 depends_on → context/pipeline        [Bundle]

 ── incoming ──────────────────────────
 calls ← search_handler              [CodeSymbol]
 tests ← test_hybrid_search          [CodeSymbol]
```

- Grouped by direction (outgoing/incoming)
- Predicate as verb, target as clickable link to `/kb/entity/{iri}`
- Target type badge

### 3. Mini-Graph

Interactive 1-hop neighborhood using Cytoscape.js (loaded from CDN).

```
         ┌──────────┐
         │ imports  │
    ┌────┤ lancedb  ├────┐
    │    └──────────┘    │
    │                    │
┌───┴──────┐      ┌─────┴─────┐
│ HybridSch│──────│ context/  │
│ (focus)  │calls │ pipeline  │
└───┬──────┘      └───────────┘
    │ defines
┌───┴──────┐
│ search() │
└──────────┘
```

- Cytoscape.js widget: ~150 lines of inline JS
- CDN loaded: `<script src="https://unpkg.com/cytoscape@3/dist/cytoscape.min.js">`
- Layout: `cose` (force-directed) for organic feel
- Styles: match Bobbin's dark theme (node colors by type, edge labels)
- Click node → navigate to its entity detail page
- Constrained to 1-hop by default, expandable via "show more" button

### 4. Temporal Sparkline

Entity history as a horizontal timeline bar. Shows when the entity was created,
modified, validated, or linked.

```
 2026-01  ···  02  ···  03  ···  04
  ▪ created    ▪ linked   ▪▪ modified
```

- Pure CSS/SVG, no JS dependency
- Events rendered as dots on a time axis
- Hover shows event detail (date, type, actor)
- Width adapts to container

### 5. Validation Badge

SHACL validation status indicator.

```
✓  Valid         (green, no violations)
⚠ 3 warnings    (amber, minor violations)
✗ 2 errors      (red, conformance failures)
```

- Inline element, used inside entity cards and detail pages
- Links to `/kb/entity/{iri}#validation` for full report

## Implementation Plan

### Phase 1: Foundation (Rust scaffolding)

Create the module structure and trait definition. No rendering yet.

```
src/ui/
  mod.rs           # Module root, KnowledgeViewSet trait
  views.rs         # View types (ViewEntity, ViewEdge, etc.)
  layout.rs        # Shared HTML layout wrapper (Askama)
  primitives.rs    # Entity card, edge list, badge renderers
```

**Dependencies to add:**
- `askama = "0.12"` — Compile-time HTML templates
- No Cytoscape.js dependency (CDN-loaded at runtime)
- No htmx dependency (CDN-loaded: `<script src="https://unpkg.com/htmx.org@2"`)

**Work:**
1. Define `KnowledgeViewSet` trait and view types
2. Create Askama base layout template (shares CSS with existing `ui.html`)
3. Add `/kb/` route group to `src/http/mod.rs` (empty, returns "coming soon"
   if no view set registered)
4. Wire `Option<Arc<dyn KnowledgeViewSet>>` into Axum app state

### Phase 2: Primitives (HTML components)

Implement the 5 micro-UI primitives as Askama templates.

```
templates/
  layout.html          # Base layout with nav, CSS, scripts
  kb/
    dashboard.html     # Knowledge dashboard
    entity_detail.html # Entity detail page
    _card.html         # Entity card partial
    _edge_list.html    # Edge list partial
    _mini_graph.html   # Cytoscape.js widget partial
    _sparkline.html    # Temporal sparkline partial
    _badge.html        # Validation badge partial
    search.html        # Knowledge search results
    bundle_graph.html  # Bundle's knowledge subgraph
```

**Work:**
1. Entity card template + CSS
2. Edge list template
3. Validation badge template
4. Temporal sparkline (CSS/SVG)
5. Mini-graph Cytoscape.js widget template

### Phase 3: Quipu Backend (`QuipuViewSet`)

Feature-gated implementation of `KnowledgeViewSet` for Quipu.

```rust
// src/ui/quipu_views.rs (behind #[cfg(feature = "knowledge")])

pub struct QuipuViewSet {
    store: Arc<quipu::Store>,
}

impl KnowledgeViewSet for QuipuViewSet {
    fn render_entity_card(&self, entity: &ViewEntity) -> String {
        // Query Quipu for additional metadata (edge count, validation)
        // Render via Askama template
    }
    // ... etc
}
```

**Work:**
1. Implement `QuipuViewSet` struct wrapping `Arc<quipu::Store>`
2. Map Quipu entity types to `ViewEntity`
3. SPARQL queries for edge lists and graph neighborhoods
4. SHACL validation status queries
5. Temporal history from Quipu's EAVT log

### Phase 4: Route Wiring

Connect the trait implementations to Axum routes.

**Work:**
1. Implement `/kb/` dashboard route (entity counts, recent entities, type breakdown)
2. Implement `/kb/entity/{iri}` detail route
3. Implement htmx partials for graph, edges, timeline
4. Implement `/kb/search` with knowledge-specific ranking
5. Add "Knowledge" tab to existing SPA navbar
6. Add entity badges to existing search results

### Phase 5: Existing Endpoint Enhancement

Add optional `include_knowledge` parameter to existing JSON endpoints.

**Work:**
1. Extend `SearchResponse` with optional knowledge annotations
2. Extend `BundleDetailResponse` with optional knowledge subgraph summary
3. Extend context assembly to surface knowledge entities used

## Design Decisions

### Q: Should Bobbin own the graph visualization or delegate to Quipu's UI?

**A: Bobbin owns rendering, Quipu provides data via the trait.**

Rationale: Bobbin controls the user's visual experience. Quipu is a library,
not a standalone UI. The `KnowledgeViewSet` trait lets Quipu influence
rendering without owning the chrome. If Quipu develops its own standalone UI
later, it can link to Bobbin's views or vice versa.

### Q: How to handle auth for the serve UI?

**A: None for v1. Same as current `bobbin serve`.**

The current UI has zero auth — it's a local development tool. Knowledge views
inherit this model. If auth becomes necessary (multi-user, remote access), it
should be added to the entire HTTP server, not per-feature.

### Q: Should bundles be editable through the UI?

**A: Read-only for v1. Editing via CLI/MCP tools only.**

Rationale: Bundles live in `tags.toml` (committed to repo). Editing through a
web UI creates a divergence between the UI state and the git-tracked config.
Read-only views are safe and useful. Write support is a future feature that
requires conflict resolution with the on-disk config.

### Q: Why Askama + htmx instead of extending the vanilla JS SPA?

**A: The SPA is already 1500 lines. Knowledge views are complex enough to
warrant server-side rendering.**

The existing SPA works well for tabular data (search results, file lists). But
knowledge views need:
- Graph rendering (Cytoscape.js)
- Deep linking (`/kb/entity/{iri}`)
- Partial updates (expanding graph neighborhoods)

Askama + htmx handles these better than growing the monolithic `ui.html`.
The two approaches coexist — `/` serves the SPA, `/kb/*` serves SSR pages.
They share CSS variables and visual language.

### Q: Why not a full frontend framework (React, Svelte)?

**A: Build step = maintenance burden for a Rust CLI tool.**

Bobbin is `cargo build` and done. Adding npm/node/webpack breaks this. Askama
compiles templates at Rust compile time. htmx is a single CDN script tag.
Cytoscape.js is another CDN script tag. Zero frontend toolchain.

## CSS Design System Sharing

The knowledge views reuse Bobbin's existing CSS custom properties:

```css
:root {
  --bg: #0f1117;      --surface: #1a1d27;   --surface2: #242837;
  --border: #2e3347;  --text: #e2e8f0;      --dim: #8892b0;
  --accent: #60a5fa;  --accent2: #818cf8;   --green: #34d399;
  --amber: #fbbf24;   --red: #f87171;       --cyan: #22d3ee;
  --mono: 'JetBrains Mono', 'Fira Code', monospace;
}
```

Entity type colors:
- `CodeModule` → `var(--accent)` (blue)
- `CodeSymbol` → `var(--cyan)` (teal)
- `Bundle` → `var(--green)` (green)
- `Section` → `var(--amber)` (amber)
- Custom types → `var(--accent2)` (purple)

## File Inventory (New Files)

```
src/ui/mod.rs              # KnowledgeViewSet trait, view types
src/ui/views.rs            # ViewEntity, ViewEdge, etc.
src/ui/layout.rs           # Askama base layout
src/ui/primitives.rs       # Component renderers
src/ui/quipu_views.rs      # QuipuViewSet (feature-gated)
src/ui/routes.rs           # Axum route handlers for /kb/*
templates/layout.html      # Base HTML template
templates/kb/*.html        # Knowledge view templates (8-10 files)
```

Estimated: ~800-1200 lines of Rust, ~500-800 lines of HTML templates.

## Risks

1. **Askama version compatibility** — Askama 0.12 requires specific Rust edition
   and derive macro setup. Verify against Bobbin's MSRV.
2. **Cytoscape.js CDN availability** — Offline usage breaks. Mitigation: bundle
   a minified copy as a fallback, or make graph widget optional.
3. **Quipu API stability** — `QuipuViewSet` depends on Quipu's query API.
   Mitigation: The trait boundary isolates Bobbin from API changes.
4. **Template compilation time** — Askama templates add to `cargo build` time.
   Typically <2s for this scale of templates.

## Non-Goals

- **Full graph editor** — No CRUD for entities/edges via UI. Use CLI/MCP.
- **Real-time updates** — No WebSocket push. Refresh to see changes.
- **Mobile-first** — Responsive but optimized for desktop (developer tool).
- **Theming** — Dark mode only. Matches terminal aesthetic.
- **Internationalization** — English only.
