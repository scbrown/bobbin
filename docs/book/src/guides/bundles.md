# Context Bundles

Context bundles are **curated knowledge anchors** — named groups of files, symbols, docs, and keywords that capture a concept or subsystem. They let agents (and humans) bootstrap context for a topic instantly instead of searching from scratch each time.

## Why Bundles?

Without bundles, every new session starts cold: the agent searches, reads files, builds a mental model, then works. With bundles, domain knowledge is captured once and reused:

```bash
# Cold start (slow, token-heavy)
bobbin search "context assembly pipeline"
# ... read 5 files ... search again ... read 3 more ...

# With bundles (instant)
bobbin bundle show context/pipeline --deep
# Full source code for all key files + symbols, ready to work
```

Bundles are particularly valuable for:

- **Handoffs**: reference a bundle so the next session can load context immediately
- **Polecat dispatch**: include `bobbin bundle show <name> --deep` in sling args
- **Feature work**: create a bundle as you explore, then attach it to the bead with `b:<slug>`
- **Onboarding**: `bobbin bundle list` shows the knowledge map of the entire codebase

## Bundle Structure

A bundle contains:

| Field | Purpose | Example |
|-------|---------|---------|
| `name` | Hierarchical identifier | `context/pipeline` |
| `description` | One-line summary | "5-phase assembly: seed → coupling → bridge → filter → budget" |
| `files` | Central source files | `src/search/context.rs` |
| `refs` | Specific symbols (`file::Symbol`) | `src/tags.rs::BundleConfig` |
| `docs` | Related documentation | `docs/designs/context-bundles.md` |
| `keywords` | Trigger terms for search | `bundle, context bundle, b:slug` |
| `includes` | Other bundles to compose | `tags` |

## Depth Levels

Bundles support three levels of detail:

- **L0** — `bobbin bundle list`: tree view of all bundles (names + descriptions)
- **L1** — `bobbin bundle show <name>`: outline with file paths, symbol names, doc paths
- **L2** — `bobbin bundle show <name> --deep`: full source code for all refs and files

Use L0 to orient, L1 to plan, L2 to work.

## Creating Bundles

### From Research

The recommended workflow is to create bundles as you explore code:

```bash
# 1. Search and discover
bobbin search "reranking pipeline"
bobbin refs find RerankerConfig
bobbin related src/search/reranker.rs

# 2. Create the bundle with what you found
bobbin bundle create "search/reranking" --global \
  -d "Score normalization and result reranking" \
  -k "rerank,score,normalize,hybrid search" \
  -f "src/search/reranker.rs" \
  -r "src/search/reranker.rs::RerankerConfig,src/search/reranker.rs::rerank_results" \
  --docs "docs/guides/searching.md"

# 3. Add more as you discover relevant files
bobbin bundle add "search/reranking" --global \
  -f "src/search/scorer.rs" \
  -r "src/search/scorer.rs::normalize_scores"
```

### Using the /bundle Skill

If your environment has the `/bundle` skill, it automates the research-to-bundle pipeline:

```bash
/bundle "context assembly pipeline"
/bundle "reactor alert processing" --save
```

The skill searches broadly, reads key files, finds symbol relationships, and synthesizes a bundle definition.

### Guidelines

- Prefer `file::Symbol` refs over whole files — symbols are more precise
- Keep bundles focused: 5-15 refs is ideal, not 50
- Use hierarchical names: `search/reranking`, `context/pipeline`, `hooks/error`
- Generate keywords from queries that produced the best results
- Check `bobbin bundle list` first to avoid duplicates

## Bundle-Aware Workflow

### Linking Bundles to Beads

Use the `b:<slug>` label convention to connect beads to bundles:

```bash
# Create a bead with bundle reference
bd new -t task "Improve reranking scores" -l b:search/reranking

# When starting work on a bundled bead
bobbin bundle show search/reranking --deep  # Load full context
```

### Handoff Pattern

When handing off work, reference the bundle so the next session bootstraps instantly:

```bash
gt handoff -s "Reranking improvements" -m "Working on bo-xyz. Bundle: search/reranking"
```

The next session runs `bobbin bundle show search/reranking --deep` and has full context.

### Evolving Bundles

Bundles should grow as you learn:

```bash
# Discovered a new relevant file during work
bobbin bundle add "search/reranking" --global -f "src/search/weights.rs"

# Found an important symbol
bobbin bundle add "search/reranking" --global -r "src/search/weights.rs::WeightConfig"

# Remove something that turned out to be irrelevant
bobbin bundle remove "search/reranking" --global -f "src/old_scorer.rs"
```

## Storage

Bundles are stored in `tags.toml` configuration files:

- **Global**: `~/.config/bobbin/tags.toml` (shared across repos, use `--global`)
- **Per-repo**: `.bobbin/tags.toml` (repo-specific)

The `--global` flag is recommended for bundles that span concepts across repos.

## Composing Bundles

Use `includes` to build bundle hierarchies:

```bash
bobbin bundle create "context" --global \
  -d "Assembles relevant code for agent prompts" \
  -f "src/search/context.rs"

bobbin bundle create "context/pipeline" --global \
  -d "5-phase assembly: seed → coupling → bridge → filter → budget" \
  -f "src/search/context.rs,src/search/scorer.rs" \
  -i "tags"  # Include the tags bundle
```

When you `bobbin bundle show context --deep`, included bundles are expanded too.

## Beads Integration

Connect bundles to work items using the `b:<slug>` label convention:

```bash
# Label a bead with its bundle
bd new -t task "Fix bridge budget overflow" -l b:context-pipeline
bd new -t bug "Tag effects not applying" -l b:context-tags

# Reference bundles in bead descriptions
bd new -t task "Add rate limiting" --description "The /context endpoint (b:context-pipeline) needs rate limiting."
```

Any agent reading a bead with a `b:` label can instantly load context:
```bash
bobbin bundle show context-pipeline --deep
```

Track bundle usage across beads:
```bash
bobbin bundle stats              # All bundles
bobbin bundle stats context      # Specific bundle
```

## Bundle Health

Validate that bundle refs still resolve:

```bash
bobbin bundle check              # All bundles
bobbin bundle check context      # Specific bundle
bobbin bundle check --json       # Machine-readable for CI
```

## Auto-Discovery

Find tightly-coupled file clusters that should be bundles:

```bash
bobbin bundle suggest                    # Default: coupling >= 0.3, cluster >= 3
bobbin bundle suggest --threshold 0.5    # Stricter coupling
bobbin bundle suggest --min-size 5       # Larger clusters only
```
