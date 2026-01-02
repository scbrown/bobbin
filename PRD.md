# Bobbin Product Requirements Document (PRD)

**Version:** 1.0
**Status:** Draft
**Last Updated:** 2026-01-02

---

## 1. Executive Summary

Bobbin is a local-first code context engine built in Rust. It indexes codebases using structural parsing (Tree-sitter), stores embeddings in an embedded vector database (LanceDB), and exposes semantic and keyword search via a CLI interface. Its unique value is "Temporal RAG" - using git history to understand file relationships beyond vector similarity.

---

## 2. Problem Statement

### Current Pain Points

1. **Context Fragmentation**: AI agents lose context across sessions; they don't know what changed yesterday or what files are related.

2. **Privacy Concerns**: Cloud-based code search requires sending proprietary code to third parties.

3. **Poor Chunking**: Most RAG systems split code arbitrarily by token count, breaking functions mid-body and losing structural meaning.

4. **No Temporal Awareness**: Existing tools only see HEAD; they miss the rich signal in git history about how code evolves together.

5. **Latency**: Cloud round-trips add latency that breaks agentic workflows.

### Opportunity

A local, structure-aware, temporally-intelligent context engine fills a gap no current tool addresses. It enables a new class of AI-assisted development where agents have deep project awareness.

---

## 3. User Personas

### Persona 1: AI Coding Agent
- **Examples**: Claude Code, Cursor, Aider, custom LLM-based tools
- **Needs**: Fast, accurate context retrieval; understanding of code structure; awareness of related files
- **Constraints**: Limited context window; no persistent memory; relies on tools for project knowledge

### Persona 2: Developer (Direct User)
- **Examples**: Engineers exploring unfamiliar codebases
- **Needs**: Quick semantic search ("where is auth handled?"); find related code; understand file relationships
- **Constraints**: Doesn't want to set up complex infrastructure; needs it to just work

### Persona 3: Tool Builder
- **Examples**: Developers building custom dev tools, Tambour middleware
- **Needs**: Reliable API/CLI to build upon; extensible architecture
- **Constraints**: Needs stable interfaces; good documentation

---

## 4. User Stories

### Indexing

| ID | Story | Priority |
|----|-------|----------|
| US-1 | As a user, I can run `bobbin index` to index my current repository | P0 |
| US-2 | As a user, I can specify which directories/files to include or exclude | P1 |
| US-3 | As a user, I can see progress while indexing large repositories | P1 |
| US-4 | As a user, I can incrementally update the index when files change | P2 |

### Semantic Search

| ID | Story | Priority |
|----|-------|----------|
| US-5 | As a user, I can search for code semantically: `bobbin search "authentication logic"` | P0 |
| US-6 | As a user, I can limit search to specific file types: `bobbin search "..." --type rust` | P1 |
| US-7 | As a user, I can control how many results are returned | P1 |
| US-8 | As a user, I can get results in JSON format for programmatic use | P0 |

### Keyword Search

| ID | Story | Priority |
|----|-------|----------|
| US-9 | As a user, I can do exact keyword search: `bobbin grep "fn authenticate"` | P0 |
| US-10 | As a user, I can use regex patterns in keyword search | P1 |
| US-11 | As a user, I can combine semantic and keyword search | P2 |

### Related Context

| ID | Story | Priority |
|----|-------|----------|
| US-12 | As a user, I can find files related to a given file: `bobbin related src/auth.rs` | P0 |
| US-13 | As a user, I can see temporal coupling scores (files that change together) | P1 |
| US-14 | As a user, I can find files affected by a specific commit | P2 |

### Git Time-Machine

| ID | Story | Priority |
|----|-------|----------|
| US-15 | As a user, I can analyze git history to build coupling relationships | P1 |
| US-16 | As a user, I can see how a file has evolved: `bobbin history src/auth.rs` | P2 |
| US-17 | As a user, I can search across historical versions (not just HEAD) | P3 |

### Documentation

| ID | Story | Priority |
|----|-------|----------|
| US-18 | As a user, markdown files are indexed alongside code | P1 |
| US-19 | As a user, I can search only documentation: `bobbin search "..." --docs` | P2 |

### Interface

| ID | Story | Priority |
|----|-------|----------|
| US-20 | As a user, I can use bobbin entirely via CLI | P0 |
| US-21 | As a user, I can optionally connect via MCP for agent integration | P2 |
| US-22 | As a user, I can run bobbin as a daemon for faster repeated queries | P2 |

---

## 5. Functional Requirements

### 5.1 Indexing Engine

**FR-1: Structural Parsing**
- Use Tree-sitter to parse source files
- Extract semantic units: functions, classes, modules, interfaces
- Support languages: Rust, TypeScript, Python, Go, Java, C/C++ (initial set)
- Gracefully handle unsupported languages (fall back to line-based chunking)

**FR-2: Embedding Generation**
- Use local ONNX model (all-MiniLM-L6-v2 default)
- Generate embeddings for each semantic unit
- Store embeddings in LanceDB

**FR-3: Metadata Extraction**
- File path, language, last modified time
- Function/class names, signatures
- Line number ranges for each chunk
- Git blame information (author, commit)

**FR-4: Incremental Updates**
- Detect changed files via git status or file mtime
- Re-index only changed files
- Handle deleted files (remove from index)

### 5.2 Search Engine

**FR-5: Semantic Search**
- Embed query text using same model
- Perform approximate nearest neighbor search in LanceDB
- Return top-k results with similarity scores

**FR-6: Keyword Search**
- Full-text search using Tantivy or SQLite FTS
- Support regex patterns
- Case-sensitive and insensitive modes

**FR-7: Hybrid Search**
- Combine semantic and keyword scores
- Configurable weighting between methods
- Reciprocal rank fusion or similar algorithm

**FR-8: Filtering**
- Filter by file type/extension
- Filter by directory path
- Filter by language
- Filter by date range (modified time)

### 5.3 Git Forensics

**FR-9: Temporal Coupling Analysis**
- Parse git log to find co-changing files
- Build weighted graph of file relationships
- Calculate coupling scores based on:
  - Frequency of co-changes
  - Recency of co-changes
  - Commit proximity

**FR-10: Related Files**
- Given a file, return related files by:
  - Temporal coupling (git history)
  - Import/dependency graph (if parseable)
  - Vector similarity

**FR-11: Commit Context**
- Given a commit, return all affected files
- Show what changed and why (commit message)

### 5.4 CLI Interface

**FR-12: Core Commands**
```
bobbin init              # Initialize index in current repo
bobbin index             # Build/rebuild full index
bobbin index --incremental  # Update changed files only
bobbin search <query>    # Semantic search
bobbin grep <pattern>    # Keyword search
bobbin related <file>    # Find related files
bobbin history <file>    # Show file evolution
bobbin status            # Show index status
bobbin config            # Manage configuration
```

**FR-13: Output Formats**
- Human-readable (default): colored, formatted
- JSON: `--json` flag for programmatic consumption
- Quiet: `--quiet` for scripts

**FR-14: Configuration**
- `.bobbin/config.toml` in repo root
- Configurable: include/exclude patterns, languages, embedding model
- Environment variable overrides

### 5.5 MCP Interface (Optional)

**FR-15: MCP Server**
- Expose CLI functionality as MCP tools
- Tools: `search`, `grep`, `related`, `index_status`
- Resources: `bobbin://index/stats`

---

## 6. Non-Functional Requirements

### 6.1 Performance

| Metric | Target |
|--------|--------|
| Index time | < 60s for 100K LOC repo |
| Search latency | < 500ms for semantic search |
| Keyword search latency | < 100ms |
| Memory usage during indexing | < 2GB |
| Index size on disk | < 500MB for 100K LOC |

### 6.2 Scalability

- Support repositories up to 1M LOC
- Handle monorepos with multiple projects
- Incremental index updates complete in < 10s for typical changes

### 6.3 Reliability

- Graceful handling of malformed source files
- Index corruption recovery via rebuild
- No data loss on crash during indexing

### 6.4 Portability

- Support: macOS (ARM64, x86_64), Linux (x86_64, ARM64), Windows (x86_64)
- Single binary distribution (no runtime dependencies)
- Works in containers

### 6.5 Privacy & Security

- Zero network calls during normal operation
- All data stored locally in `.bobbin/` directory
- No telemetry unless explicitly opted-in
- Index files respect `.gitignore` by default

---

## 7. Technical Architecture

### 7.1 High-Level Components

```
┌─────────────────────────────────────────────────────────────┐
│                         CLI / MCP                           │
├─────────────────────────────────────────────────────────────┤
│                      Query Engine                           │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────────────┐ │
│  │  Semantic   │  │   Keyword   │  │  Related Context    │ │
│  │   Search    │  │   Search    │  │     Resolver        │ │
│  └─────────────┘  └─────────────┘  └─────────────────────┘ │
├─────────────────────────────────────────────────────────────┤
│                     Indexing Engine                         │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────────────┐ │
│  │ Tree-sitter │  │  Embedding  │  │   Git Forensics     │ │
│  │   Parser    │  │  Generator  │  │     Analyzer        │ │
│  └─────────────┘  └─────────────┘  └─────────────────────┘ │
├─────────────────────────────────────────────────────────────┤
│                      Storage Layer                          │
│  ┌─────────────────────────┐  ┌───────────────────────────┐│
│  │  LanceDB (Vectors)      │  │  SQLite (Metadata + FTS)  ││
│  └─────────────────────────┘  └───────────────────────────┘│
└─────────────────────────────────────────────────────────────┘
```

### 7.2 Data Flow

**Indexing:**
1. Walk repository files (respecting ignores)
2. Parse each file with Tree-sitter
3. Extract semantic chunks (functions, classes, etc.)
4. Generate embeddings via ONNX runtime
5. Store vectors in LanceDB, metadata in SQLite
6. Analyze git log for temporal coupling
7. Store coupling graph in SQLite

**Querying:**
1. Receive query via CLI/MCP
2. Embed query text
3. Search LanceDB for similar vectors
4. Optionally search SQLite FTS for keywords
5. Combine and rank results
6. Augment with related context if requested
7. Format and return results

### 7.3 Storage Schema

**SQLite Tables:**
```sql
-- Indexed files
CREATE TABLE files (
    id INTEGER PRIMARY KEY,
    path TEXT UNIQUE NOT NULL,
    language TEXT,
    mtime INTEGER,
    hash TEXT,
    indexed_at INTEGER
);

-- Semantic chunks
CREATE TABLE chunks (
    id INTEGER PRIMARY KEY,
    file_id INTEGER REFERENCES files(id),
    chunk_type TEXT,  -- function, class, module, etc.
    name TEXT,
    start_line INTEGER,
    end_line INTEGER,
    content TEXT,
    vector_id TEXT   -- reference to LanceDB
);

-- Full-text search
CREATE VIRTUAL TABLE chunks_fts USING fts5(
    content, name,
    content='chunks'
);

-- Temporal coupling
CREATE TABLE coupling (
    file_a INTEGER REFERENCES files(id),
    file_b INTEGER REFERENCES files(id),
    score REAL,
    co_changes INTEGER,
    last_co_change INTEGER,
    PRIMARY KEY (file_a, file_b)
);
```

**LanceDB Schema:**
```
vectors table:
  - id: string (matches chunks.vector_id)
  - vector: float32[384]  // MiniLM dimension
  - file_path: string
  - chunk_name: string
```

---

## 8. Configuration

### 8.1 Default Config (`.bobbin/config.toml`)

```toml
[index]
# Patterns to include (glob)
include = ["**/*.rs", "**/*.ts", "**/*.py", "**/*.go", "**/*.md"]

# Patterns to exclude (in addition to .gitignore)
exclude = ["**/node_modules/**", "**/target/**", "**/dist/**"]

# Respect .gitignore
use_gitignore = true

[embedding]
# Model to use (path to ONNX or model name)
model = "all-MiniLM-L6-v2"

# Batch size for embedding generation
batch_size = 32

[search]
# Default number of results
default_limit = 10

# Hybrid search weight (0 = keyword only, 1 = semantic only)
semantic_weight = 0.7

[git]
# Enable temporal coupling analysis
coupling_enabled = true

# How far back to analyze (commits)
coupling_depth = 1000

# Minimum co-changes to establish coupling
coupling_threshold = 3
```

---

## 9. Milestones

### Phase 1: Foundation (MVP)
**Goal:** Basic indexing and search working end-to-end

- [ ] Project scaffolding (Rust workspace, dependencies)
- [ ] Tree-sitter integration for Rust, TypeScript, Python
- [ ] ONNX embedding generation (all-MiniLM-L6-v2)
- [ ] LanceDB vector storage
- [ ] SQLite metadata storage
- [ ] CLI: `init`, `index`, `search`, `grep`
- [ ] JSON output format
- [ ] Basic documentation

**Exit Criteria:** Can index a medium repo and run semantic + keyword search

### Phase 2: Intelligence
**Goal:** Git forensics and related context

- [ ] Git log parsing and coupling analysis
- [ ] Coupling score calculation
- [ ] CLI: `related`, `history`
- [ ] Improved search ranking with coupling boost
- [ ] Markdown/documentation indexing
- [ ] Additional language support (Go, Java, C++)
- [ ] Incremental indexing

**Exit Criteria:** `bobbin related` returns useful suggestions based on git history

### Phase 3: Polish
**Goal:** Production-ready, extensible

- [ ] MCP server wrapper
- [ ] Watch mode (daemon with file system watching)
- [ ] Performance optimization
- [ ] Multi-repo support
- [ ] Configurable embedding models
- [ ] Comprehensive test suite
- [ ] Release binaries for all platforms

**Exit Criteria:** Ready for Tambour integration and public release

---

## 10. Risks & Mitigations

| Risk | Impact | Likelihood | Mitigation |
|------|--------|------------|------------|
| Tree-sitter grammar issues for some languages | Medium | Medium | Fallback to line-based chunking |
| LanceDB performance at scale | High | Low | Benchmark early, have SQLite-vec as backup |
| Embedding quality for code | Medium | Medium | Support multiple models, allow user choice |
| Git history analysis slow on large repos | Medium | High | Limit depth, cache results, incremental updates |
| Cross-platform ONNX issues | Medium | Medium | Extensive CI testing, pre-built binaries |

---

## 11. Open Questions

1. **Import graph analysis**: Should we parse imports to establish explicit dependencies? (Deferred to future phase)

2. **Caching strategy**: How aggressively should we cache embeddings vs. regenerate? (TBD during implementation)

3. **Multi-language files**: How to handle files with mixed languages (e.g., HTML with JS)? (Use dominant language)

4. **Binary files**: Index binary assets' metadata? (Probably not in MVP)

---

## 12. Glossary

| Term | Definition |
|------|------------|
| **Temporal Coupling** | Measure of how often two files change together in git history |
| **Semantic Chunk** | A meaningful unit of code (function, class) extracted via parsing |
| **Tree-sitter** | Incremental parsing library for building syntax trees |
| **LanceDB** | Embedded vector database using Lance columnar format |
| **MCP** | Model Context Protocol - Anthropic's standard for AI tool integration |
| **ONNX** | Open Neural Network Exchange - portable model format |
| **RAG** | Retrieval Augmented Generation - pattern of fetching context for LLMs |

---

## Appendix A: CLI Reference (Draft)

```
USAGE:
    bobbin <COMMAND>

COMMANDS:
    init        Initialize bobbin in current repository
    index       Build or update the search index
    search      Semantic search for code
    grep        Keyword/regex search
    related     Find files related to a given file
    history     Show evolution of a file
    status      Show index status and statistics
    config      View or modify configuration
    help        Print help information

GLOBAL FLAGS:
    --json      Output in JSON format
    --quiet     Suppress non-essential output
    --verbose   Show detailed progress
    --config    Path to config file

EXAMPLES:
    bobbin init
    bobbin index
    bobbin search "authentication handler"
    bobbin search "error handling" --type rust --limit 20
    bobbin grep "fn main"
    bobbin related src/lib.rs
    bobbin status --json
```

---

## Appendix B: North Star - Semantic Annotation Layer (Future)

> **Status:** Conceptual - Not planned for initial phases
> **Purpose:** Capture long-term vision for agent-authored semantic relationships

### Concept

Enable markdown files to contain structured semantic relationships via YAML frontmatter. This allows GenAI agents to create explicit, queryable relationships between documents that go beyond vector similarity. Frontmatter is a well-established pattern (Jekyll, Hugo, Obsidian) making it familiar and tooling-friendly.

### Use Cases

1. **Lessons Learned → Issues**: A lessons-learned doc can explicitly reference the issues that spawned it
2. **ADRs → Code**: Architecture Decision Records can link to the code they govern
3. **Code → Rationale**: Comments or companion docs can reference why decisions were made
4. **Cross-Document Threading**: Connect related concepts across the knowledge base

### Example Syntax (Conceptual)

```markdown
---
title: "Lessons Learned: Authentication Refactor"
references:
  issues: ["bobbin-42", "bobbin-47"]
supersedes: "docs/adr/ADR-003.md"
tags: ["security", "breaking-change"]
links:
  - to: "docs/adr/ADR-015.md"
    relationship: "implements"
  - to: "src/auth/mod.rs"
    relationship: "governs"
---

## Summary
During the auth refactor, we learned...
```

### Why This Matters

- **Explicit > Implicit**: Vector similarity finds "related" content; annotations capture *how* things relate
- **Agent Memory**: GenAI can record learnings in a structured way that persists and is queryable
- **Knowledge Graph**: Transforms flat docs into a navigable graph of project knowledge
- **Audit Trail**: Explicit links create traceable decision history

### Implementation Considerations (Deferred)

- YAML frontmatter parser for markdown files
- Schema for relationship types (`references`, `supersedes`, `links`, `tags`)
- Storage in SQLite as edges in a graph
- Query API: `bobbin graph --from ADR-015 --relationship implements`
- Validation: warn on broken references (missing files, unknown issues)
- Visualization: export to DOT/Mermaid for graph rendering
- Compatibility with existing frontmatter tools (Obsidian, etc.)

### Relationship to Tambour

This feature aligns with Tambour's mission of context injection. Tambour could:
- Inject relevant lessons-learned when an agent starts related work
- Surface ADRs when code governed by them is modified
- Build "context packages" that follow annotation links

---

*Document maintained by the Bobbin team.*
