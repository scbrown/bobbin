# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

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
