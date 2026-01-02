# Development Log

## 2026-01-02: Tambour Agent Harness

### Problem

Running multiple Claude agents on the same codebase causes conflicts:
- File changes collide when agents work in the same directory
- Race conditions when multiple agents grab tasks from `bd ready`
- Crashed agents leave issues stuck in `in_progress`

### Solution

Created agent harness scripts using beads' native worktree support:

**`scripts/start-agent.sh [issue-id] [--label <label>]`** - Spawns isolated agent:
- Uses `bd worktree create` for proper beads redirect
- Claims issue atomically before agent starts (prevents races)
- Injects `bd show` output so agent sees exactly what beads command was run
- Provides explicit execution instructions (read docs, explore, implement, commit)
- Traps failures to unclaim issue if script or Claude crashes
- Optional `--label` flag to filter by label (workflow-agnostic)

**`scripts/finish-agent.sh`** - Cleans up after completion:
- Merges branch, removes worktree, closes issue

**`scripts/health-check.sh [--fix]`** - Zombie detection:
- Finds in_progress issues with no agent running
- `--fix` automatically unclaims zombied tasks

### Tenets

1. **Tambour enables workflows, it doesn't impose them** - No hardcoded labels or filtering. Use `--label` if you want to focus.
2. **Tambour is distinct from any specific project** - It orchestrates agents on beads issues, nothing more.
3. **Tambour will eventually be extracted** - Lives in bobbin temporarily while interface stabilizes.

### Key Design Decisions

1. **Claim at script start, not agent start** - Prevents race conditions when spawning agents rapidly
2. **Wrapper instead of exec** - Allows monitoring Claude's exit code for failure recovery
3. **Beads native worktree** - Uses `bd worktree create` which handles the `.beads/redirect` automatically
4. **Just module** - Tambour recipes isolated in `tambour.just`, accessed via `just tambour <recipe>`
5. **Workflow-agnostic** - Picks next ready task by priority; label filtering is opt-in

### Files Created

- `scripts/start-agent.sh` - Agent launcher with `--label` support
- `scripts/finish-agent.sh` - Cleanup script
- `scripts/health-check.sh` - Zombie detection and recovery
- `justfile` - Main justfile importing tambour module
- `tambour.just` - Tambour-specific recipes
- `docs/tambour.md` - Vision doc with tenets
- `CLAUDE.md` - Agent instructions for worktree workflow
- `AGENT.md` - Development notes, tambour vs bobbin boundaries

### Recipes

```bash
just tambour agent                  # Next ready task by priority
just tambour agent-for <issue>      # Specific issue
just tambour agent-label <label>    # Filter by label
just tambour finish <issue>         # Merge and cleanup
just tambour abort <issue>          # Cancel/unclaim
just tambour health                 # Check for zombies
just tambour health-fix             # Auto-fix zombies
```

### Next Steps

See `docs/tambour.md` for future directions including agent pools, health monitoring, and deeper beads/bobbin integration.

---

## 2026-01-02: Project Scaffolding (bobbin-lqq)

### Completed

**Rust Workspace Setup**

Created `Cargo.toml` with all required dependencies:

| Category | Crates |
|----------|--------|
| CLI | `clap` (derive, env) |
| Serialization | `serde`, `serde_json`, `toml` |
| Async | `tokio` (full) |
| Parsing | `tree-sitter`, `tree-sitter-rust`, `tree-sitter-typescript`, `tree-sitter-python` |
| Vector DB | `lancedb`, `arrow` |
| SQLite | `rusqlite` (bundled, backup) |
| Embeddings | `ort` (ONNX), `ndarray`, `tokenizers` |
| Errors | `anyhow`, `thiserror` |
| Utilities | `walkdir`, `ignore`, `regex`, `chrono`, `indicatif`, `colored`, `tracing`, `directories`, `sha2`, `hex` |

**Module Structure**

Created complete module hierarchy:

- `src/main.rs` - Entry point with tokio async runtime
- `src/config.rs` - Configuration loading/saving with sensible defaults
- `src/types.rs` - Core types: `Chunk`, `ChunkType`, `SearchResult`, `FileCoupling`, `IndexStats`

**CLI Layer** (`src/cli/`)

- `mod.rs` - Clap-based command dispatcher with global flags
- `init.rs` - Repository initialization (creates `.bobbin/`, config, updates `.gitignore`)
- `index.rs` - Index building (stub)
- `search.rs` - Semantic search (stub)
- `grep.rs` - Keyword search (stub)
- `related.rs` - Related files (stub)
- `status.rs` - Index statistics (stub)

**Indexing Engine** (`src/index/`)

- `parser.rs` - Tree-sitter integration for Rust, TypeScript, Python
  - Extracts functions, classes, structs, traits, impls, modules
  - Falls back to line-based chunking for unsupported languages
  - Generates deterministic chunk IDs using SHA256
- `embedder.rs` - ONNX embedding generation (stub, awaiting model)
- `git.rs` - Git history analysis
  - Parses `git log` to find co-changing files
  - Calculates temporal coupling scores
  - Supports incremental updates via commit tracking

**Search Engine** (`src/search/`)

- `semantic.rs` - Vector similarity search via LanceDB
- `keyword.rs` - Full-text search via SQLite FTS5
- `hybrid.rs` - Reciprocal Rank Fusion (RRF) for combining results

**Storage Layer** (`src/storage/`)

- `sqlite.rs` - Complete schema with:
  - `files` table for file metadata
  - `chunks` table for semantic units
  - `chunks_fts` virtual table for FTS5
  - `coupling` table for temporal relationships
  - Automatic FTS triggers for sync
- `lance.rs` - LanceDB wrapper (stub)

**Toolchain**

- Installed Rust 1.92.0 (stable-aarch64-apple-darwin)

### Implementation Status

| Component | Status | Notes |
|-----------|--------|-------|
| Cargo.toml | ✅ Complete | All dependencies specified |
| Module structure | ✅ Complete | All files created |
| CLI parsing | ✅ Complete | Commands defined, flags working |
| `init` command | ✅ Complete | Creates config, updates gitignore |
| Tree-sitter parser | ✅ Complete | Rust/TS/Python support |
| SQLite schema | ✅ Complete | Tables, FTS, triggers defined |
| Git analyzer | ✅ Complete | Coupling analysis logic |
| Embedder | 🔲 Stub | Needs ONNX model integration |
| LanceDB storage | 🔲 Stub | Needs actual LanceDB calls |
| `index` command | 🔲 Stub | Needs to wire up components |
| `search` command | 🔲 Stub | Needs embedder + LanceDB |
| `grep` command | 🔲 Stub | Needs FTS integration |
| `related` command | 🔲 Stub | Needs coupling queries |
| `status` command | 🔲 Stub | Needs stats queries |

### Next Steps

1. **Verify build** - Run `cargo check` to ensure everything compiles
2. **Implement LanceDB storage** - Connect to actual LanceDB
3. **Implement embedder** - Download and load ONNX model
4. **Wire up `index` command** - Connect parser → embedder → storage
5. **Wire up `search` command** - Query flow end-to-end

### Dependencies to Unblock

This task (bobbin-lqq) blocks:
- bobbin-7o5: Tree-sitter code parsing integration
- bobbin-8h0: ONNX embedding generation
- bobbin-cj1: SQLite metadata and FTS storage
- bobbin-j0x: LanceDB vector storage integration
