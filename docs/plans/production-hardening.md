# Bobbin Production Hardening Plan

## Context

Bobbin is a local-first Rust code context engine (~17k LOC), feature-complete at Phase 3. The build is broken on fresh machines (`protoc` missing), tambour references remain after extraction, and several production hardening gaps need addressing. This plan creates a series of independently committable work items.

---

## Bead 1: Install protoc + `just setup` recipe (P0 — build blocker)

`protoc` is required by `lance-encoding` (transitive dep via lancedb). Not installed on this system.

- Install `protoc` on this machine now (`sudo apt-get install protobuf-compiler`)
- Add `setup` recipe to `justfile` that installs system deps (protoc, verifies rust toolchain)
- Make recipe idempotent and cross-platform (Linux apt / macOS brew)
- Add `just setup` to CONTRIBUTING.md prerequisites

**Files**: `justfile`, `CONTRIBUTING.md`

---

## Bead 2: Clean up all tambour references (P0 — housekeeping)

Tambour was extracted. 16 files still reference it.

**Delete**:
- `tambour.just`, `scripts/` (all 5 scripts), `docs/tambour.md`, `docs/tambour-metrics.md`, `.tambour/`

**Edit**:
- `justfile`: remove `mod tambour 'tambour.just'`
- `.gitignore`: remove `.tambour/`, `.tambour-source`, and Python patterns (no longer needed)
- `Cargo.toml` exclude list: remove `".tambour/"`, `"tambour.just"`, `"scripts/"`, `"docs/tambour-metrics.md"`

**Files**: justfile, .gitignore, Cargo.toml, plus deletions above

---

## Bead 3: Fix production unwrap() calls (P1 — crash prevention)

~15 unwrap() calls in production code can panic. Test code unwraps are fine.

**Critical fixes**:
- `src/index/embedder.rs:135,143` — `.next().unwrap()` on batch results → proper `Result` with context
- `src/index/git.rs:169` — `partial_cmp().unwrap()` on f32 sort → `.unwrap_or(Ordering::Equal)`
- `src/storage/lance.rs:48` — `to_str().unwrap()` on Path → `.context("non-UTF8 path")?`
- `src/storage/lance.rs:860-867,1069-1074` — column downcast unwraps → extract helper function returning `Result`
- `src/analysis/impact.rs:319,331` — iterator `.next().unwrap()` → `.expect()` with guard context or pattern match

**Pattern**: Follow the `.unwrap_or(Ordering::Equal)` approach already used in `src/mcp/server.rs:867`.

**Files**: `src/index/embedder.rs`, `src/index/git.rs`, `src/storage/lance.rs`, `src/analysis/impact.rs`

---

## Bead 4: Integration test foundation (P1 — testing)

No integration tests exist. Dev deps (`tempfile`, `assert_cmd`, `predicates`) are declared but unused.

- Create `tests/cli_smoke.rs` — CLI smoke tests (`--help`, `--version`, `init`, `status`, `index`, `search`, `grep`)
- Create `tests/index_roundtrip.rs` — end-to-end init → index → search → verify results
- Create `tests/fixtures/` — small multi-language test files (Rust, Python, TypeScript)
- Add `test-integration` recipe to justfile: `cargo test --test '*'`

**Files**: `tests/cli_smoke.rs`, `tests/index_roundtrip.rs`, `tests/fixtures/`, `justfile`

---

## Bead 5: Add missing MCP tools (P2 — feature parity)

MCP server has 8 tools; 3 CLI commands missing from MCP: `deps`, `history`, `status`.

- **`deps`** tool: file import dependencies. Reuse `MetadataStore::get_dependencies()` from `src/storage/sqlite.rs`
- **`history`** tool: git commit history. Reuse `GitAnalyzer::get_file_history()` from `src/index/git.rs`
- **`status`** tool: index statistics. Already partially exists as `get_stats_json()` resource

**Files**: `src/mcp/server.rs`, `src/mcp/tools.rs`

---

## Bead 6: Add missing HTTP endpoints (P2 — feature parity)

HTTP API has 4 endpoints; missing 8 for parity with CLI/MCP.

Add: `GET /grep`, `GET /context`, `GET /related`, `GET /refs`, `GET /symbols`, `GET /hotspots`, `GET /deps`, `GET /history`

Follow existing `search` handler pattern: parse query params → open stores → execute → return JSON.

**Files**: `src/http/handlers.rs`, `src/http/client.rs`

---

## Bead 7: Wire up incremental indexing (P2 — performance)

3 functions in `src/index/git.rs` marked `#[allow(dead_code)]` TODO(bobbin-6vq):
- `get_commit_files()`, `get_changed_files()`, `get_head_commit()`

- Store last-indexed commit hash in metadata store
- On `bobbin index`: if last-indexed commit exists, use `get_changed_files()` for delta
- After indexing, update stored commit hash
- Remove `#[allow(dead_code)]` annotations

**Files**: `src/index/git.rs`, `src/cli/index.rs`, `src/storage/sqlite.rs`

---

## Bead 8: CI pipeline — GitHub Actions (P2 — quality gate)

No CI exists. Create `.github/workflows/ci.yml`:
- Install protoc, rust stable
- `cargo check`, `cargo clippy -- -D warnings`, `cargo test`, `cargo fmt -- --check`
- Add `fmt` recipe to justfile

**Files**: `.github/workflows/ci.yml`, `justfile`

---

## Bead 9: Update AGENTS.md and CONTRIBUTING.md (P3 — docs)

After tambour cleanup, rewrite docs to reflect direct beads/git workflow:
- AGENTS.md: replace tambour commands with `bd` commands and direct git workflow
- CONTRIBUTING.md: remove Python section, add protoc prereq, add `just setup`

**Depends on**: Beads 1, 2

**Files**: `AGENTS.md`, `CONTRIBUTING.md`

---

## Execution Order

```
Phase 1 (blockers):    Bead 1 → Bead 2
Phase 2 (safety):      Bead 3 → Bead 4
Phase 3 (features):    Beads 5, 6, 7 (parallel)
Phase 4 (infra+docs):  Bead 8 → Bead 9
```

## Verification

After all beads:
- `just setup` installs protoc successfully
- `just build` compiles clean
- `just test` passes (unit tests)
- `just test-integration` passes (integration tests)
- `just lint` passes clean
- `grep -ri tambour src/ docs/ justfile` returns nothing
- MCP server exposes deps/history/status tools
- HTTP API has all endpoints responding
- No `#[allow(dead_code)]` in git.rs incremental functions
