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

## Rust Development (Bobbin)

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
