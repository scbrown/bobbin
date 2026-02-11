# Plan: File Classification + Git Temporal Awareness (Line-Level Provenance)

## Context

Bobbin's hook injection is dominated by changelog/markdown files (31/37 = 84% across 5 ruff eval tasks). Changelogs score high on semantic similarity because they mention feature names, but they're useless for fixing bugs. Ground truth overlap is ~6%.

**Key insight**: A changelog entry that matches a query was added in a *specific commit* — and that commit also touched the actual source files. By git-blaming the matching chunk's line range, we discover the commit that introduced it, then find the source files changed in that same commit. This is **line-level provenance** — far more precise than coarse file-level coupling (which would relate a changelog to *everything* it ever co-changed with).

**Requirements from user**:
- Use git blame + git log (not coupling table) to bridge docs → source
- Always show both docs AND source code in injection output
- Make the dual-display configurable via flag
- Use section headers in output: "Source Files" and "Documentation"

## Design: Four Changes

### 1. FileCategory enum + classify_file() — `src/types.rs`

Add lightweight path-based file classification:

```rust
pub enum FileCategory { Source, Test, Documentation, Config }

pub fn classify_file(path: &str) -> FileCategory
```

Heuristics (conservative — default is Source):
- **Documentation**: `.md`, `.mdx`, `.rst`, `.txt`; names like `CHANGELOG`, `README`, `BREAKING_CHANGES`; paths containing `/docs/`, `/changelogs/`
- **Test**: paths containing `/test/`, `/tests/`, `/spec/`; names matching `*_test.*`, `*_spec.*`, `test_*.*`; snapshot dirs
- **Config**: `Cargo.toml`, `package.json`, `Makefile`, `.gitignore`, `*.yaml`/`*.yml`, `.github/`
- **Source**: everything else (safe default)

### 2. Git Blame Line-Level Provenance — `src/index/git.rs` + `src/search/context.rs`

New method on `GitAnalyzer`:

```rust
/// Blame a specific line range to find the commits that introduced those lines.
pub fn blame_lines(&self, file_path: &str, start: u32, end: u32) -> Result<Vec<BlameEntry>>

pub struct BlameEntry {
    pub commit_hash: String,
    pub line_number: u32,
}
```

Runs: `git blame -L{start},{end} --porcelain {file_path}` — porcelain format is machine-parseable and gives one commit hash per line.

New method on `ContextAssembler` — `bridge_docs_via_provenance()`:

1. After hybrid search, identify seed results where `category == Documentation`
2. For each doc chunk, call `git_analyzer.blame_lines(file, start_line, end_line)`
3. Deduplicate commit hashes
4. For each unique commit, call `git_analyzer.get_commit_files(hash)` (already exists — runs `git diff-tree --no-commit-id --name-only -r`)
5. Filter returned files to `Source`-category only (via `classify_file()`)
6. Fetch chunks for those source files from vector store
7. Add as bridged results with `FileRelevance::Bridged` and metadata about which doc chunk + commit triggered them

This replaces the coupling-based bridging approach. No changes to the coupling table or `expand_coupling()`.

### 3. Sectioned Output with Headers — `src/cli/hook.rs`

Modify `format_context_for_injection()` to group files by category:

```
Bobbin found N relevant files (M source, K docs):

=== Source Files ===

--- crates/ruff_linter/src/rules/flake8_boolean_trap/helpers.rs:45-89 check_call (function, score 0.87) ---
<content>

--- crates/ruff_linter/src/rules/flake8_boolean_trap/helpers.rs:12-30 is_boolean_arg (function, score 0.75) ---
<content>

=== Documentation ===

--- changelogs/0.14.x.md:120-135 Flake8 Boolean Trap section (section, score 0.92) ---
<content>
```

Source files always listed first. Documentation follows. Both categories always shown (when results exist in each).

### 4. Configurable Flag — `src/cli/hook.rs` + `src/config.rs`

Add `--show-docs` flag to `InjectContextArgs` (default: true):

```rust
/// Include documentation files in injection output (default: true)
#[arg(long, default_value = "true")]
show_docs: bool,
```

Also add to `bobbin.toml` config under `[hooks]`:
```toml
show_docs = true  # Include documentation in hook injection
```

When `show_docs = false`, doc-category files are excluded from output entirely (but still used for provenance bridging to find source files).

## Files to Modify

| File | Changes |
|------|---------|
| `src/types.rs` | Add `FileCategory` enum, `classify_file()` fn, unit tests |
| `src/index/git.rs` | Add `blame_lines()` method + `BlameEntry` struct |
| `src/search/context.rs` | Add `category` field to `SeedResult`, `CoupledChunkInfo`, `ContextFile`. Add `FileRelevance::Bridged`. Add `bridge_docs_via_provenance()` method. Update `ContextSummary` with `bridged_additions`, `source_files`, `doc_files` |
| `src/cli/hook.rs` | Sectioned output formatting. `--show-docs` flag. Updated metrics JSON with `source_files`, `doc_files`, `bridged_additions`. Pass `GitAnalyzer` into context assembler |
| `src/config.rs` | Add `show_docs` to `HooksConfig` |

Reuse existing functions (no changes needed):
- `GitAnalyzer::get_commit_files()` in `src/index/git.rs` — already does `git diff-tree --name-only`
- `VectorStore::get_chunks_for_file()` in `src/storage/lance.rs` — fetch chunks for bridged source files
- `Command::new("git")` pattern used throughout `git.rs`

## Implementation Order

1. **`src/types.rs`** — `FileCategory` + `classify_file()` + tests (no deps)
2. **`src/index/git.rs`** — `blame_lines()` + `BlameEntry` + tests (needs git repo fixture)
3. **`src/search/context.rs`** — Category fields, `bridge_docs_via_provenance()`, updated summary
4. **`src/config.rs`** — `show_docs` in HooksConfig
5. **`src/cli/hook.rs`** — Sectioned formatting, `--show-docs` flag, updated metrics, wire GitAnalyzer through

## Risks

- **Git blame is slow on large files**: Changelogs can be 1000+ lines, but we only blame the *matching chunk's* line range (typically 10-30 lines). Fast.
- **Blame shows commits from initial file creation**: For very old changelog entries, the commit may not have coupling to current source layout. Mitigated by the fact that only high-scoring (semantically relevant) chunks get blamed.
- **GitAnalyzer not currently passed to ContextAssembler**: Need to add it as a parameter. The assembler currently only takes embedder + vector_store + metadata_store. Adding git_analyzer is a clean extension.

## Verification

1. `cargo test` — all existing tests pass
2. New unit tests: `classify_file()` heuristics, `blame_lines()` output parsing
3. Manual test: `bobbin hook inject-context` on a repo with changelogs → verify source files appear under "Source Files" header
4. Re-run ruff-001 eval → measure source file ratio in injection (target: >50%) and ground truth overlap improvement
5. Test `--show-docs false` suppresses documentation section

## Bead

`bo-cwt4v` — assigned to bobbin/crew/goldblum
