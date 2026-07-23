# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.6.2] - 2026-07-23

### Added

- *(deploy)* Glibc-safe build + gated, rollback-capable cutover([3b52b9a](https://github.com/scbrown/bobbin/commit/3b52b9aaf342228b0c54f72dc6d89d0daeeb7eb7))

### CI/CD

- *(release)* Adopt release-plz — config AND the job that executes it([749984c](https://github.com/scbrown/bobbin/commit/749984c048ecb024b121b2c59864b50869cf808c))

### Fixed

- *(release)* Track Cargo.lock — release-plz cannot determine versions without it([bbd8b3f](https://github.com/scbrown/bobbin/commit/bbd8b3f874a07b983d43cde1454578a1cb091207))
- *(release)* Declare publish = false — the crates.io 'bobbin' is an unrelated crate([3ccdcc5](https://github.com/scbrown/bobbin/commit/3ccdcc581e7793363f2fba714a1a4ffa949499c5))
- *(calibrate)* Auto-calibration failed every run — passed source tree as calibrate path instead of bobbin home([f94f7a0](https://github.com/scbrown/bobbin/commit/f94f7a0e31812275197ebcd7579b73e06c8deef3))
- *(release)* Untrack committed-but-gitignored data files — they fail release-plz's clean-tree check([0ea99cd](https://github.com/scbrown/bobbin/commit/0ea99cd7f7ca2613ffa73cf38e0c6e82d68da49a))
- *(release)* Release = true — release-plz skips publish=false crates by default([9563aae](https://github.com/scbrown/bobbin/commit/9563aaeac13a932d59afe4abdb453609edd6ed74))

## [Unreleased]

## [0.6.3] - 2026-07-23

### Documentation

- *(plans)* Status-label the 3 hardening/metrics/hooks plans vs main([aa3f24c](https://github.com/scbrown/bobbin/commit/aa3f24c2f84e14877b0847587dfc5b44f471619c))
- *(plans)* Label 5 plan docs with implementation status + frontmatter for 4 guide pages([ef2714a](https://github.com/scbrown/bobbin/commit/ef2714a3c591f3ea8504ee340adaec2b9a337563))
- *(plans)* Status-label the 4 eval plans — all implemented/record ( sweep) (#48)([3c324e7](https://github.com/scbrown/bobbin/commit/3c324e7af60b9a641f1c0876673275bbe5361cbd))
- *(plans)* Label 4 plan docs with verified implementation status (#49)([a9ca093](https://github.com/scbrown/bobbin/commit/a9ca0933611c9fb880e8330db0074fc2ca79a19f))
- *(plans)* Status-label the last 6 plan docs — 3 done, 2 dark-behind-a-feature, 1 backlog([6d14759](https://github.com/scbrown/bobbin/commit/6d147594a136615a72dcc90a1cd6dcb58b38484e))

### Fixed

- *(release)* Match release-plz git_tag_name to the repo's v-prefix tags([e064e56](https://github.com/scbrown/bobbin/commit/e064e5634add7c1ac5d5d53fe379e475b02aaa5c))
- *(release)* Move publish restriction out of the manifest so release-plz opens PRs([a8cffbe](https://github.com/scbrown/bobbin/commit/a8cffbe784815cdf7677c4fb14253cf3e249688a))

## [0.6.0] - 2026-07-13

Multimodal PDF ingest, index-freshness safety net, and two indexing/telemetry
correctness fixes.

### Added

- **Multimodal ingest — PDF text (bo-j5r0)** — opt-in `[index] multimodal`
  flag. When enabled, `bobbin index` also walks `**/*.pdf`, extracts text via a
  pure-Rust extractor (`pdf-extract`; no Python/native toolchain), and chunks it
  like a plain-text document (`language = "pdf"`) so runbooks, design docs, and
  specs become searchable. Off by default — no change to the default
  dep/behavior profile. Image captioning (vision LLM) is tracked as a follow-up.
- **Periodic reindex backstop for `watch` (#44)** — `bobbin watch` now runs a
  periodic full-tree reconciliation (on by default, every 15 min;
  `--reindex-interval-secs`, `0` disables). Each sweep re-embeds files whose
  content hash drifted and prunes rows for files that vanished from disk,
  catching events the file watcher dropped. Sweeps are incremental, so one where
  the watcher kept up does almost no work.
- **Index freshness signal in `status` (#44)** — `bobbin status` reports a
  `Freshness` line (and JSON field) that flags the index stale when the current
  git HEAD commit is newer than the last index run. Uses commit time, not
  wall-clock, so a quiet repo is never a false positive.

### Fixed

- **Batched prune delete (#43)** — pruning a source with more than SQLite's
  `SQLITE_MAX_VARIABLE_NUMBER` (32766) files in one pass no longer exceeds the
  bound-variable limit and aborts. The `DELETE … IN (…)` is chunked within a
  single transaction, keeping the prune atomic and the index consistent.
- **Hook injection count in remote deployments (#42)** — `bobbin hook status`
  reported `Injection count: 0` while injection was firing. The remote inject
  path now advances `hook_state`, and `hook status` resolves the bobbin root the
  same way the inject path does (first ancestor with `.bobbin/config.toml`), so
  the reported count is accurate and no longer depends on CWD.

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
