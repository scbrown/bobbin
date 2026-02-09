---
title: "Deps & Refs"
description: Exploring import dependencies and symbol references across files
tags: [deps, refs, guide]
status: draft
category: guide
related: [cli/deps.md, cli/refs.md]
commands: [deps, refs]
---

# Deps & Refs

Understanding how code connects — which files import what, where symbols are defined and used — is essential for navigating a codebase and planning changes. Bobbin provides two complementary tools: `deps` for import-level dependencies and `refs` for symbol-level references.

## Import dependencies with `deps`

The `deps` command shows the import graph for a file. During indexing, bobbin extracts `import`, `use`, `require`, and equivalent statements from each file and resolves them to actual file paths on disk.

### Forward dependencies

See what a file imports:

```bash
bobbin deps src/main.rs
```

Output shows each import and where it resolves to:

```text
src/main.rs imports:
  use crate::config::Config    → src/config.rs
  use crate::cli::run          → src/cli/mod.rs
  use crate::storage::LanceStore → src/storage/lance.rs
```

This tells you what `main.rs` depends on — the files that must exist and be correct for `main.rs` to work.

### Reverse dependencies

Find out what depends on a file:

```bash
bobbin deps --reverse src/config.rs
```

Output shows every file that imports from `config.rs`:

```text
Files that import src/config.rs:
  src/main.rs         use crate::config::Config
  src/cli/init.rs     use crate::config::Config
  src/cli/index.rs    use crate::config::Config
  src/cli/search.rs   use crate::config::Config
  src/cli/hook.rs     use crate::config::{Config, HooksConfig}
```

This is the **blast radius** of changing `config.rs`. Every file in this list might need attention if you modify Config's API.

### Both directions

See the full picture at once:

```bash
bobbin deps --both src/search/hybrid.rs
```

## Symbol references with `refs`

While `deps` works at the file level, `refs` works at the symbol level. It finds where specific functions, types, and traits are defined and used.

### Finding a symbol's definition and usages

```bash
bobbin refs find Config
```

Output shows the definition location and every usage:

```text
Definition:
  struct Config — src/config.rs:10-25

Usages (12):
  src/main.rs:5          use crate::config::Config;
  src/cli/init.rs:3      use crate::config::Config;
  src/cli/index.rs:8     let config = Config::load(path)?;
  ...
```

### Filtering by symbol type

When a name is ambiguous, filter by type:

```bash
# Only struct definitions named "Config"
bobbin refs find --type struct Config

# Only functions named "parse"
bobbin refs find --type function parse
```

### Listing symbols in a file

Get an overview of everything defined in a file:

```bash
bobbin refs symbols src/config.rs
```

Output:

```text
src/config.rs (4 symbols):
  struct Config           lines 10-25
  struct HooksConfig      lines 30-48
  impl Default for Config lines 50-70
  fn load                 lines 72-95
```

Use `--verbose` to include signatures:

```bash
bobbin refs symbols --verbose src/config.rs
```

## Practical workflows

### Understanding a module before changing it

Before modifying a module, map its surface area:

```bash
# What does this module expose?
bobbin refs symbols src/search/hybrid.rs

# Who depends on it?
bobbin deps --reverse src/search/hybrid.rs

# What does it depend on?
bobbin deps src/search/hybrid.rs
```

This three-command sequence gives you a complete picture: what the module contains, what it needs, and what would break if you change its API.

### Planning a safe rename

You want to rename a function. First, find every usage:

```bash
bobbin refs find process_batch
```

The output lists every file and line that references `process_batch`. This is your rename checklist.

### Impact analysis

When changing a type's definition, trace the impact:

```bash
# Where is SearchResult defined?
bobbin refs find --type struct SearchResult

# What files import the module containing it?
bobbin deps --reverse src/types.rs
```

Combine forward deps (what `SearchResult` depends on) with reverse deps (what depends on `SearchResult`) to understand the full impact chain.

### Navigating unfamiliar code

You're looking at a function that calls `store.upsert_chunks()`. Where is that defined?

```bash
bobbin refs find upsert_chunks
```

Bobbin shows you the definition file and line number. No IDE required.

### Finding dead code candidates

List all symbols in a file, then check each for usages:

```bash
bobbin refs symbols src/utils.rs
bobbin refs find helper_function_a
bobbin refs find helper_function_b
```

If a symbol has a definition but zero usages, it may be dead code worth removing.

### JSON output for scripting

Both commands support `--json`:

```bash
# Get all dependents as a JSON array
bobbin deps --reverse --json src/config.rs | jq '.dependents[].source_file'

# Get all usages of a symbol
bobbin refs find --json Config | jq '.usages[].file_path'
```

## Deps vs refs vs related

These three tools answer different questions:

| Tool | Question | Level |
|------|----------|-------|
| `deps` | What does this file import / what imports it? | File (import graph) |
| `refs` | Where is this symbol defined / used? | Symbol (name resolution) |
| `related` | What files change alongside this one? | File (git history) |

Use `deps` when you care about the build-time dependency graph. Use `refs` when you care about a specific identifier. Use `related` when you care about change-time coupling that may not follow the import graph.

## Next steps

- [Git Coupling](git-coupling.md) — temporal relationships beyond the import graph
- [Hotspots](hotspots.md) — find the most critical files to understand
- [`deps` CLI reference](../cli/deps.md) — full flag reference
- [`refs` CLI reference](../cli/refs.md) — full flag reference
