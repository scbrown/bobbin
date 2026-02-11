# Polars (Rust)

*No evaluation results yet. Polars tasks (polars-001 through polars-005) are queued for execution.*

## Task Overview

| Task | Difficulty | Description |
|------|-----------|-------------|
| polars-001 | easy | Fix negative slice bug in group-by slicing |
| polars-002 | medium | Fix optimizer panic in right joins with type coercion |
| polars-003 | medium | Fix nested strict casts returning null instead of error |
| polars-004 | medium | Fix sortedness tracking for `concat_str` with multiple inputs |
| polars-005 | medium | Fix inconsistent division-by-zero behavior for literals |

## Project Details

- **Repository**: [pola-rs/polars](https://github.com/pola-rs/polars)
- **Language**: Rust + Python (via maturin)
- **Size**: ~50K LOC, medium codebase
- **Build**: `maturin develop` (debug mode, ~5min)
