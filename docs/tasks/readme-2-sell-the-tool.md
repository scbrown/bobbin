# Task: Make the README sell bobbin

## Summary

The README needs to grab attention, explain why bobbin exists, and convince developers/agent builders to try it. Currently it reads like a reference manual. It needs a compelling narrative.

## File

`README.md`

## Key changes

### 1. Opening hook

Replace the dry one-liner with something that sells the problem and solution:

The current opening:
> A local-first semantic code indexing tool. Bobbin indexes your codebase for semantic search using embeddings stored in LanceDB, with metadata and full-text search in SQLite.

Should convey: **Your AI agent doesn't understand your codebase. Bobbin fixes that.**

Bobbin's unique angle is Temporal RAG - it doesn't just search code, it understands how code evolves together. No other local tool does this. Lead with that.

### 2. "Why Bobbin?" section

Add a section right after the opening that positions bobbin against alternatives. Key differentiators:

| | Bobbin | grep/ripgrep | GitHub Copilot | Cloud RAG |
|---|---|---|---|---|
| Understands code structure | AST-aware | No | Partial | Varies |
| Knows what changes together | Git temporal coupling | No | No | No |
| Runs locally | Always | Always | No | No |
| Semantic search | Yes (embeddings) | No | Yes | Yes |
| No API keys needed | Yes | Yes | No | No |
| Works offline | Yes | Yes | No | No |

The "Temporal RAG" angle is the real differentiator. No one else does git-history-informed retrieval.

### 3. Compelling use cases

Add concrete scenarios that make people go "I need this":

- **"What files do I need to touch?"** - `bobbin related src/auth.rs` shows you everything that historically changes alongside auth
- **"How does this codebase handle errors?"** - `bobbin search "error handling"` finds all error patterns semantically
- **"Set up Claude Code with full project context"** - `bobbin serve` exposes your entire index via MCP
- **"What changed recently in this module?"** - `bobbin history src/api/` shows evolution with churn metrics

### 4. Agent integration section

This is a HUGE selling point. Bobbin was built for AI agents. Highlight:
- MCP server (`bobbin serve`) works with Claude Code, Cursor, etc.
- JSON output on every command for programmatic consumption
- The upcoming `bobbin context` command (coming soon)
- Example MCP configuration snippet for Claude Code

### 5. Quick demo / "See it in action"

The Quick Start is fine but could be more exciting. Show the output, not just the commands. Include example output that demonstrates the value:

```
$ bobbin search "authentication middleware"
✓ Found 3 results (hybrid)

1. src/auth/middleware.rs:42 (authenticate)
   function rust · score 0.95 [hybrid]

2. src/auth/token.rs:15 (validate_token)
   function rust · score 0.87 [semantic]

$ bobbin related src/auth/middleware.rs
Related files for src/auth/middleware.rs:
  src/auth/token.rs      score: 0.92  (changed together 15 times)
  tests/auth_test.rs     score: 0.85  (changed together 12 times)
  src/config/auth.toml   score: 0.71  (changed together 8 times)
```

### 6. Badges

Add at the top:
- crates.io version
- crates.io downloads
- License (MIT)
- CI status (once workflows exist)

Standard markdown badge format:
```markdown
[![Crates.io](https://img.shields.io/crates/v/bobbin.svg)](https://crates.io/crates/bobbin)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)
```

### 7. Tone

Current tone is technical and dry. For the sections aimed at selling, use:
- Active voice ("Bobbin finds..." not "Files are found by...")
- Concrete examples over abstract descriptions
- Short sentences for impact
- Bold the key differentiators

The reference sections (command docs, config) can stay technical.

## Structure recommendation

```
# Bobbin
[badges]
[one compelling paragraph]

## Why Bobbin?
[comparison table + key differentiators]

## Quick Start
[commands with example output]

## Use Cases
[3-4 compelling scenarios]

## Commands
[full reference for all 8 commands]

## MCP Server (AI Agent Integration)
[setup instructions, example config]

## Configuration
[config.toml reference]

## Supported Languages
[updated table]

## Architecture
[existing diagram - keep it]

## Roadmap
[updated, accurate]

## Contributing
[link to CONTRIBUTING.md]

## License
```

## Acceptance Criteria

- [ ] Opening hook grabs attention
- [ ] "Why Bobbin?" section with comparison table
- [ ] Use cases section with concrete examples
- [ ] Agent integration section with MCP setup
- [ ] Example output shown (not just commands)
- [ ] Badges at top
- [ ] Confident, active tone throughout selling sections
- [ ] Still accurate and useful as a reference
