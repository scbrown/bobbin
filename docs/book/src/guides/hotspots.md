---
title: "Hotspots"
description: "Finding high-churn, high-complexity code hotspots for refactoring targets"
tags: [hotspots, churn, complexity, guide]
commands: [hotspots]
status: draft
category: guide
---

# Hotspots

Not all code is equally risky. A file that's both complex *and* frequently changed is far more likely to harbor bugs than one that's simple or stable. Bobbin's hotspot analysis identifies these high-risk files by combining git churn with AST-based complexity scoring.

## The hotspot model

A hotspot is a file that scores high on two axes:

- **Churn** — how often the file has been modified in git history. High churn means the code is actively evolving, which increases the chance of introducing defects.
- **Complexity** — how structurally complex the file is, measured by analyzing its AST. Deep nesting, many branches, and large functions all contribute to higher complexity.

The **hotspot score** is the geometric mean of these two signals:

```
score = sqrt(churn_normalized * complexity)
```

A file must score high on *both* to be a hotspot. A simple file that changes constantly, or a complex file that never changes, won't rank high. The intersection is what matters.

## Finding hotspots

```bash
bobbin hotspots
```

This shows the top 20 hotspots from the last year, ranked by score:

```
 Score  Churn  Complexity  File
 0.823    47       0.72    src/cli/hook.rs
 0.756    38       0.68    src/index/parser.rs
 0.691    52       0.41    src/search/hybrid.rs
 0.634    29       0.65    src/config.rs
 ...
```

### Adjusting the time window

Narrow or widen the churn analysis period:

```bash
# Last 3 months — focus on recent activity
bobbin hotspots --since "3 months ago"

# Last 6 months
bobbin hotspots --since "6 months ago"

# All time
bobbin hotspots --since "10 years ago"
```

A shorter window emphasizes *current* hotspots. A longer window surfaces chronic problem files.

### Filtering by score

Show only files above a minimum score:

```bash
bobbin hotspots --threshold 0.5
```

This is useful when you want a short, actionable list of the worst offenders.

### Scoping to a directory

Analyze a specific subsystem:

```bash
bobbin hotspots --path src/search
```

### Result count

```bash
bobbin hotspots --limit 10    # Top 10 only
bobbin hotspots --limit 50    # Broader view
```

## How complexity is measured

Bobbin uses Tree-sitter to parse each file's AST and compute a weighted complexity score in the range [0, 1]. The scoring considers:

- **Nesting depth** — deeply nested code (loops inside conditionals inside match arms) scores higher.
- **Branch count** — if/else chains, match arms, and ternary expressions add complexity.
- **Function size** — longer functions are harder to reason about.
- **Structural density** — how much logic is packed into a given span of code.

Non-code files (Markdown, JSON, YAML, TOML) and unsupported languages are excluded automatically.

**Supported languages**: Rust, TypeScript/JavaScript, Python, Go, Java, C, C++.

## Practical workflows

### Prioritizing refactoring

You have limited time for tech debt. Hotspots tell you where to focus:

```bash
bobbin hotspots --threshold 0.6 -n 10
```

The top 10 files above 0.6 are your highest-impact refactoring targets. Simplifying these files will reduce the most bug-prone, hardest-to-maintain code in your project.

### Sprint planning

At the start of a sprint, check which files in the areas you'll be working on are hotspots:

```bash
bobbin hotspots --path src/api --since "3 months ago"
```

If a hotspot is in your path, consider allocating time to simplify it before adding more features on top.

### Tracking improvements over time

Run hotspot analysis before and after a refactoring effort:

```bash
# Before: snapshot current hotspots
bobbin hotspots --json > hotspots-before.json

# ... do the refactoring work ...

# After: compare
bobbin hotspots --json > hotspots-after.json
```

Use the JSON output to compare scores and verify that your refactoring actually reduced the hotspot score for the targeted files.

### CI integration

Add hotspot analysis to your CI pipeline to catch regressions:

```bash
bobbin hotspots --json --threshold 0.8
```

If any file exceeds 0.8, fail the check or emit a warning. This prevents new code from becoming a hotspot without anyone noticing.

### Combining with file history

For a hotspot that surprises you, dig into its change history:

```bash
bobbin history src/cli/hook.rs
```

The history output shows commit dates, authors, and messages. You'll see *why* the file has high churn — is it active feature development, repeated bug fixes, or configuration changes?

### Verbose output

For a deeper understanding of the scoring:

```bash
bobbin hotspots --verbose
```

Verbose mode includes a legend explaining the scoring methodology and shows both raw and normalized values.

## Next steps

- [Git Coupling](git-coupling.md) — discover which files change together
- [Deps & Refs](deps-refs.md) — understand import chains for hotspot files
- [`hotspots` CLI reference](../cli/hotspots.md) — full flag reference
- [`history` CLI reference](../cli/history.md) — dig into file change history
