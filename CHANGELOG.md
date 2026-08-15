# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.6.7] - 2026-08-07

Bootstrap the repository-owned release baseline. Release-plz can now package
the pinned Quipu dependency and derives Bobbin's version from `v*` tags instead
of the unrelated crate on crates.io.

### Added

- *(deploy)* pull-based deploy from a published release artifact

### Fixed

- *(release)* derive versions from repository tags
- *(release)* make the pinned Quipu dependency packageable

## [0.6.6] - 2026-08-04

Release: the ks9cl P0 finally has a name

### Added

- *(mcp)* knowledge tools now federate the ontology, and say which graph answered([25e38cf](https://github.com/scbrown/bobbin/commit/25e38cf3209910e79811aba5ed92848b91c1ba74))

### Fixed

- *(mcp)* a server restart permanently broke every live agent's MCP tools([4958748](https://github.com/scbrown/bobbin/commit/4958748ca25011b792e5729c11689d75785bdb02))
- *(deploy)* the glibc-safe build image has no protoc, so the build dies mid-run([9e1f716](https://github.com/scbrown/bobbin/commit/9e1f7166b57f608ee284478f09cfe2c228f9cc01))
- *(mcp)* trim knowledge payloads so the answer can actually be received([12b7d07](https://github.com/scbrown/bobbin/commit/12b7d074f302f38e812b1a066a2b5650d680c73f))

### Changed

- *(mcp)* federate via the EXISTING quipu_endpoint key, not a second knob([74ac8c9](https://github.com/scbrown/bobbin/commit/74ac8c9adeeb70a9b5bbe2497fe3aef8a5e6f189))

### Documentation

- *(deploy)* make the glibc-safe container build a RECIPE, not an emergency procedure([7f2ecbd](https://github.com/scbrown/bobbin/commit/7f2ecbd8ed1596ba1339ebc6cde8974196c5ce6c))

## [0.6.5] - 2026-08-02

Release: the first release since v0.6.0 that can actually build

### Added

- *(deploy)* fail the cutover on a featureless binary, not just a glibc mismatch([7aafb2c](https://github.com/scbrown/bobbin/commit/7aafb2c51ce69b417faeff2b9c0fc6d6a6feae85))
- *(version)* emit the build git sha so a deploy is verifiable (/version + --version)([300b9c0](https://github.com/scbrown/bobbin/commit/300b9c06bfc8e2f0282645dc07967a0fd35ed370))

### Fixed

- *(deploy)* build the shipped binary WITH --features knowledge([864c710](https://github.com/scbrown/bobbin/commit/864c710d34123adea091885f52b0f41773a6bd8a))
- *(beads)* /beads pushes the Issue filter into LanceDB instead of over-fetch-then-filter([2eba8f2](https://github.com/scbrown/bobbin/commit/2eba8f29447ec57da16c1eb6522f49eca1e7154a))
- *(lance)* bound compaction memory and prune before compacting([585a02e](https://github.com/scbrown/bobbin/commit/585a02eece732de9d903a6d0aea4091cca0dea4e))
- *(embed)* bound the embed batch at the chokepoint — a whole corpus reached the model in one call([b7f340b](https://github.com/scbrown/bobbin/commit/b7f340b38cd0a1ea92a02bc19e7206b8bcfe61af))
- *(lance)* maintenance must sweep every table, not just chunks([90ea77a](https://github.com/scbrown/bobbin/commit/90ea77ad15000bb4200f55b7b6b62b0789ed0dbf))
- *(lance)* scheduled maintenance WAITS for the lock; a skip is no longer silent([2795d71](https://github.com/scbrown/bobbin/commit/2795d71877658d4e26f8fa6ac565108116d10c3f))
- *(lance)* one lock acquisition per sweep — a per-op wait multiplies by 27 repos([3b68aea](https://github.com/scbrown/bobbin/commit/3b68aeaedd4c63995e90d78a10d5c21da29964a6))
- *(release)* the aarch64 leg OOM-kills the runner under fat LTO — 4 tags, 0 releases([f84f7b7](https://github.com/scbrown/bobbin/commit/f84f7b758fe7d3bb58b0b4c46e1a43a086a95851))

## [0.6.4] - 2026-07-23

Release: deploy the knowledge/PPR build to the serving host (#56)

### Added

- *(config)* feature-gate a non-zero default ppr_weight (0.3) on knowledge builds([297f77c](https://github.com/scbrown/bobbin/commit/297f77c1bdc18bae545a3e809097af6fe38068cf))

### Fixed

- *(release)* match release-plz git_tag_name to the repo's v-prefix tags([e064e56](https://github.com/scbrown/bobbin/commit/e064e5634add7c1ac5d5d53fe379e475b02aaa5c))
- *(release)* move publish restriction out of the manifest so release-plz opens PRs([a8cffbe](https://github.com/scbrown/bobbin/commit/a8cffbe784815cdf7677c4fb14253cf3e249688a))
- *(serve)* wire the Quipu store into the HTTP /context path so PPR runs there too([4c6ed6e](https://github.com/scbrown/bobbin/commit/4c6ed6e1d43626fdcd95f2c23b19dd058119fdba))

### Documentation

- *(plans)* status-label the 3 hardening/metrics/hooks plans vs main([aa3f24c](https://github.com/scbrown/bobbin/commit/aa3f24c2f84e14877b0847587dfc5b44f471619c))
- *(plans)* label 5 plan docs with implementation status + frontmatter for 4 guide pages([ef2714a](https://github.com/scbrown/bobbin/commit/ef2714a3c591f3ea8504ee340adaec2b9a337563))
- *(plans)* status-label the 4 eval plans — all implemented/record (sweep) (#48)([3c324e7](https://github.com/scbrown/bobbin/commit/3c324e7af60b9a641f1c0876673275bbe5361cbd))
- *(plans)* label 4 plan docs with verified implementation status (#49)([a9ca093](https://github.com/scbrown/bobbin/commit/a9ca0933611c9fb880e8330db0074fc2ca79a19f))
- *(plans)* status-label the last 6 plan docs — 3 done, 2 dark-behind-a-feature, 1 backlog([6d14759](https://github.com/scbrown/bobbin/commit/6d147594a136615a72dcc90a1cd6dcb58b38484e))
- *(design)* status-label the 8 design docs outside docs/plans/([786834e](https://github.com/scbrown/bobbin/commit/786834e71b50fec929474317d88eeccb45df384b))
- *(plans)* note the named-graph (quipu #36/#49) substrate for subset-export (#57)([38a1c53](https://github.com/scbrown/bobbin/commit/38a1c53a783f344868025fc43bc4331de8cab300))
- *(design)* correct micro-ui status ⬜→🟡 — the embedding IS built([13ec199](https://github.com/scbrown/bobbin/commit/13ec199ef9c37d18a2fe76320dd18b7416f60411))
- label implementation status of roadmap + architecture (plan sweep)([9e35d57](https://github.com/scbrown/bobbin/commit/9e35d57f9cdddb7a3dca74f927173efe0a945295))

## [0.6.3] - 2026-07-23

Release: the structural-backend seam callers + purge watermark fix, released via the glibc-safe runner build for the serving-host deploy lane

### Added

- *(analysis)* the structural-backend seam — swappable engine contract for refs/symbols/impact([6bb49f8](https://github.com/scbrown/bobbin/commit/6bb49f853779d74927cdb03a9d22e2acd307e2cc))
- *(analysis)* route callers through the structural-backend seam; impact goes behind it([5b7fc3a](https://github.com/scbrown/bobbin/commit/5b7fc3a2ed7f689178eb61633dd617b176c41825))

### Fixed

- *(index)* purge resets the per-repo commit watermark so re-index rebuilds full history([a1d5871](https://github.com/scbrown/bobbin/commit/a1d587129020b459c3b55bdaeeb8e8ad521ede6a))

## [0.6.2] - 2026-07-23

Release: glibc-safe deploy tooling + the calibrate fix, plus the release-plz adoption trail

### Added

- *(deploy)* glibc-safe build + gated, rollback-capable cutover([3b52b9a](https://github.com/scbrown/bobbin/commit/3b52b9aaf342228b0c54f72dc6d89d0daeeb7eb7))

### Fixed

- *(release)* track Cargo.lock — release-plz cannot determine versions without it([bbd8b3f](https://github.com/scbrown/bobbin/commit/bbd8b3f874a07b983d43cde1454578a1cb091207))
- *(release)* declare publish = false — the crates.io 'bobbin' is an unrelated crate([3ccdcc5](https://github.com/scbrown/bobbin/commit/3ccdcc581e7793363f2fba714a1a4ffa949499c5))
- *(calibrate)* auto-calibration failed every run — passed source tree as calibrate path instead of bobbin home([f94f7a0](https://github.com/scbrown/bobbin/commit/f94f7a0e31812275197ebcd7579b73e06c8deef3))
- *(release)* untrack committed-but-gitignored data files — they fail release-plz's clean-tree check([0ea99cd](https://github.com/scbrown/bobbin/commit/0ea99cd7f7ca2613ffa73cf38e0c6e82d68da49a))
- *(release)* release = true — release-plz skips publish=false crates by default([9563aae](https://github.com/scbrown/bobbin/commit/9563aaeac13a932d59afe4abdb453609edd6ed74))

### CI/CD

- *(release)* adopt release-plz — config AND the job that executes it([749984c](https://github.com/scbrown/bobbin/commit/749984c048ecb024b121b2c59864b50869cf808c))

## [0.6.1] - 2026-07-22

Release: the paa8 repo-scoped index state + m2ob compaction lock/retry release

### Added

- *(index)* index yaml/jinja/terraform/shell so IaC repos are searchable([c35d632](https://github.com/scbrown/bobbin/commit/c35d632dcb54c9052ac2a068a81dfb4f8e0f37fb))

### Fixed

- *(knowledge)* build the quipu feature, and make PPR actually rerank([296238c](https://github.com/scbrown/bobbin/commit/296238cdec6f8cc737fd0c90699c8f4dbe8e2ead))
- *(deps)* choose the quipu rev deliberately, and say why([048e1bc](https://github.com/scbrown/bobbin/commit/048e1bce70200abe65c043ac1145eee1cfcb7661))
- *(index)* repo-scope ALL incremental state — hashes, watermarks, deletes — and give every store one consistent key([3eaced2](https://github.com/scbrown/bobbin/commit/3eaced2a68a16a30c819a502ca492108b4e0fb52))
- *(lance)* single-compactor gate + commit-conflict retry — stop the indexer/server table race([4c02576](https://github.com/scbrown/bobbin/commit/4c025769aff38f9c36667d64db09ee8a12b0294d))

### Documentation

- *(plans)* mark PPR plan as dark — knowledge feature enabled by no build path([37d70c5](https://github.com/scbrown/bobbin/commit/37d70c5382f8abf98e61e88f4a806fc7f2229300))

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
