# Plan: Eval Results in mdbook + New Language Tasks

## Context

We just validated the eval framework end-to-end on flask-001 and found:
- The pipeline works but needed fixes (setup_command, werkzeug pinning, Python 3.11)
- First results are promising (with-bobbin: 100% precision, 40% faster)
- Results are currently JSON files — no way to share them publicly
- Polecats built the framework but never ran it end-to-end — we need to
  build solid infrastructure first so agents don't skip validation

We want to publish eval results as part of the bobbin documentation site,
add project/bobbin metadata to results, and expand to TypeScript and Go repos.

## Changes

### 0. Add tokei to project setup

**`justfile`** — add tokei to the `setup` recipe:
- Check for `tokei` binary, install via `cargo install tokei` if missing
- Also check for `uv` (needed for Python eval tasks)
- This ensures polecats have all eval dependencies ready

### 1. Collect metadata during eval runs

**`eval/runner/workspace.py`** — add `collect_loc_stats()`:
- Run `tokei --output json` on workspace (fast, language-aware LOC counter)
- Error if tokei not found (no silent fallback — forces `just setup` to be run)
- Returns: `{language: {files, lines, code, comments, blanks}}`
- Called once per workspace setup, stored in result JSON as `project_metadata`

**`eval/runner/bobbin_setup.py`** — modify `setup_bobbin()`:
- Time the index step with `time.monotonic()`
- After indexing, run `bobbin status --json` to capture stats
- Return metadata dict: `{index_duration_seconds, total_files, total_chunks, total_embeddings, languages}`
- Currently returns `None` — change is backward-compatible

**`eval/runner/cli.py`** — update `_run_single()`:
- Capture LOC stats after workspace setup
- Capture bobbin metadata from `setup_bobbin()` return value
- Store both as new keys in result JSON: `project_metadata`, `bobbin_metadata`

### 2. SVG chart generation

**`eval/analysis/svg_charts.py`** (new):
- `grouped_bar_chart(groups, width, height)` — inline SVG grouped bars
- `horizontal_bar(value, max_value, color)` — sparkline bar for table cells
- Dracula palette: purple `#bd93f9` (no-bobbin), green `#50fa7b` (with-bobbin)
- Pure Python string formatting, no dependencies

### 3. mdbook page generation

**`eval/analysis/mdbook_pages.py`** (new):
- Reads results JSON + task YAMLs
- Generates markdown pages with frontmatter + inline SVG charts
- Reuses helpers from `report.py` (`_load_results`, `_group_by_task`, `_compute_approach_stats`)

**Pages generated** under `docs/book/src/eval/`:

| Page | Content |
|------|---------|
| `summary.md` | Comparison table, grouped bar chart, per-project mini-table, judge summary |
| `projects.md` | Project catalog: LOC breakdown, bobbin stats per repo |
| `flask.md` | Per-task detail: commit link, prompt, results table, files touched |
| `ruff.md` | Same format as flask.md |

**`docs/book/src/eval/overview.md`** (hand-written, not generated):
- Explains commit-revert methodology
- Describes scoring dimensions
- Task selection criteria

### 4. CLI command

**`eval/runner/cli.py`** — add `publish` command:
```
bobbin-eval publish results/ --output-dir docs/book/src/eval --tasks-dir tasks
```
- Loads results + tasks + judge results
- Generates all mdbook pages
- Writes to output directory

### 5. mdbook integration

**`docs/book/src/SUMMARY.md`** — add section between Architecture and Reference:
```markdown
# Evaluation

- [Methodology](eval/overview.md)
- [Results Summary](eval/summary.md)
- [Project Catalog](eval/projects.md)
- [Flask (Python)](eval/flask.md)
- [Ruff (Rust)](eval/ruff.md)
```

**`docs/book/custom/css/custom.css`** — add eval styling:
- `.eval-pass` / `.eval-fail` badges (green/red)
- `.eval-chart` container for SVG
- `.eval-delta-positive` / `.eval-delta-negative` colors

### 6. New language tasks (AFTER infrastructure is validated)

**TypeScript** — `microsoft/TypeScript` (the compiler):
- Large, very well tested, frequent focused cross-file fixes
- `setup_command: "npm install"`, `test_command: "npx hereby runtests --tests=..."`
- Curate tasks from compiler bug fixes (2-5 file changes)

**Go** — `hashicorp/terraform` (infrastructure as code):
- Large, excellent test suite, frequent isolated bug fixes
- `setup_command: "go mod download"`, `test_command: "go test ./internal/..."`
- Curate tasks from provider/command bug fixes

5 tasks each, curated via `eval/scripts/curate_tasks.py`.

## Per-task detail page content (example)

Each task section on a project page shows:
1. **Task ID + difficulty badge** (easy/medium/hard)
2. **Commit**: hash linked to GitHub, one-line message
3. **Prompt**: the exact text sent to the agent (collapsible `<details>`)
4. **Results table**: no-bobbin vs with-bobbin (tests, precision, recall, F1, duration)
5. **Files touched vs ground truth**: side-by-side list
6. **Inline SVG bar chart** comparing the two approaches
7. **Project LOC**: total lines, language breakdown from tokei
8. **Bobbin stats**: chunks, embeddings, index duration

## Implementation order

**Phase A — Build infrastructure and validate (this session):**
1. Add tokei + uv to `just setup`
2. Metadata collection (workspace.py, bobbin_setup.py, cli.py)
3. Re-run flask-001 both approaches to get results WITH metadata
4. SVG charts module (svg_charts.py)
5. Page generation module (mdbook_pages.py)
6. `publish` CLI command
7. Hand-write overview.md, update SUMMARY.md + CSS
8. Run `bobbin-eval publish`, build mdbook, visually verify output
9. Commit everything that works

**Phase B — Full eval run on existing tasks (can be delegated):**
10. Run all 10 tasks (flask + ruff) × 2 approaches × 3 attempts
11. Run LLM judge on results
12. Publish and commit updated pages

**Phase C — New languages (after Phase B validates):**
13. Curate 5 TypeScript tasks from microsoft/TypeScript
14. Curate 5 Go tasks from hashicorp/terraform
15. Run evals, judge, publish

## Verification

After Phase A:
- `bobbin-eval publish results/` generates valid markdown with metadata
- `just docs build` succeeds with new eval pages
- Pages render correctly with Dracula theme
- SVG charts display in browser
- flask-001 results show LOC breakdown and bobbin stats

After Phase B:
- Full comparison table populated
- All 10 tasks have results for both approaches
- Judge results integrated into summary

## Files modified

| File | Change |
|------|--------|
| `justfile` | Add tokei + uv to setup recipe |
| `eval/runner/workspace.py` | Add `collect_loc_stats()` |
| `eval/runner/bobbin_setup.py` | Return metadata from `setup_bobbin()` |
| `eval/runner/cli.py` | Store metadata in results, add `publish` command |
| `eval/analysis/svg_charts.py` | New: SVG chart generation |
| `eval/analysis/mdbook_pages.py` | New: mdbook page generation |
| `docs/book/src/eval/overview.md` | New: hand-written methodology page |
| `docs/book/src/SUMMARY.md` | Add Evaluation section |
| `docs/book/custom/css/custom.css` | Add eval CSS classes |
