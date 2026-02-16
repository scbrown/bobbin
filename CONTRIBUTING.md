# Contributing to Bobbin

Bobbin is a local-first Rust code context engine.

## Using Just

This project uses [just](https://github.com/casey/just) as a command runner. **Always prefer `just` commands over raw `cargo` commands** - they're configured with sensible defaults that reduce output noise and save context.

```bash
just --list          # Show available commands
just setup           # Install system deps (protoc, c++, verify rust)
just build           # Build (quiet output)
just test            # Run tests (quiet output)
just check           # Type check (quiet output)
just lint            # Run clippy (quiet output)
just run             # Build and run
```

### Verbose Output

All cargo commands run in quiet mode by default (`-q --message-format=short`). To see full output:

```bash
just build verbose=true
just test verbose=true
```

## Rust Development

### Prerequisites

- Rust (stable toolchain) — install via [rustup](https://rustup.rs)
- `just` command runner
- `protoc` (Protocol Buffers compiler) — required by lancedb
- C++ compiler (`g++` on Linux, Xcode CLT on macOS)

Run `just setup` to install system dependencies automatically.

### Build Commands

```bash
just build           # Build the project
just test            # Run all tests
just check           # Type check without building
just lint            # Lint with clippy
```

## CI Pipeline

GitHub Actions runs on every push to `main` and on pull requests. The CI workflow:

1. Installs system dependencies (`protoc`, `cmake`, `g++`)
2. Runs `cargo check`
3. Runs `cargo test`
4. Runs `cargo clippy`

Ensure all three pass locally before pushing:

```bash
just check && just test && just lint
```

## Feature Integration Checklist

When adding new features to bobbin (new search signals, data sources, chunk types, or storage capabilities), review whether the `bobbin context` command should incorporate the new signal.

The `context` command is the "everything relevant in one shot" command. It combines hybrid search + temporal coupling to assemble task-aware context bundles. New retrieval signals should flow into it.

**Before merging a new feature, check:**

- [ ] Does this feature produce a new retrieval signal? (e.g., dependency graph, complexity scores)
- [ ] If yes, should `context` use it during assembly? Update `src/search/context.rs`
- [ ] Does this change chunk types or storage schema? Update context output types if needed
- [ ] Does the MCP `context` tool need updating? Check `src/mcp/server.rs`
- [ ] Are there new CLI flags that `context` should also expose?

**Task specs for the context command live in `docs/tasks/context-*.md`.**

## Documentation Checklist

When adding or modifying features, update documentation alongside code changes.

**Before merging, check:**

- [ ] Does the CLI `--help` text accurately describe the new/changed flags?
- [ ] Is the relevant `docs/book/src/cli/<command>.md` page updated?
- [ ] If a new MCP tool was added, is `docs/book/src/mcp/tools.md` updated?
- [ ] Are new concepts explained in the appropriate guide (`docs/book/src/guides/`)?
- [ ] Does `README.md` need updating? (e.g., feature list, MCP tool count, examples)
- [ ] Does the book build cleanly? (`mdbook build docs/book`)
- [ ] Do CI doc checks pass? (`npx markdownlint-cli2 "docs/book/src/**/*.md" "README.md" "CONTRIBUTING.md"`)
