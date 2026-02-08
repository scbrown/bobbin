# Bobbin Feature Backlog

High-level feature ideas for Phase 4: Higher-Order Analysis. These compose
bobbin's existing signals (embeddings, coupling, complexity, deps, refs, history)
into capabilities that are greater than the sum of their parts.

---

## 1. Test Coverage Mapping

**Idea**: Use git temporal coupling to infer which test files cover which source
files. If `auth.rs` and `test_auth.rs` have a coupling score of 0.9, that's
strong evidence of a test relationship. Flag source files with no coupled test
files as coverage gaps.

**Building blocks that exist**:
- `FileCoupling` data model with score, co_changes, last_co_change
- `MetadataStore::get_coupling()` queries coupled files sorted by score
- Context assembly already expands via coupling
- Git coupling analysis (`analyze_coupling()`) with frequency + recency scoring

**What's needed**:
- Test file detection heuristics (pattern-based: `*_test.rs`, `tests/`, `__tests__/`, `*.spec.ts`, etc.)
- New CLI command: `bobbin coverage [file]` — show test files covering a source file, or show uncovered files
- Coverage gap report: source files with zero coupled test files
- Optional: `ChunkType::Test` variant for richer chunk classification
- MCP tool: `test_coverage`

**Scope**: Medium. Mostly composition of existing coupling queries + pattern matching.

---

## 2. Claude Code Hooks / Tool Integration

**Idea**: Make bobbin a drop-in context provider for Claude Code and other AI
tools. Ship hook scripts and MCP config so users can wire bobbin into their
workflow with minimal setup. Context injection at the right moments — on prompt
submit, on file open, on diff review.

**Building blocks that exist**:
- MCP server with 8 tools, stdio transport (`bobbin serve`)
- JSON output from all CLI commands (search, context, grep, refs, etc.)
- Context assembly with budget tracking and coupling expansion
- HTTP thin-client mode for centralized deployments

**What's needed**:
- Pre-built Claude Code hook scripts:
  - `PreToolUse` hook: inject relevant context before file reads/edits
  - `PostToolUse` hook: update index after file writes
  - `UserPromptSubmit` hook: enrich prompts with bobbin context
- `bobbin hook install` command to configure hooks in `~/.claude/hooks`
- `bobbin hook context` — streamlined context output tuned for hook injection
  (compact, budget-aware, no chrome)
- MCP server config generator: `bobbin mcp-config` outputs the JSON block for
  claude_desktop_config.json or .mcp.json
- Documentation: integration guide for Claude Code, Cursor, Windsurf, Continue

**Scope**: Medium-Large. Hook scripts are small but the UX design matters.
The MCP server already works — this is about making setup frictionless.

---

## 3. Semantic Commit Indexing

**Idea**: Index commit messages as searchable chunks alongside code. Enable
queries like "find the commit that added rate limiting" or "what changed around
the auth refactor". Commits become first-class searchable entities.

**Building blocks that exist**:
- `FileHistoryEntry` with message, author, timestamp, extracted issue IDs
- `parse_file_history()` already extracts commit data from git log
- Embedder can embed arbitrary text (commit messages work fine)
- LanceDB schema has `repo` column for multi-repo support
- `ChunkType` enum is extensible

**What's needed**:
- New `ChunkType::Commit` variant
- Commit indexing pipeline: parse git log → embed messages → store in LanceDB
- Schema extension: add `commit_hash`, `commit_author`, `commit_date` columns
  (or encode in existing fields)
- Incremental commit indexing: track last-indexed commit, only index new ones
- CLI: `bobbin search --type commit "rate limiting"` (already works if chunks exist)
- Commit-to-file linking: store which files a commit touched for navigation
- MCP tool update: search tool already supports type filtering

**Scope**: Medium. The indexing pipeline is the main work. Search comes free
once commits are stored as chunks.

---

## 4. Cross-Repo Coupling

**Idea**: Detect temporal coupling across repository boundaries. When changing
the API schema in repo A always requires updating the client in repo B, surface
that relationship. Critical for monorepo-adjacent architectures and microservice
ecosystems.

**Building blocks that exist**:
- Multi-repo support: `--repo` flag, repo column in LanceDB, `get_all_repos()`
- Per-repo coupling analysis via `analyze_coupling()`
- SQLite metadata store for coupling + dependencies

**What's needed**:
- Extend SQLite coupling table with `repo_a` and `repo_b` columns
- Cross-repo coupling analyzer: given repos A and B, find files that change in
  the same time windows (not same commits — these are different repos)
- Time-window correlation: commits within N minutes across repos suggest coupling
- CLI: `bobbin related --cross-repo file.rs` — show coupled files in other repos
- Dashboard view: `bobbin coupling-matrix` — show which repos are tightly coupled
- Config: register related repos and their git roots

**Scope**: Large. Requires new analysis algorithm (time-window correlation vs
same-commit co-change). Schema migration for SQLite tables.

---

## 5. Refactoring Planner

**Idea**: Given a refactoring target (rename symbol, move file, extract function),
generate an ordered modification plan using impact analysis + symbol refs + dependency
graph. "To rename `FooBar` → `BazQux`, modify these 14 files in dependency order."

**Building blocks that exist**:
- `ImpactAnalyzer` with transitive expansion (coupling + semantic signals, decay)
- `RefAnalyzer` with `find_refs()` (definition + usages with exact line numbers)
- `MetadataStore::get_dependents()` — what files import a given file
- `GitAnalyzer::get_diff_files()` — precise line-level change extraction
- Import resolution across 7 languages (Rust, TS, Python, Go, Java, C++)

**What's needed**:
- Implement `ImpactMode::Deps` (currently stubbed) to use dependency graph
- `RefactoringPlan` data structure: target, operation type, ordered file list,
  per-file change descriptions, confidence scores
- Dependency-aware ordering: modify leaf dependents first, work toward root
- Operation types: rename symbol, move file, extract function, inline function
- CLI: `bobbin plan rename FooBar BazQux` — generate plan
- CLI: `bobbin plan move src/old.rs src/new.rs` — generate move plan
- MCP tool: `refactoring_plan`
- Dry-run mode: show what would change without making changes

**Scope**: Large. The algorithm for ordering modifications correctly (respecting
dependency direction) is the hard part. Symbol refs are FTS-based (~80% accuracy),
so plans should include confidence levels.

---

## Priority Assessment

| Feature | Leverage | Scope | Existing Foundation | Priority |
|---------|----------|-------|-------------------|----------|
| Test Coverage Mapping | High | Medium | Strong (coupling exists) | P1 |
| Claude Code Hooks | High | Medium | Strong (MCP + CLI exist) | P1 |
| Semantic Commit Indexing | Medium | Medium | Strong (git parsing exists) | P2 |
| Refactoring Planner | High | Large | Good (impact + refs exist) | P2 |
| Cross-Repo Coupling | Medium | Large | Partial (multi-repo exists) | P3 |
