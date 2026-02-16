---
title: Roadmap
description: Planned features and development roadmap for bobbin
tags: [appendix, roadmap]
status: draft
category: appendix
related: [appendix/vision.md, appendix/changelog.md]
---

# Roadmap

## Phase 1: Foundation (MVP) -- Complete

- [x] Tree-sitter code indexing (Rust, TypeScript, Python)
- [x] LanceDB vector storage
- [x] SQLite metadata + FTS
- [x] CLI: `init` command
- [x] CLI: `index` command (full and incremental)
- [x] Configuration management
- [x] ONNX embedding generation
- [x] CLI: `search` command
- [x] CLI: `grep` command
- [x] CLI: `status` command

## Phase 2: Intelligence -- Complete

- [x] Hybrid search (RRF combining semantic + keyword)
- [x] Git temporal coupling analysis
- [x] Related files suggestions
- [x] Additional language support (Go, Java, C++)

## Phase 3: Polish -- In Progress

- [x] MCP server integration (`bobbin serve`)
- [x] Multi-repo support (`--repo` flag)
- [x] LanceDB-primary storage consolidation
- [x] Contextual embeddings
- [x] Semantic markdown chunking (pulldown-cmark)
- [x] File history and churn analysis (`bobbin history`)
- [x] Context assembly command (`bobbin context`)
- [x] Watch mode / file watcher daemon (`bobbin watch`)
- [x] Shell completions (`bobbin completions`)
- [x] Code hotspot identification (`bobbin hotspots`)
- [x] Symbol reference resolution (`bobbin refs`)
- [x] Import/dependency analysis (`bobbin deps`)
- [x] AST complexity metrics
- [x] Transitive impact analysis with decay
- [x] Thin-client HTTP mode (`--server` flag)
- [ ] Configurable embedding models (infrastructure exists, UI incomplete)
- [ ] Integration tests against real repos
- [ ] Performance optimizations at scale

### Phase 3.5: Production Hardening -- In Progress

See `docs/plans/production-hardening.md` for details.

- [x] Install protoc + `just setup` recipe (bobbin-1nv)
- [x] Clean up tambour references (bobbin-7pn)
- [x] Fix production unwrap() calls (bobbin-ehp)
- [x] Integration test foundation (bobbin-ul6)
- [ ] Add missing MCP tools — deps, history, status (bobbin-tnt)
- [x] Add missing HTTP endpoints (bobbin-pid)
- [x] Wire up incremental indexing (bobbin-thb)
- [x] CI pipeline — GitHub Actions (bobbin-ola)
- [x] Update AGENTS.md and CONTRIBUTING.md (bobbin-6lx)

## Phase 4: Higher-Order Analysis -- In Progress

Compose existing signals into capabilities greater than the sum of their parts.
See `docs/plans/backlog.md` for detailed exploration of each feature.

- [ ] Test coverage mapping via git coupling
- [x] Claude Code hooks / tool integration
- [ ] Semantic commit indexing
- [ ] Refactoring planner (rename, move, extract)
- [ ] Cross-repo temporal coupling
