# Task: Add exclude patterns to Cargo.toml

## Summary

Add exclude patterns to prevent internal files from being packaged in the crate.

## File

`Cargo.toml` (modify)

## Implementation

Add to `[package]` section:

```toml
exclude = [
    ".beads/",
    ".tambour/",
    ".runtime/",
    ".claude/",
    ".logs/",
    "scripts/",
    "docs/tasks/",
    "docs/dev-log.md",
    "docs/tambour-metrics.md",
    "mail/",
    "state.json",
    "AGENTS.md",
    "GEMINI.md",
    "PRD.md",
    "VISION.md",
    "justfile",
    "tambour.just",
    "Local RAG and Context Injection.md",
]
```

## Verification

Run `cargo package --list --allow-dirty` and verify:
- No `.beads/` files
- No `.tambour/` files
- No `scripts/` files
- No internal docs (PRD.md, VISION.md, AGENTS.md, etc.)
- DOES include: src/, Cargo.toml, Cargo.lock, README.md, LICENSE, CONTRIBUTING.md

## Acceptance Criteria

- [ ] exclude list in Cargo.toml
- [ ] `cargo package --list` shows only distributable files
- [ ] Package size is reasonable (no 1.7MB research docs)
