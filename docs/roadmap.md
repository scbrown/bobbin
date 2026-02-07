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
- [ ] Watch mode / file watcher daemon
- [ ] Shell completions (bash/zsh/fish)
- [ ] Configurable embedding models
- [ ] Import/dependency graph analysis
- [ ] Integration tests against real repos
- [ ] Performance optimizations
