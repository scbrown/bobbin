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

During `bobbin index`, if `[git].coupling_enabled` is true (the default), bobbin walks the last N commits (controlled by `coupling_depth`, default 1000) and records which files change together. The coupling score between two files is:

```text
score = co_changes / min(changes_a, changes_b)
```

Where `co_changes` is the number of commits touching both files, and `changes_a`/`changes_b` are their individual commit counts. A pair needs at least `coupling_threshold` (default 3) co-changes to be stored.

## Finding related files

The `bobbin related` command shows temporal coupling for a specific file:

```bash
bobbin related src/auth.rs
```

Output lists files ranked by coupling score, highest first:

```text
src/session.rs         0.82  (shared 41 of 50 commits)
src/middleware/auth.rs 0.64  (shared 32 of 50 commits)
tests/auth_test.rs     0.58  (shared 29 of 50 commits)
src/config.rs          0.24  (shared 12 of 50 commits)
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
coupling_depth = 1000

# Minimum co-changes to establish a coupling link
coupling_threshold = 3
```

**`coupling_depth`** — for large repositories, you may want to increase this to capture longer-term patterns. For small repos, the default of 1000 is usually enough to cover the full history.

**`coupling_threshold`** — raising this eliminates noise from files that coincidentally appeared in a few commits together. The default of 3 is a reasonable minimum.

## Limitations

- **Squashed merges** lose information. If your workflow squashes feature branches into single commits, coupling analysis sees fewer data points.
- **Large commits** (renames, formatting changes, dependency updates) can create false coupling. Bobbin doesn't currently filter these out.
- **New files** have no history. Until a file has been modified in several commits, it won't show meaningful coupling data.

## Next steps

- [Hotspots](hotspots.md) — combine coupling insights with complexity and churn analysis
- [Context Assembly](context-assembly.md) — coupling-aware context bundles
- [`related` CLI reference](../cli/related.md) — full flag reference
- [Configuration Reference](../config/reference.md) — `[git]` settings
