---
title: Git Coupling
description: Understanding temporal coupling, related files, and git-based code relationships
tags: [coupling, related, git, guide]
status: draft
category: guide
related: [cli/related.md, guides/hotspots.md]
commands: [related]
---

# Git Coupling

Code that changes together belongs together — or at least, you need to know about it together. Bobbin's temporal coupling analysis mines your git history to discover which files are linked by shared change patterns, even when they have no import relationship.

## What is temporal coupling?

Temporal coupling measures how often two files appear in the same git commit. If `auth.rs` and `session.rs` are modified in 30 of the same 50 commits, they have a high coupling score. This signal is independent of the language, the import graph, or the directory structure.

This matters because:

- **Hidden dependencies** — files may be logically coupled without any import between them (a schema file and the code that queries it, a test file and its fixtures).
- **Change propagation** — when you modify one file, coupled files are likely candidates for coordinated changes.
- **Code review scope** — temporal coupling reveals what a reviewer should look at beyond the diff.

## How bobbin computes coupling

During `bobbin index`, if `[git].coupling_enabled` is true (the default), bobbin walks the last N commits (controlled by `coupling_depth`, default 5000) and records which files change together. The coupling score between two files blends how often they co-change with how recently they last did:

```text
score = 0.7 * (co_changes / max_co_changes) + 0.3 * recency
recency = 1 / (1 + days_since_last_co_change / 30)
```

Where `co_changes` is the number of commits touching both files, and `max_co_changes` is the largest co-change count seen across all pairs in the repo (so the frequency term is normalized to 0.0–1.0). The `recency` term is ~1.0 for a pair that changed together today and ~0.5 at 30 days, so a hot pair that has gone quiet decays below a pair that still co-changes. A pair needs at least `coupling_threshold` (default 3) co-changes to be stored.

## Finding related files

The `bobbin related` command shows temporal coupling for a specific file:

```bash
bobbin related src/auth.rs
```

Output lists files ranked by coupling score, highest first:

```text
Related to src/auth.rs:
1. src/session.rs (score: 0.82) - Co-changed 41 times
2. src/middleware/auth.rs (score: 0.64) - Co-changed 32 times
3. tests/auth_test.rs (score: 0.58) - Co-changed 29 times
4. src/config.rs (score: 0.24) - Co-changed 12 times
```

### Filtering by strength

Show only strongly coupled files:

```bash
bobbin related src/auth.rs --threshold 0.5
```

This filters out weak coupling signals, showing only files with a score above 0.5.

### Adjusting result count

```bash
bobbin related src/auth.rs --limit 20
```

## Practical workflows

### Before modifying a file

When you're about to make changes to a file, check what else might need updating:

```bash
bobbin related src/database/pool.rs
```

If `pool.rs` is strongly coupled to `migrations.rs` and `connection_config.rs`, those files likely need attention when you change the pool implementation.

### Understanding a module's true boundaries

Directory structure doesn't always reflect logical boundaries. Temporal coupling reveals the *real* modules:

```bash
# Check what's coupled to a core type
bobbin related src/types/user.rs --limit 15
```

You might discover that `user.rs` is tightly coupled to files in three different directories — that's the actual module boundary for "user" functionality.

### Finding missing test coverage

If a source file has high coupling with its test file, that test is actively maintained alongside the implementation. If there's no test file in the coupling results, the tests may be stale or missing:

```bash
bobbin related src/parser.rs | grep test
```

No test files in the output? That's a signal worth investigating.

### Code review preparation

Before reviewing a PR that touches `api/handlers.rs`, check what the author might have missed:

```bash
bobbin related src/api/handlers.rs --threshold 0.3
```

If the coupling analysis shows `api/validators.rs` should usually change alongside handlers but the PR doesn't touch it, that's worth flagging.

## Coupling in context assembly

Temporal coupling is also used by `bobbin context` to expand search results. When you run:

```bash
bobbin context "fix authentication bug" --depth 1
```

Bobbin finds code matching your query, then pulls in temporally coupled files. This is why context assembly often surfaces files you didn't directly search for but will need to understand.

See [Context Assembly](context-assembly.md) for details on how coupling expansion works within the context pipeline.

## Configuration

Control coupling analysis in `.bobbin/config.toml`:

```toml
[git]
# Enable/disable temporal coupling analysis
coupling_enabled = true

# How many commits to analyze (more = more accurate, slower indexing)
coupling_depth = 5000

# Minimum co-changes to establish a coupling link
coupling_threshold = 3
```

**`coupling_depth`** — controls how far back to scan for co-change patterns. The default of 5000 covers most project histories. Set to 0 to scan the full history.

**`coupling_threshold`** — raising this eliminates noise from files that coincidentally appeared in a few commits together. The default of 3 is a reasonable minimum.

## Limitations

- **Squashed merges** lose information. If your workflow squashes feature branches into single commits, coupling analysis sees fewer data points.
- **Merge commits** are excluded (`--no-merges`) since they don't represent real co-changes.
- **Mega-commits** (>50 files) are skipped automatically to prevent false coupling from reformats, renames, and dependency updates.
- **New files** have no history. Until a file has been modified in several commits, it won't show meaningful coupling data.

## Next steps

- [Hotspots](hotspots.md) — combine coupling insights with complexity and churn analysis
- [Context Assembly](context-assembly.md) — coupling-aware context bundles
- [`related` CLI reference](../cli/related.md) — full flag reference
- [Configuration Reference](../config/reference.md) — `[git]` settings
