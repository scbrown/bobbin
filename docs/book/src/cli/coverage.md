---
title: coverage
description: Map test↔source coverage inferred from git coupling
tags: [cli, coverage, coupling]
status: draft
category: cli-reference
related: [cli/related.md, guides/git-coupling.md]
commands: [coverage]
feature: coverage
source_files: [src/cli/coverage.rs, src/index/coverage.rs]
---

# coverage

Map test↔source coverage inferred from git co-change history. A source file
lists the test files that change with it (the tests that likely cover it); a
test file lists the source files it covers.

Coverage is derived at query time from the temporal-coupling table (the same
signal behind [`related`](related.md)), filtered by the file's role. Test files
are detected by path conventions (`test_*`, `*_test.*`, `*_spec.*`, `tests/`,
`spec/`, `__tests__/`, …) — no separate index or coverage instrumentation is
required.

## Usage

```bash
bobbin coverage <FILE> [OPTIONS]
```

## Examples

```bash
bobbin coverage src/auth.rs            # Tests that cover auth.rs
bobbin coverage tests/test_auth.rs     # Sources test_auth.rs covers
bobbin coverage src/auth.rs --threshold 0.5   # Only strong links
```

## Options

| Flag | Short | Description |
|------|-------|-------------|
| `--limit <N>` | `-n` | Maximum results (default: 10) |
| `--threshold <F>` | | Minimum coupling score (default: 0.0) |

## Notes

Coverage links are a heuristic from commit co-change, not execution tracing: a
test and source that always change together are inferred to be linked. Files
with no shared commit history return no links.
