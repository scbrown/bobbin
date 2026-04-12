# Bobbin Knowledge Integration: Embedding Quipu Web Components

> Bobbin annotates. Quipu renders. The bridge is hyperlinks and web standards.

## Status

- **Author**: crew/stryder (Dearing)
- **Date**: 2026-04-12
- **Revision**: v2 — supersedes polecat chrome's KnowledgeViewSet design
- **Status**: Design (approved direction)
- **Depends on**: Quipu standalone UI (qp-0ae, done), quipu-components.js (shipped)
- **Ref**: `quipu/docs/design/quipu-ui.md` (authoritative Quipu UI architecture)

## Why v2

The original micro-ui.md (2026-04-05) had Bobbin reimplementing Quipu's visual
surface via a `KnowledgeViewSet` trait, Askama SSR templates, and `/kb/*`
routes. The Quipu UI design doc (same date) explicitly rejected this approach:

1. **Bobbin becomes a knowledge graph UI toolkit** — not its job
2. **Quipu loses its visual identity** — every element is a Bobbin template
3. **Tight coupling at the wrong layer** — `ViewEntity`, `render_entity_card()`
   is CRUD-level integration, not delegation

Quipu has since shipped its own UI stack:

- 5 web components (`<quipu-graph>`, `<quipu-entity>`, `<quipu-sparql>`,
  `<quipu-timeline>`, `<quipu-schema>`) in `ui/quipu-components.js`
- Leptos WASM standalone app with Sigma.js + Graphology graph rendering
- Content-negotiated entity URLs (`/entity/{iri}` → HTML / JSON-LD / Turtle)
- Spotlight API (`POST /spotlight` — text → entity annotations)
- Triple Pattern Fragments (`GET /fragments` — incremental graph fetching)

Bobbin's job is to **host** these components, not reimplement them.

## What Was Removed from v1

| v1 Concept | Disposition |
|------------|-------------|
| `KnowledgeViewSet` trait | **Dropped** — Quipu owns all knowledge rendering |
| `src/ui/` module (views.rs, layout.rs, primitives.rs) | **Dropped** — no Bobbin-side rendering |
| `ViewEntity`, `ViewEdge`, `ViewSearchHit` types | **Dropped** — Quipu defines its own types |
| `/kb/*` route group | **Dropped** — Bobbin embeds web components in existing SPA |
| Askama templates (layout.html, kb/*.html) | **Dropped** — no SSR for knowledge views |
| `QuipuViewSet` impl | **Dropped** — no rendering trait to implement |
| Cytoscape.js graph widgets | **Dropped** — Quipu uses Sigma.js + Graphology |

## What Stays from v1

- CSS design system sharing (custom properties: `--bg`, `--surface`, `--accent`)
- Progressive enhancement (existing SPA keeps working, knowledge is additive)
- Feature-gated integration (`knowledge` feature flag)
- Dark, monospace, terminal-aesthetic

## Architecture

```text
┌─────────────────────────────────────────────────────┐
│                    User's Browser                    │
│                                                     │
│  ┌──────────────────────────────────────────────┐   │
│  │ Bobbin SPA (ui.html)                         │   │
│  │                                              │   │
│  │  ┌─ Code Results ─────────────────────────┐  │   │
│  │  │  file.rs:42 — HybridSearch  [koror ↗]  │  │   │
│  │  │  file.go:10 — parseConfig   [dns ↗]    │  │   │
│  │  └────────────────────────────────────────┘  │   │
│  │                                              │   │
│  │  ┌─ Knowledge Panel ─────────────────────┐   │   │
│  │  │  <quipu-graph endpoint="..." query="…">│  │   │
│  │  │  << Quipu renders this entire area >>  │  │   │
│  │  └────────────────────────────────────────┘  │   │
│  │                                              │   │
│  │  <script type="application/ld+json">…</script>  │
│  └──────────────────────────────────────────────┘   │
│         │ REST                    │ REST/SPARQL      │
│         ▼                        ▼                   │
│   ┌───────────┐           ┌──────────────┐          │
│   │ Bobbin    │  crate    │ Quipu        │          │
│   │ Server    │◄─────────►│ Server       │          │
│   └───────────┘           └──────────────┘          │
└─────────────────────────────────────────────────────┘
```

## Integration Levels

Three levels, from simplest to richest. Each is independently useful.

### Level 1: Entity Links + Spotlight Annotations

Bobbin search results that match knowledge entities get clickable badges
linking to Quipu's standalone UI.

**How it works**:

1. When a search query arrives, Bobbin calls `POST quipu.svc/spotlight`
   with the query text
2. Quipu returns entity annotations with confidence scores
3. Bobbin attaches entity badges to matching search results
4. Badges are hyperlinks to `quipu.svc/entity/{iri}` (content-negotiated)

```text
search_result.rs:42  — HybridSearch trait definition
  📊 koror (ProxmoxNode)  →  http://quipu.svc/entity/aegis:koror
```

**Implementation**:

- New optional field in search response: `spotlight_annotations: Vec<SpotlightHit>`
- `SpotlightHit`: `{ surface: String, iri: String, entity_type: String, confidence: f32 }`
- Feature-gated behind `knowledge`
- SPA renders badges with links when annotations present

**Bobbin server change**: ~30 lines in search handler to call spotlight API.
**SPA change**: ~20 lines to render entity badges on search results.

### Level 2: Embedded Web Components

Bobbin's SPA loads Quipu's web components and provides a "Knowledge" panel.
Quipu renders itself — Bobbin provides the viewport.

**How it works**:

1. SPA loads `<script src="quipu.svc/quipu-components.js">` (one tag)
2. A new "Knowledge" tab in the SPA navigation shows a knowledge panel
3. The panel contains `<quipu-graph>` with the current search query
4. Clicking entities in the graph navigates within Quipu's widget
5. "Pop out" button opens Quipu's standalone UI in a new tab

```html
<quipu-graph
  endpoint="http://quipu.svc"
  query="SELECT ?s ?p ?o WHERE { ?s ?p ?o . ?s a aegis:SystemdService }"
  height="400px">
</quipu-graph>
```

**Communication**: `postMessage` between Bobbin (host) and Quipu (component).
Bobbin sends: `{ action: "show", query: "dns", context: "search" }`.

**Available components** (from `quipu-components.js`):

| Component | Purpose | Key Attributes |
|-----------|---------|----------------|
| `<quipu-graph>` | Interactive graph explorer | `endpoint`, `query`, `focus`, `depth`, `height` |
| `<quipu-entity>` | Entity detail card | `endpoint`, `iri`, `show-edges`, `show-history` |
| `<quipu-sparql>` | SPARQL workbench | `endpoint`, `query`, `height` |
| `<quipu-timeline>` | Episode timeline | `endpoint`, `from`, `to` |
| `<quipu-schema>` | Schema browser | `endpoint`, `shape` |

**SPA change**: ~80 lines — new tab, script loader, component placement.

### Level 3: JSON-LD Annotations

Bobbin emits `<script type="application/ld+json">` blocks on search result
pages with structured knowledge about displayed code.

```html
<script type="application/ld+json">
{
  "@context": { "@vocab": "https://schema.org/", "quipu": "https://quipu.dev/ontology#" },
  "@type": "SoftwareSourceCode",
  "@id": "https://quipu.svc/entity/parseConfig",
  "name": "parseConfig",
  "quipu:dependsOn": [{"@id": "https://quipu.svc/entity/yamlParser"}]
}
</script>
```

**Why**: Machine-readable annotations with zero visual impact. Browser
extensions, CI tools, and Quipu's own UI can consume them. This is the
Drupal RDF module pattern — declarative mapping, not code.

**Implementation**: Driven by `bobbin-quipu-mapping.toml`.

## Declarative Type Mapping

A config file maps Bobbin's data model to Quipu's ontology, following the
Drupal RDF module pattern:

```toml
# bobbin-quipu-mapping.toml

[quipu]
endpoint = "http://quipu.svc"

[mappings.code_symbol]
bobbin_type = "CodeSymbol"
quipu_type = "aegis:SoftwareComponent"
match_by = "name"

[mappings.code_module]
bobbin_type = "CodeModule"
quipu_type = "aegis:CodeRepository"
match_by = "path"

[annotations]
# Which Quipu relationships to surface on Bobbin search results
show_predicates = ["aegis:dependsOn", "aegis:ownedBy", "aegis:runsOn"]
max_depth = 1
spotlight_confidence = 0.5
```

Changes to either model only require updating the mapping — not code.

## CSS Theme Sharing

Quipu web components inherit the host page's CSS custom properties.
Bobbin's SPA already defines:

```css
:root {
  --bg: #0f1117;      --surface: #1a1d27;   --surface2: #242837;
  --border: #2e3347;  --text: #e2e8f0;      --dim: #8892b0;
  --accent: #60a5fa;  --accent2: #818cf8;   --green: #34d399;
  --amber: #fbbf24;   --red: #f87171;       --cyan: #22d3ee;
  --mono: 'JetBrains Mono', 'Fira Code', monospace;
}
```

Quipu's `quipu-components.js` uses its own dark theme that's visually
compatible. For tighter integration, Quipu components could read the host's
custom properties via `getComputedStyle(document.documentElement)`.

## Implementation Plan

### Phase 1: Spotlight Integration (Level 1)

Add spotlight API call to search handler; render entity badges in SPA.

**Files changed**:
- `src/http/handlers/search.rs` — optional spotlight call (feature-gated)
- `src/http/ui.html` — entity badge rendering in search results
- `Cargo.toml` — no new deps needed (HTTP client already exists)

### Phase 2: Web Component Embedding (Level 2)

Load `quipu-components.js` in SPA; add Knowledge tab with `<quipu-graph>`.

**Files changed**:
- `src/http/ui.html` — script tag, Knowledge tab, component placement
- `src/config.rs` — optional `quipu_endpoint` config field

### Phase 3: JSON-LD + Mapping Config (Level 3)

Emit JSON-LD blocks; add `bobbin-quipu-mapping.toml` support.

**Files changed**:
- `src/http/ui.html` — JSON-LD script block generation
- `src/knowledge/mapping.rs` — mapping config loader (new, ~100 lines)
- `src/knowledge/mod.rs` — re-export mapping module

### Phase 4: Context Visualization

When Bobbin shows "Context Preview", include a `<quipu-graph>` showing
which knowledge entities were injected into agent context.

**Files changed**:
- `src/http/ui.html` — context tab enhancement
- `src/http/handlers/context.rs` — include entity IRIs in response

## File Inventory (New/Changed Files)

```
src/http/handlers/search.rs    # +spotlight call (~30 lines)
src/http/ui.html               # +badges, Knowledge tab, JSON-LD (~100 lines)
src/config.rs                  # +quipu_endpoint field (~5 lines)
src/knowledge/mapping.rs       # NEW: mapping config loader (~100 lines)
src/knowledge/mod.rs           # +mapping module re-export
bobbin-quipu-mapping.toml      # NEW: type mapping config
```

Estimated: ~250 lines of new Rust, ~100 lines of SPA changes. Compare to
v1's ~1200 lines of Rust + ~800 lines of Askama templates.

## Non-Goals

- **Rendering knowledge views** — Quipu owns this via web components
- **SPARQL proxy** — Quipu's workbench talks directly to quipu.svc
- **Graph visualization library** — Quipu chose Sigma.js; Bobbin doesn't care
- **Standalone knowledge UI** — use Quipu's standalone app at quipu.svc
- **Real-time updates** — refresh to see changes
- **Auth** — same as current `bobbin serve` (none for v1)
