# Plan: Comprehensive Documentation Strategy (bo-rd42)

## Context

Bobbin has ~16 CLI commands, MCP tools, and rich analysis features, but documentation is scattered across README.md, docs/commands.md (only 9 of 16 commands documented), docs/architecture.md, docs/configuration.md, and various internal plan/task/design docs. There's no unified documentation site, no linting, and no frontmatter metadata. The goal is a comprehensive mdbook, frontmatter schema that dogfoods bobbin's own indexing, doc linting with auto-formatting, a `bobbin prime` command for LLM users, and a process to keep docs current as features land.

---

## 1. mdbook Setup

### Structure: `docs/book/`

Mirrors the pixelsrc convention at `/home/braino/gt/pixelsrc/crew/goldblum/docs/book/`.

```
docs/book/
├── book.toml
├── custom/css/custom.css
└── src/
    ├── SUMMARY.md
    ├── README.md                    # Book intro (adapted from repo README)
    ├── getting-started/
    │   ├── installation.md
    │   ├── quick-start.md           # Guided walkthrough (see "Try It" section)
    │   ├── concepts.md              # Chunks, hybrid search, coupling, context assembly
    │   └── agent-setup.md           # Claude Code, Cursor, MCP config
    ├── guides/
    │   ├── searching.md             # search + grep workflows
    │   ├── context-assembly.md      # context command deep dive
    │   ├── git-coupling.md          # related, coupling, temporal RAG
    │   ├── hotspots.md              # churn + complexity analysis
    │   ├── deps-refs.md             # deps + refs workflows
    │   ├── multi-repo.md            # --repo flag, multi-repo indexing
    │   ├── watch-automation.md      # watch mode, CI integration
    │   └── hooks.md                 # Claude Code hooks setup + smart injection
    ├── cli/
    │   ├── overview.md              # Global flags, thin-client mode
    │   ├── init.md .. completions.md  # 16 individual command pages
    │   └── prime.md                 # NEW: LLM primer command
    ├── mcp/
    │   ├── overview.md
    │   ├── tools.md                 # All MCP tools reference
    │   ├── client-config.md         # Claude Code, Cursor, generic
    │   └── http-mode.md             # --server flag thin-client
    ├── config/
    │   ├── reference.md             # Full config.toml reference
    │   ├── index.md .. hooks.md     # Per-section pages
    ├── architecture/
    │   ├── overview.md .. languages.md
    ├── reference/
    │   ├── chunk-types.md
    │   ├── search-modes.md
    │   ├── exit-codes.md
    │   └── glossary.md
    └── appendix/
        ├── vision.md
        ├── roadmap.md
        ├── changelog.md
        └── contributing.md
```

~47 pages across 9 sections. Every registered CLI command gets its own page.

### book.toml

```toml
[book]
title = "Bobbin Documentation"
authors = ["Steve Brown"]
description = "Complete documentation for Bobbin - the local-first code context engine"
src = "src"
language = "en"

[build]
build-dir = "book"

[output.html]
default-theme = "coal"
preferred-dark-theme = "coal"
git-repository-url = "https://github.com/scbrown/bobbin"
edit-url-template = "https://github.com/scbrown/bobbin/edit/main/docs/book/{path}"
additional-css = ["custom/css/custom.css"]
site-url = "/book/"

[output.html.search]
enable = true

[output.linkcheck]
warning-policy = "warn"
optional = true
```

### Content Migration

| Source | Destination | Action |
|--------|-------------|--------|
| `docs/commands.md` | `cli/*.md` | Split into 16 individual pages, write 7 missing |
| `docs/architecture.md` | `architecture/*.md` | Split into sub-pages |
| `docs/configuration.md` | `config/*.md` | Split + add missing `[hooks]` section |
| `docs/roadmap.md` | `appendix/roadmap.md` | Copy |
| `VISION.md` | `appendix/vision.md` | Copy |
| `CHANGELOG.md` | `appendix/changelog.md` | Copy |
| `CONTRIBUTING.md` | `appendix/contributing.md` | Copy + add doc checklist |
| `README.md` | `src/README.md` | Adapt as book intro |

**Stay separate** (not in book): `docs/plans/`, `docs/tasks/`, `docs/design/`, `docs/dev-log.md`, `PRD.md`, `AGENTS.md`, `GEMINI.md`

---

## 2. Frontmatter Metadata Schema

Standard YAML frontmatter for every page in the book. Designed to dogfood bobbin's existing frontmatter extraction (`src/index/parser.rs:584`) and align with the PRD Appendix B "Semantic Annotation Layer" vision.

### Schema

```yaml
---
title: "Page Title"                    # Required
description: "One-line summary"        # Required
tags: [cli, search, guide]             # Required - classification
status: stable                         # Required - stable | draft | planned
category: cli-reference               # Required - getting-started | guide | cli-reference | mcp | config | architecture | reference | appendix

# Relationships (PRD Appendix B alignment)
related:                               # Optional
  - path: "cli/search.md"
    relationship: "see-also"           # see-also | implements | supersedes | extends | requires

# Feature mapping (ties docs to source code)
feature: search                        # Optional - maps to bobbin module
commands: [search, grep]               # Optional - CLI commands covered
source_files:                          # Optional - source files described
  - src/cli/search.rs
  - src/search/hybrid.rs

# Versioning
last_verified: "0.1.0"                # Optional - bobbin version when last verified
---
```

### Why This Matters for Bobbin

1. **Immediate**: Bobbin already indexes frontmatter as `ChunkType::Doc` chunks — structured frontmatter means `bobbin search "hooks configuration"` finds the right page
2. **Coverage tooling**: `commands` and `source_files` fields enable automated gap detection
3. **Dogfooding**: The `related` field with typed relationships is the PRD Appendix B vision — bobbin's own docs become the first test corpus
4. **Future**: When bobbin gains frontmatter-aware graph queries, these docs exercise them

---

## 3. Documentation Linting & Formatting

### Two-Layer Approach: Vale + markdownlint-cli2

Use **both** tools — they're complementary, not competing:

| Concern | Tool | What It Catches |
|---------|------|-----------------|
| **Prose quality** | Vale | Passive voice, weasel words, jargon, inconsistent terminology, spelling |
| **Structure** | markdownlint-cli2 | Heading hierarchy, list formatting, code block languages, blank lines |
| **Auto-fix** | markdownlint-cli2 `--fix` | Fixes ~40% of structural violations automatically |

#### What Vale adds that markdownlint can't do

- **Style enforcement**: Microsoft/Google style guide rules (e.g., "don't say 'simply'" → suggests alternative)
- **Custom vocabulary**: Enforce "bobbin" (not "Bobbin" mid-sentence), "LanceDB" (not "lancedb"), "Tree-sitter" (not "treesitter")
- **Prose warnings**: Flag passive voice, weasel words ("very", "basically"), overly complex sentences
- **No auto-fix**: Vale reports only — the human (or agent) decides how to fix prose issues

#### Vale Config: `.vale.ini`

```ini
StylesPath = .vale/styles
MinAlertLevel = suggestion

Vocab = Bobbin

[*.md]
BasedOnStyles = Vale, write-good
```

With `.vale/styles/Vocab/Bobbin/accept.txt`:
```
bobbin
LanceDB
Tree-sitter
MCP
ONNX
SQLite
```

#### markdownlint Config: `.markdownlint-cli2.yaml`

```yaml
config:
  MD013: { line_length: 120, tables: false, code_blocks: false, headings: false }
  MD033: false
  MD024: { siblings_only: true }
  MD026: false
  MD041: true
  MD040: true
  MD004: { style: "dash" }

globs:
  - "docs/book/src/**/*.md"
  - "README.md"
  - "CONTRIBUTING.md"

ignores:
  - "docs/tasks/**"
  - "docs/plans/**"
  - "docs/design/**"
```

### Markdown Table Auto-Formatting

Neither Vale nor markdownlint formats tables. Add **prettier** with its built-in markdown support:

```bash
npx prettier --write "docs/book/src/**/*.md" --prose-wrap preserve
```

Prettier auto-aligns table columns so they're readable even in raw markdown:
```
# Before:
| Flag | Short | Default | Description |
|------|-------|---------|-------------|
| `--threshold` | `-t` | `0.85` | Minimum cosine similarity |
| `--scan` | | `false` | Scan entire codebase |

# After (prettier):
| Flag           | Short | Default | Description                |
| -------------- | ----- | ------- | -------------------------- |
| `--threshold`  | `-t`  | `0.85`  | Minimum cosine similarity  |
| `--scan`       |       | `false` | Scan entire codebase       |
```

Add a `.prettierrc.yaml` scoped to markdown:
```yaml
overrides:
  - files: "*.md"
    options:
      proseWrap: preserve
      tabWidth: 2
```

### Frontmatter & Coverage Validation Scripts

- `scripts/validate-frontmatter.sh` — Checks required fields, valid `status` values, resolvable `related` paths, `commands` match `src/cli/mod.rs`
- `scripts/doc-coverage.sh` — Compares registered CLI commands, MCP tools, and config sections against documented pages

---

## 4. justfile: Grouped Recipes with Subcommand Pattern

`just` doesn't support true subcommands, but we can use a **single parameterized recipe** to avoid top-level sprawl. This mirrors how `cargo` subcommands work:

```just
# === Documentation ===

# Documentation management: just docs <cmd>
# Commands: build, serve, lint, fix, fmt, vale, validate, coverage, check
docs cmd="build":
    #!/usr/bin/env bash
    set -euo pipefail
    case "{{cmd}}" in
        build)    mdbook build docs/book ;;
        serve)    mdbook serve docs/book --open ;;
        lint)     npx markdownlint-cli2 "docs/book/src/**/*.md" "README.md" "CONTRIBUTING.md" ;;
        fix)      npx markdownlint-cli2 --fix "docs/book/src/**/*.md" "README.md" "CONTRIBUTING.md" ;;
        fmt)      npx prettier --write "docs/book/src/**/*.md" --prose-wrap preserve ;;
        vale)     vale docs/book/src/ ;;
        validate) bash scripts/validate-frontmatter.sh ;;
        coverage) bash scripts/doc-coverage.sh ;;
        check)    just docs lint && just docs vale && just docs validate && just docs build ;;
        *)        echo "Unknown: {{cmd}}. Try: build serve lint fix fmt vale validate coverage check" ;;
    esac
```

Usage: `just docs build`, `just docs serve`, `just docs fmt`, `just docs check`, etc.

Only one new top-level recipe (`docs`) added to the justfile. All doc operations are sub-commands of it.

---

## 5. "Try It" — Guided Quick Start

Pixelsrc's WASM demos are domain-specific (rendering sprites in-browser). For bobbin, the equivalent is a **guided walkthrough** that works both in the docs and as a CLI command.

### In the Book: Guided Quick-Start Page

`docs/book/src/getting-started/quick-start.md` structured as an interactive tutorial:

1. **Step-by-step with expected output** — Each step shows the command AND its expected terminal output, so the reader can follow along and verify
2. **Progressive complexity** — init → index → search → context → related → hooks
3. **"What just happened?"** callout boxes after each step explaining the internals
4. **Troubleshooting tips** at each step for common issues

Example pattern:
```markdown
### Step 2: Index your codebase

```bash
$ bobbin index
```

You should see output like:

```
Indexing /home/you/myproject...
  Parsed 142 files (4 languages)
  Generated 1,847 chunks
  Embedded 1,847 vectors (batch 64, ~12s)
  Updated coupling data (500 commits)
✓ Index complete: 1,847 chunks in 14.2s
```

> **What happened?** Bobbin parsed your source files with Tree-sitter...
```

### In the CLI: `bobbin tour` (Bead 12)

A guided interactive walkthrough using the user's actual repo:

```bash
$ bobbin tour
Welcome to Bobbin! Let's explore your codebase.

[1/6] First, let's see what's indexed...
  → Running: bobbin status --detailed
  ...
  Press Enter to continue, or 'q' to quit.

[2/6] Search for something. Type a query (or press Enter for "error handling"):
  > _
```

**Modular tour segments**: Each feature registers a tour step. New features MUST add a tour segment — this is enforced in the doc checklist. Per-feature tours also supported: `bobbin tour hooks`, `bobbin tour search`.

This mirrors pixelsrc's demo coverage system: every feature has a runnable demonstration.

---

## 6. `bobbin prime` — LLM Context Overview

Modeled after pixelsrc's `pxl prime` command (`/home/braino/gt/pixelsrc/crew/goldblum/src/prime.rs`).

### What It Does

Outputs a structured, LLM-friendly overview of bobbin and the current project state. Designed to be piped into an LLM context window or used as an MCP tool.

### CLI Interface

```bash
bobbin prime                     # Full primer
bobbin prime --brief             # Condensed version (~50 lines)
bobbin prime --section commands  # Just the commands section
bobbin prime --section status    # Just current index status
```

### Output Sections

1. **Overview** — What bobbin is, core value prop (from VISION.md essence)
2. **Commands** — All registered commands with one-line descriptions (extracted from clap help text)
3. **Status** — Current index stats: files, chunks, languages, last indexed (live from `bobbin status`)
4. **Configuration** — Active config summary (paths, embedding model, search defaults)
5. **Integration** — MCP server setup, hooks status, thin-client mode
6. **Architecture** — Storage locations (.bobbin/), key modules, data flow summary

### Implementation

- New file: `src/cli/prime.rs`
- Embedded markdown: `docs/primer.md` (static sections compiled into binary via `include_str!`)
- Dynamic sections: call into existing `status` logic for live stats
- Register in `src/cli/mod.rs` as `Prime(prime::PrimeArgs)`
- Add as MCP tool: `bobbin_prime` in `src/mcp/server.rs`
- `--brief` flag returns condensed ~50-line version
- `--section` flag returns only the named section
- `--json` flag returns structured JSON

### Key Files

| File | Reference |
|------|-----------|
| `src/prime.rs` (pixelsrc) | `/home/braino/gt/pixelsrc/crew/goldblum/src/prime.rs` — reference implementation |
| `src/mcp/tools/prime.rs` (pixelsrc) | `/home/braino/gt/pixelsrc/crew/goldblum/src/mcp/tools/prime.rs` — MCP tool pattern |
| `src/cli/status.rs` | Existing status logic to reuse for live stats |
| `AGENTS.md` | Current static LLM primer — `bobbin prime` replaces this |

---

## 7. Keeping Docs Up to Date

### Feature Documentation Checklist (add to CONTRIBUTING.md)

> When adding or changing features:
> - [ ] CLI command page: create/update `docs/book/src/cli/<command>.md`
> - [ ] Configuration: update relevant `docs/book/src/config/*.md`
> - [ ] Guide: add/update guide if user-facing workflow changes
> - [ ] SUMMARY.md: add new pages to table of contents
> - [ ] Frontmatter: valid YAML with required fields
> - [ ] MCP tools: update `docs/book/src/mcp/tools.md`
> - [ ] Primer: update `docs/primer.md` if commands or architecture changed
> - [ ] README.md: mention new user-facing features
> - [ ] Tour segment: add a `bobbin tour` step for the feature
> - [ ] `just docs check` passes

### CI: `.github/workflows/docs.yml`

- On PR: markdownlint + vale + frontmatter validation + mdbook build
- On push to main: build + deploy to GitHub Pages (can defer deployment)

---

## 8. Implementation Order (Beads)

### Phase 1: Scaffolding

| Bead | Title | Depends On | Effort |
|------|-------|------------|--------|
| 1 | mdbook skeleton + book.toml + stub pages + custom CSS | — | Small |
| 2 | Migrate existing content (split commands, arch, config) | 1 | Medium |
| 3 | Document 7 missing CLI commands (deps, refs, hotspots, benchmark, watch, completions, hook) | 1 | Medium |

Beads 2 and 3 can run in parallel after Bead 1.

### Phase 2: New Content + Frontmatter

| Bead | Title | Depends On | Effort |
|------|-------|------------|--------|
| 4 | Write 8 user guides | 2 | Large |
| 5 | Getting Started (4 pages) + MCP (4 pages) + Reference (4 pages) | 1 | Medium |
| 6 | Add frontmatter to all ~47 pages | 2, 3, 4, 5 | Small-Medium |

Beads 4 and 5 can run in parallel.

### Phase 3: Tooling

| Bead | Title | Depends On | Effort |
|------|-------|------------|--------|
| 7 | Linting setup (markdownlint + vale + prettier + justfile `docs` recipe) | — | Small |
| 8 | CI pipeline (.github/workflows/docs.yml) | 1, 7 | Small |
| 9 | Coverage + frontmatter validation scripts | 6 | Medium |

Bead 7 is independent. Bead 8 needs 1+7. Bead 9 needs frontmatter in place.

### Phase 4: `bobbin prime` + Polish

| Bead | Title | Depends On | Effort |
|------|-------|------------|--------|
| 10 | `bobbin prime` CLI command + MCP tool + primer.md | — | Medium |
| 11 | Update repo README with book link, run linkcheck, clean up old docs | all | Small |
| 12 | `bobbin tour` interactive guided walkthrough | 10 | Medium |

Beads 10 and 12 are independent of the book work. 12 depends on 10 (tour references prime for context).

### Dependency Graph

```
                    ┌── 2 (migrate) ──────┐
1 (skeleton) ──────┤                      ├── 6 (frontmatter) → 9 (validation) → 11 (polish)
                    ├── 3 (missing cmds) ──┤
                    └── 5 (getting started)┘
                              │
                              └── 4 (guides) ──── 6

7 (lint+fmt) ──────── 8 (CI) ──── 11 (polish)

10 (bobbin prime) ──── 11 (polish)   # independent track
```

---

## Key Files

| File | Role |
|------|------|
| `src/cli/mod.rs` | Source of truth for registered commands |
| `docs/commands.md` | Current CLI ref to migrate (9 of 16 documented) |
| `src/config.rs` | Config structs including undocumented `HooksConfig` |
| `src/index/parser.rs:584` | Frontmatter extraction — dogfooding target |
| `src/mcp/server.rs` | MCP tools to document |
| `src/cli/hook.rs` | Complex command (7 subcommands) needing docs |
| `src/cli/status.rs` | Status logic to reuse in `bobbin prime` |
| `src/prime.rs` (pixelsrc) | Reference implementation for `bobbin prime` |
| `justfile` | Add `docs` parameterized recipe |

## Verification

After full implementation:
1. `just docs check` passes (lint + vale + validate + build)
2. `just docs fmt` auto-formats all tables
3. `just docs coverage` reports 100% CLI command coverage
4. `bobbin prime` outputs LLM-friendly overview; `bobbin prime --brief` is <50 lines
5. `bobbin index docs/book/src && bobbin search "hooks configuration"` finds relevant pages (dogfooding)
6. `mdbook serve docs/book --open` renders correctly in browser
7. Frontmatter `related` fields create a navigable relationship graph across docs
