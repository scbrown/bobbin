# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- **Multimodal ingest — PDF text (bo-j5r0)** — opt-in `[index] multimodal`
  flag. When enabled, `bobbin index` also walks `**/*.pdf`, extracts text via a
  pure-Rust extractor (`pdf-extract`; no Python/native toolchain), and chunks it
  like a plain-text document (`language = "pdf"`) so runbooks, design docs, and
  specs become searchable. Off by default — no change to the default
  dep/behavior profile. Image captioning (vision LLM) is tracked as a follow-up.

## [0.4.0] - 2026-06-27

Search quality, knowledge-graph ranking, workflow telemetry, and a bead
access-control hardening.

### Added

- **Personalized PageRank ranking signal** — `search::ppr` folds a bounded
  graph-connectivity boost (seeded by the top hybrid hits, computed via
  `quipu::page_rank` over the `co_changed_with` coupling graph) into context
  ranking. Off by default; enable with `--ppr-weight` / `[search] ppr_weight`.
  Eval harness gains `calibrate --ppr-weights` for tuning.
- **Workflow telemetry (GH#9)** — `bead_lineage` store + `bobbin bead
  link`/`history` (Layer 1); automatic bead→commit association from `Bead*`
  commit trailers during indexing (Layer 1.5); `bobbin bundle additions` and
  `bundle drift` over the lineage (Layer 2).
- **Ontology (GH#14)** — ontology-aware search (tag/bundle hierarchy
  expansion), `GET /ontology` + `/ontology/{tag}` REST endpoints, and
  `bobbin ontology infer` (candidate concepts from coupling communities).
- **Beads indexing (GH#13)** — index bead labels and `metadata`; incremental
  bead indexing (content-hash skip); `[beads] exclude_labels` keeps sensitive
  beads (e.g. `security`, `escalation`) out of the index entirely.

### Fixed

- **FTS 500 (GH#21)** — keyword/`--type` search no longer 500s with "Failed to
  collect FTS results"; the index self-heals (rebuild + retry) and is rebuilt
  after `watch` compaction.
- **Hook status (GH#10)** — detects project-level hooks whose commands are
  wrapped with env prefixes / absolute paths / `|| true`.
- `--type` help and MCP schemas now list all valid chunk types (incl.
  `issue`/`bead`, `commit`, `doc`).

### Security

- **Bead access control** — bead chunks (`beads:<rig>:<id>`) are now
  access-scoped to their rig, so per-rig allow/deny rules apply to beads exactly
  as to code (previously they bypassed deny rules and could expose all beads).

## [0.3.1] - 2026-03-24

### Added

- `bobbin connect <url>` command — server-first setup with auto hook install
- Auto-detect forge type (GitHub, GitLab, Forgejo, Bitbucket) for source URL deep links
- Deep linking support in web UI (`#search?q=foo`, `#context?q=bar`)
- Repo/tag/group filter controls on the web UI search page
- Feedback CLI command with server mode proxy (GH#7)
- `--repo-root` flag for cross-repo deep bundle view
- Multi-agent onboarding improvements — `BOBBIN_SERVER` env hints + hook install (GH#4)
- Inline query syntax reference in search guide (`repo:`, `lang:`, `type:`, `file:`, `tag:`, `group:`)

### Fixed

- Watch service now detects per-file git repo names (GH#2)
- Hook status walks up directory tree to find parent settings.json (GH#3)
- Bundle/tags discovery walks up directory tree past git roots (GH#5)
- Normalize absolute file paths to repo-relative in `bundle add` (GH#6)
- Dynamic `repo_path_prefix` replaces hardcoded `/var/lib/bobbin/repos/` path

## [0.1.0] - 2026-02-07

### Added

- Code indexing with tree-sitter parsing for Rust, TypeScript, Python, Go, Java, and C++
- Semantic search using ONNX Runtime embeddings (all-MiniLM-L6-v2)
- Full-text keyword search via LanceDB/tantivy
- Hybrid search combining semantic and keyword results with Reciprocal Rank Fusion
- Git history analysis for temporal context
- Coupling detection between files based on co-change patterns
- MCP server for AI agent integration
- CLI with `index`, `search`, `grep`, `mcp-server`, and `completions` subcommands
- LanceDB as primary vector storage with SQLite for coupling metadata
- Support for `.bobbinignore` exclude patterns
