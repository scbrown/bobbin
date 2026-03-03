<p align="center">
  <img src="assets/bobbin-header.svg" alt="BOBBIN" width="700"/>
</p>

<p align="center">
  <img src="assets/bobbin-spool.svg" alt="Thread bobbin spool" width="360"/>
</p>

**Local-first context injection engine for AI coding agents.** Index your codebase, search it semantically, and automatically inject relevant context into every agent prompt. No API keys. No cloud. Sub-100ms queries.

> *Your codebase has structure, history, and meaning. Bobbin indexes all three — and feeds them to your agent exactly when needed.*

## How It Works

Bobbin sits between your codebase and your AI agent, automatically providing the right context at the right time:

```text
 ┌──────────┐     ┌──────────┐     ┌───────────────┐     ┌──────────┐
 │ Codebase │────▶│  Index   │────▶│ Context Engine │────▶│  Agent   │
 │          │     │          │     │               │     │          │
 │ code     │     │ chunks   │     │ search        │     │ Claude   │
 │ docs     │     │ vectors  │     │ couple        │     │ Cursor   │
 │ git      │     │ coupling │     │ assemble      │     │ any MCP  │
 │ commits  │     │ commits  │     │ inject        │     │ client   │
 └──────────┘     └──────────┘     └───────────────┘     └──────────┘
```

**1. Index** — Parse code with tree-sitter, embed with ONNX, analyze git history for coupling.
**2. Search** — Hybrid semantic + keyword search via Reciprocal Rank Fusion.
**3. Assemble** — Build budget-controlled context bundles from search results + coupled files.
**4. Inject** — Deliver context via Claude Code hooks or MCP tools, automatically on every prompt.

## See It In Action

```text
$ bobbin search "authentication middleware"
✓ Found 8 results for: authentication middleware (hybrid)

1. src/auth/middleware.rs:14 (verify_token)
   function rust · lines 14-47 · score 0.8923 [hybrid]

2. src/auth/session.rs:88 (create_session)
   function rust · lines 88-121 · score 0.8541 [semantic]
```

```text
$ bobbin context "fix the login bug"
✓ Context for: fix the login bug
  6 files, 14 chunks (487/500 lines)

--- src/auth/middleware.rs [direct, score: 0.8923] ---
  verify_token (function), lines 14-47
--- src/auth/session.rs [coupled via src/auth/middleware.rs] ---
  create_session (function), lines 88-121
```

## Quick Start

```bash
cargo install bobbin            # Install
cd your-project
bobbin init && bobbin index     # Index your codebase
bobbin hook install             # Set up automatic context injection
```

That's it. Every prompt you send now gets relevant context injected automatically.

## Features

### Context Injection Pipeline

**[Automatic Hook Injection](https://scbrown.github.io/bobbin/guides/hooks.html)** — On every prompt, bobbin embeds your message, searches the index, assembles a context bundle, and injects it as a system reminder. Smart gating skips injection when context would be irrelevant. Session dedup avoids re-injecting unchanged context.

**[Tool-Aware Reactions](https://scbrown.github.io/bobbin/guides/hooks.html)** — Rules that fire after tool calls. When an agent edits a Terraform-managed file, remind them to update IaC. When they restart a service, surface known issues. Configurable in `.bobbin/reactions.toml`.

**[PostToolUseFailure Context](https://scbrown.github.io/bobbin/guides/hooks.html)** — When a tool fails, bobbin searches for relevant code and docs to help the agent recover.

**[Feedback Loop](https://scbrown.github.io/bobbin/guides/hooks.html)** — Every injection gets a unique `injection_id`. Agents can rate injections as useful, noise, or harmful to improve quality over time.

### Indexing & Search

**[Hybrid Search](https://scbrown.github.io/bobbin/guides/searching.html)** — Semantic + keyword results fused via Reciprocal Rank Fusion. Boolean queries (`OR`, `NOT`), regex patterns, and inline filters (`repo:`, `lang:`, `type:`).

**[Structure-Aware Parsing](https://scbrown.github.io/bobbin/architecture/languages.html)** — Tree-sitter extracts functions, classes, structs, traits, and more from 7 languages. Markdown parsed into sections, tables, and code blocks.

**[Multi-Repo Indexing](https://scbrown.github.io/bobbin/guides/multi-repo.html)** — Index multiple repositories into one database. Named groups for scoped search. Webhook-triggered incremental reindexing.

**[Tags & Annotations](https://scbrown.github.io/bobbin/config/reference.html)** — Tag chunks with `auto:config`, `user:ops-docs`, or any label. Tag effects (boost, demote, exclude) control how tagged content ranks in search.

### Codebase Intelligence

**[Git Temporal Coupling](https://scbrown.github.io/bobbin/guides/git-coupling.html)** — Analyzes commit history to find files that change together. Reveals hidden dependencies no import graph can see.

**[Context Assembly](https://scbrown.github.io/bobbin/guides/context-assembly.html)** — `bobbin context "fix the login bug"` builds a budget-controlled bundle: search results, coupled files, bridging signals, and temporal decay — all within a configurable line budget.

**[Hotspot Analysis](https://scbrown.github.io/bobbin/guides/hotspots.html)** — Identify high-churn, high-complexity code that's most likely to cause bugs.

**[Impact Prediction](https://scbrown.github.io/bobbin/cli/impact.html)** — Predict which files are affected by a change using coupling data and dependency graphs.

**[Duplicate Detection](https://scbrown.github.io/bobbin/cli/similar.html)** — Find semantically similar code chunks or scan for duplicates across the codebase.

**[Review Context](https://scbrown.github.io/bobbin/cli/review.html)** — Assemble review bundles from git diffs to aid code review.

### Agent Integration

**[MCP Server](https://scbrown.github.io/bobbin/mcp/overview.html)** — `bobbin serve` exposes 12+ tools to Claude Code, Cursor, and any MCP-compatible client.

**[HTTP API](https://scbrown.github.io/bobbin/mcp/http-mode.html)** — Multi-repo search server with web UI, REST API, Prometheus metrics, and Forgejo webhook integration.

**[Role-Based Access](https://scbrown.github.io/bobbin/config/reference.html)** — Control what each agent can see with path-based filtering per role.

### Performance

**GPU Accelerated** — Automatic CUDA detection for 10-25x faster indexing on NVIDIA GPUs. Falls back to CPU seamlessly.

| Metric | CPU | GPU (RTX 4070S) |
|--------|-----|-----------------|
| Embed throughput | ~100 chunks/s | ~2,400 chunks/s |
| Index 57K chunks | >30 min | ~4 min |

## AI Agent Setup

### Claude Code Hooks (recommended)

```bash
bobbin hook install    # Auto-installs all hooks
```

This installs hooks for `UserPromptSubmit` (context injection), `SessionStart` (project primer), `PostToolUse` (reactions), and `PostToolUseFailure` (error recovery context).

### MCP Server

```json
{
  "mcpServers": {
    "bobbin": {
      "command": "bobbin",
      "args": ["serve"]
    }
  }
}
```

Tools: `search`, `grep`, `context`, `related`, `find_refs`, `list_symbols`, `read_chunk`, `hotspots`, `impact`, `review`, `similar`, `prime`.

## Supported Languages

| Language   | Parser        | Extracted Units |
|------------|---------------|-----------------|
| Rust       | Tree-sitter   | functions, impl blocks, structs, enums, traits, modules |
| TypeScript | Tree-sitter   | functions, methods, classes, interfaces |
| Python     | Tree-sitter   | functions, classes |
| Go         | Tree-sitter   | functions, methods, type declarations |
| Java       | Tree-sitter   | methods, constructors, classes, interfaces, enums |
| C++        | Tree-sitter   | functions, classes, structs, enums |
| Markdown   | pulldown-cmark| sections, tables, code blocks, YAML frontmatter |

Other file types (YAML, TOML, shell scripts, etc.) use line-based chunking with overlap.

## Documentation

**[The Bobbin Book](https://scbrown.github.io/bobbin/)** — comprehensive guides, reference, and architecture docs.

| Topic | Link |
|-------|------|
| Getting Started | [Installation & first index](https://scbrown.github.io/bobbin/getting-started/quick-start.html) |
| Hooks & Injection | [Automatic context injection](https://scbrown.github.io/bobbin/guides/hooks.html) |
| Searching | [Hybrid, boolean, regex queries](https://scbrown.github.io/bobbin/guides/searching.html) |
| Context Assembly | [Budget-controlled bundles](https://scbrown.github.io/bobbin/guides/context-assembly.html) |
| Git Coupling | [Temporal co-change analysis](https://scbrown.github.io/bobbin/guides/git-coupling.html) |
| Multi-Repo | [Named groups, webhooks](https://scbrown.github.io/bobbin/guides/multi-repo.html) |
| CLI Reference | [All 20+ commands](https://scbrown.github.io/bobbin/cli/overview.html) |
| MCP Tools | [Agent integration reference](https://scbrown.github.io/bobbin/mcp/overview.html) |
| Configuration | [`.bobbin/config.toml` reference](https://scbrown.github.io/bobbin/config/reference.html) |
| Architecture | [System design & data flow](https://scbrown.github.io/bobbin/architecture/overview.html) |
| Evaluation | [Methodology & results](https://scbrown.github.io/bobbin/eval/overview.html) |
| Contributing | [Build, test, develop](CONTRIBUTING.md) |

## License

[MIT](LICENSE)
