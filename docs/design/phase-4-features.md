# Phase 4: Analysis & Intelligence Features

Phase 3 gave bobbin a solid foundation: structure-aware parsing, hybrid search,
git coupling, context assembly, and MCP integration. Phase 4 builds on these
primitives to answer higher-level questions about codebases.

## Design Principles

Each feature should:
1. **Reuse existing primitives** -- embeddings, coupling, parsing, LanceDB queries
2. **Work as both CLI and MCP tool** -- every command gets a corresponding MCP tool
3. **Support multi-repo** -- respect the `--repo` filter pattern
4. **Output JSON** -- `--json` flag for programmatic consumption

---

## 1. `bobbin similar` -- Semantic Clone Detection

### Motivation

Codebases accumulate near-duplicate code over time. Functions that do roughly
the same thing in different modules, copy-pasted handlers with minor variations,
parallel implementations that drifted apart. Finding these is tedious manually
but trivial with embeddings -- chunks that are semantically similar will have
nearby vectors.

### CLI Interface

```bash
# Find chunks similar to a specific function
bobbin similar src/auth.rs:login_handler
bobbin similar src/auth.rs:login_handler --threshold 0.85

# Find all near-duplicate pairs across the codebase
bobbin similar --scan
bobbin similar --scan --threshold 0.90 --repo backend

# Find chunks similar to a query (like search, but returns clusters)
bobbin similar "error handling with retry logic"
```

| Flag | Short | Default | Description |
|------|-------|---------|-------------|
| `--threshold <F>` | `-t` | `0.85` | Minimum cosine similarity to report |
| `--scan` | | `false` | Scan entire codebase for all duplicate pairs |
| `--limit <N>` | `-n` | `10` | Max results (or max clusters in scan mode) |
| `--repo <NAME>` | `-r` | all | Filter to specific repository |
| `--cross-repo` | | `false` | In scan mode, also compare across repos |

### Output

```
Similar to login_handler (src/auth.rs:45-82):

  1. session_handler (src/session.rs:23-58)     [0.94 similarity]
     Both handle credential validation with token generation

  2. api_login (src/api/auth.rs:12-45)           [0.91 similarity]
     Nearly identical logic, different error types

  3. mock_login (tests/auth_test.rs:100-130)     [0.87 similarity]
     Test double mirrors production implementation
```

### Implementation

**Core primitive**: LanceDB vector search already gives us cosine similarity.
The main work is:

1. **Single-target mode**: Look up the target chunk's embedding, run a vector
   search excluding itself, filter by threshold. This is essentially
   `VectorStore::search()` with a pre-computed embedding instead of a query
   string.

2. **Scan mode**: More interesting. Approach:
   - Pull all embeddings from LanceDB as a matrix
   - Use a k-NN self-join (each vector against all others)
   - LanceDB supports this natively via vector search on each row
   - For performance, batch: iterate chunks, search for each, deduplicate pairs
   - Cache results -- this is an expensive operation

3. **Cluster output**: Group similar chunks into clusters rather than listing
   all O(n^2) pairs. Simple approach: union-find on pairs above threshold.

**New code needed**:
- `src/cli/similar.rs` -- CLI command
- `src/search/similar.rs` -- Core similarity logic
- `src/mcp/server.rs` -- Add `similar` MCP tool

**Existing code reused**:
- `VectorStore::search()` for single-target similarity
- `VectorStore::get_chunks_for_file()` to resolve `file:name` targets
- `Embedder::embed()` if querying by text instead of chunk reference

### Complexity

Medium-low for single-target mode (essentially a variant of search).
Medium for scan mode (needs batched self-join + clustering).

---

## 2. `bobbin hotspots` -- Churn + Complexity Analysis

### Motivation

Not all code deserves equal attention. Files that change frequently AND are
complex are maintenance risks -- they're where bugs hide and refactoring pays
off the most. Bobbin already has both signals: churn rate (from `GitAnalyzer`)
and structural complexity (from Tree-sitter AST). Combining them surfaces the
code that most needs attention.

### CLI Interface

```bash
bobbin hotspots
bobbin hotspots --limit 20
bobbin hotspots --since "6 months ago"
bobbin hotspots --repo backend
bobbin hotspots --sort complexity   # Sort by complexity instead of combined score
```

| Flag | Short | Default | Description |
|------|-------|---------|-------------|
| `--limit <N>` | `-n` | `10` | Number of hotspots to show |
| `--since <DATE>` | | `"1 year ago"` | How far back to analyze git history |
| `--sort <FIELD>` | `-s` | `combined` | Sort by: `combined`, `churn`, `complexity` |
| `--repo <NAME>` | `-r` | all | Filter to specific repository |
| `--detailed` | | `false` | Show per-chunk breakdown within files |

### Output

```
Code Hotspots (sorted by risk score):

  #  File                        Churn  Complexity  Score
  1. src/storage/lance.rs         47      8.2       386
  2. src/search/context.rs        31      7.1       220
  3. src/index/parser.rs          28      6.8       190
  4. src/cli/index.rs             25      5.4       135
  5. src/mcp/server.rs            22      6.1       134

  Hotspot score = churn * complexity
  Churn = commits in last 12 months
  Complexity = avg AST depth * unique node types per chunk
```

### Implementation

**Churn data**: Already available. `GitAnalyzer::get_file_history()` gives us
commit counts per file. We need a new method that efficiently gets churn counts
for all files at once (batch mode) rather than one-by-one:

```rust
// New method on GitAnalyzer
pub fn get_file_churn(&self, since: Option<&str>) -> Result<HashMap<String, u32>>
```

This walks `git log --name-only` once and counts per-file appearances.

**Complexity metrics**: New. Compute from the Tree-sitter AST at index time
and store alongside chunks. Metrics per chunk:

- **AST depth**: Maximum nesting depth of the syntax tree
- **Node count**: Total AST nodes (proxy for size + branching)
- **Cyclomatic complexity**: Count of branch points (if/match/for/while/||/&&)

File-level complexity = average of chunk complexities, weighted by chunk size.

For the initial version, we can compute complexity on-the-fly from stored chunk
content by re-parsing with Tree-sitter. This avoids schema changes. If it's too
slow, we add a `complexity` column to LanceDB in a follow-up.

**Scoring**: `hotspot_score = churn * complexity`. Both values are normalized
to [0, 1] range before multiplication so neither dominates.

**New code needed**:
- `src/cli/hotspots.rs` -- CLI command
- `src/analysis/complexity.rs` -- AST complexity metrics
- `src/analysis/mod.rs` -- New analysis module
- `src/mcp/server.rs` -- Add `hotspots` MCP tool

**Existing code reused**:
- `GitAnalyzer::get_file_history()` (or new batch churn method)
- `Parser::new()` for re-parsing chunks to compute complexity
- `VectorStore::get_all_file_paths()` for file listing
- `VectorStore::get_chunks_for_file()` for chunk content

### Complexity

Medium. The git churn part is straightforward. The AST complexity analysis is
new but well-understood (cyclomatic complexity is a standard metric). The main
design question is whether to compute on-the-fly or pre-compute at index time.

**Recommendation**: Compute on-the-fly initially. If `bobbin hotspots` takes
>2s on a large repo, add pre-computed metrics to the LanceDB schema.

---

## 3. `bobbin review` -- Diff-Aware Context Assembly

### Motivation

`bobbin context` takes a natural language query and assembles relevant code.
But the most common "query" when doing code review is implicit: "what do I need
to understand to review these changes?" Given a git diff (branch, commit range,
or unstaged changes), bobbin can automatically assemble the surrounding context:
related functions, coupled files, affected interfaces.

This is the killer feature for AI-assisted code review.

### CLI Interface

```bash
# Context for uncommitted changes
bobbin review

# Context for a branch vs main
bobbin review --branch feature/auth

# Context for a specific commit range
bobbin review HEAD~3..HEAD

# Context for staged changes only
bobbin review --staged

# Larger budget for complex reviews
bobbin review --budget 1000 --branch feature/auth
```

| Flag | Short | Default | Description |
|------|-------|---------|-------------|
| `--branch <NAME>` | `-b` | | Compare branch against main |
| `--staged` | | `false` | Only consider staged changes |
| `--budget <LINES>` | | `500` | Max context lines |
| `--depth <N>` | `-d` | `1` | Coupling expansion depth |
| `--content <MODE>` | `-c` | `preview` | Content mode: full, preview, none |
| `--repo <NAME>` | `-r` | all | Filter coupled files to a repo |

### Output

Same format as `bobbin context`, but the "query" is derived from the diff:

```
✓ Review context for 3 changed files (branch: feature/auth)
  12 files, 24 chunks (387/500 lines)

--- src/auth.rs [changed: +45 -12] ---
  login_handler (function), lines 45-82
    pub async fn login_handler(req: LoginRequest) -> Result<Token> {
    ...

--- src/session.rs [coupled via git, score: 0.82] ---
  create_session (function), lines 12-34
    pub fn create_session(user: &User) -> Session {
    ...

--- src/types.rs [changed: +3 -0] ---
  LoginRequest (struct), lines 8-14
    pub struct LoginRequest {
    ...
```

### Implementation

**Step 1: Extract changed files and hunks from git diff.**

```rust
// New method on GitAnalyzer
pub fn get_diff_files(&self, spec: &DiffSpec) -> Result<Vec<DiffFile>>

pub struct DiffFile {
    pub path: String,
    pub added_lines: Vec<u32>,     // Line numbers of additions
    pub removed_lines: Vec<u32>,   // Line numbers of removals (in old version)
    pub status: DiffStatus,        // Added, Modified, Deleted, Renamed
}

pub enum DiffSpec {
    Unstaged,
    Staged,
    Branch(String),
    Range(String),  // e.g., "HEAD~3..HEAD"
}
```

**Step 2: Map diff hunks to indexed chunks.**

For each changed file, find which chunks overlap with the changed line ranges.
This uses `VectorStore::get_chunks_for_file()` and filters by line overlap.

**Step 3: Feed into existing ContextAssembler.**

The changed chunks become the "seed" results (instead of search results).
The coupling expansion, budget management, and output formatting all reuse
the existing `ContextAssembler` infrastructure. We essentially replace the
"search" step with "diff analysis" and keep everything else.

```rust
// Pseudocode for the core flow
let diff_files = git_analyzer.get_diff_files(&spec)?;
let seed_chunks = map_diff_to_chunks(&diff_files, &vector_store).await?;
let bundle = assembler.assemble_from_seeds(seed_chunks, repo).await?;
```

**This means we need to refactor `ContextAssembler` slightly**: extract the
seed-to-bundle logic from the search-to-seed logic so both `context` and
`review` can share the expansion/budgeting code.

**New code needed**:
- `src/cli/review.rs` -- CLI command
- `src/search/review.rs` -- Diff analysis + seed extraction
- Refactor `src/search/context.rs` -- Extract `assemble_from_seeds()`
- `src/index/git.rs` -- Add `get_diff_files()` method
- `src/mcp/server.rs` -- Add `review` MCP tool

**Existing code reused**:
- `ContextAssembler` (budgeting, coupling expansion, output formatting)
- `VectorStore::get_chunks_for_file()` for mapping diffs to chunks
- `MetadataStore::get_coupling()` for expansion
- `GitAnalyzer` for git operations

### Complexity

Medium. The git diff parsing is new but standard (`git diff --numstat` + custom
parsing). The main architectural work is refactoring ContextAssembler to accept
seeds from any source, not just search results. The coupling expansion and
budget management are fully reusable.

---

## 4. `bobbin impact` -- Change Impact Analysis

### Motivation

Before changing a function, you want to know: what else might break? Bobbin
already knows three kinds of relationships:
1. **Temporal coupling** -- files that historically change together
2. **Semantic similarity** -- code that does similar things
3. **Import/dependency graph** -- code that directly depends on this (in flight: `bobbin-graph`)

Combining all three gives a much better impact prediction than any one alone.

### CLI Interface

```bash
bobbin impact src/auth.rs
bobbin impact src/auth.rs:login_handler
bobbin impact src/auth.rs --depth 2           # Transitive impact
bobbin impact src/auth.rs --mode coupling     # Only coupling signal
bobbin impact src/auth.rs --mode semantic     # Only similarity signal
bobbin impact src/auth.rs --mode deps         # Only dependency signal
```

| Flag | Short | Default | Description |
|------|-------|---------|-------------|
| `--depth <N>` | `-d` | `1` | Transitive impact depth |
| `--mode <MODE>` | `-m` | `combined` | Signal: `combined`, `coupling`, `semantic`, `deps` |
| `--limit <N>` | `-n` | `15` | Max results |
| `--threshold <F>` | `-t` | `0.1` | Min impact score |
| `--repo <NAME>` | `-r` | all | Filter to specific repository |

### Output

```
Impact analysis for login_handler (src/auth.rs:45-82):

  #  File                          Signal      Score  Reason
  1. src/session.rs                coupling    0.82   Co-changed 47 times
  2. src/api/auth.rs               semantic    0.91   Similar auth logic
  3. src/middleware/auth.rs         deps        1.00   Imports login_handler
  4. tests/auth_test.rs            coupling    0.71   Co-changed 33 times
  5. src/types.rs                  coupling    0.45   Co-changed 12 times
  6. src/api/session.rs            semantic    0.43   Similar session handling

  Combined score = max(coupling, semantic, deps)
  Signals: coupling (git co-change), semantic (embedding similarity), deps (import graph)
```

### Implementation

**Three signal sources, combined with max():**

1. **Coupling signal**: Already exists. `MetadataStore::get_coupling()` returns
   co-change scores for a file.

2. **Semantic signal**: Use the target chunk's embedding to search for similar
   chunks (same as `similar` command). Filter to other files only.

3. **Dependency signal**: Depends on `bobbin-graph` landing. When available,
   query the import graph for dependents (what imports this file/function).
   Initially, this signal can be absent -- the command works with just coupling
   + semantic, and gains deps when the graph feature ships.

**Combining signals**: For each candidate file, take the max score across
available signals. This avoids weighting issues and ensures a strong signal
in any dimension surfaces the file.

**Transitive impact (depth > 1)**: Run impact analysis recursively on the
results of depth 1. Use a visited set to prevent cycles. Decay scores by
multiplying at each depth level (e.g., depth 2 scores *= 0.5).

**New code needed**:
- `src/cli/impact.rs` -- CLI command
- `src/analysis/impact.rs` -- Multi-signal impact analysis
- `src/mcp/server.rs` -- Add `impact` MCP tool

**Existing code reused**:
- `MetadataStore::get_coupling()` for coupling signal
- `VectorStore::search()` for semantic signal
- `VectorStore::get_chunks_for_file()` for resolving file:function targets
- `Embedder::embed()` if doing semantic comparison
- (Future) Dependency graph queries from `bobbin-graph`

### Dependency

**Partial dependency on `bobbin-graph`**. The `deps` signal is gated on
the import graph feature. Impact analysis ships with coupling + semantic
first, then gains deps as an upgrade. The `--mode deps` flag returns an
error until the graph is available.

### Complexity

Medium. The individual signals are all existing queries. The new work is:
- Resolving `file:function` syntax to a specific chunk
- Running multiple signal queries and merging results
- Transitive expansion with decay

---

## 5. `bobbin refs` -- Symbol Cross-References

### Motivation

Bobbin already extracts named symbols (functions, classes, structs, traits)
via Tree-sitter. The missing piece: where are those symbols *used*? A symbol
table that maps definitions to usages turns bobbin into a lightweight
cross-reference tool. For AI agents, this is essential -- "find all callers
of this function" is one of the most common code navigation operations.

### CLI Interface

```bash
# Find all references to a symbol
bobbin refs login_handler
bobbin refs LoginRequest --type struct
bobbin refs "impl AuthService" --type impl

# Find the definition of a symbol
bobbin refs login_handler --definition

# List all symbols in a file
bobbin refs --file src/auth.rs
```

| Flag | Short | Default | Description |
|------|-------|---------|-------------|
| `--type <TYPE>` | `-t` | any | Filter by symbol type |
| `--definition` | `-D` | `false` | Find definition instead of usages |
| `--file <PATH>` | `-f` | | List all symbols in a file |
| `--limit <N>` | `-n` | `20` | Max results |
| `--repo <NAME>` | `-r` | all | Filter to specific repository |

### Output

```
References to login_handler:

  Definition:
    src/auth.rs:45  fn login_handler(req: LoginRequest) -> Result<Token>

  Usages (7 found):
    src/api/routes.rs:23      .route("/login", post(login_handler))
    src/api/routes.rs:45      .route("/v2/login", post(login_handler))
    src/middleware/auth.rs:67  let token = login_handler(req).await?;
    tests/auth_test.rs:12     use crate::auth::login_handler;
    tests/auth_test.rs:34     let result = login_handler(mock_req).await;
    tests/auth_test.rs:56     let result = login_handler(bad_req).await;
    tests/integration.rs:89   use super::login_handler;
```

### Implementation

This is the most substantial new feature. It requires a **usage index** that
doesn't exist yet. Two approaches:

**Approach A: Tree-sitter identifier scanning (recommended)**

At index time, after extracting semantic chunks, also scan for identifier
usages within each chunk. For each chunk, find all identifier tokens that
match a known definition name.

```rust
// New data stored in LanceDB (or a separate table)
pub struct SymbolRef {
    pub symbol_name: String,       // e.g., "login_handler"
    pub definition_chunk_id: String, // Chunk where it's defined
    pub usage_file: String,        // File containing the usage
    pub usage_line: u32,           // Line of usage
    pub usage_context: String,     // The line of code
}
```

**Index-time process:**
1. First pass: Extract definitions (already done -- these are our named chunks)
2. Build a symbol table: `HashMap<String, ChunkId>` of all definition names
3. Second pass: For each chunk, walk the Tree-sitter AST looking for
   `identifier` nodes whose text matches a known definition
4. Store refs in a new LanceDB table `refs` or extend the chunks table

**Approach B: Regex-based scanning (simpler, less accurate)**

Skip the second Tree-sitter pass. Instead, for each known definition name,
do an FTS search to find chunks that contain that identifier. This is faster
to implement but produces false positives (the name appearing in comments,
strings, or unrelated contexts).

**Recommendation**: Start with Approach B (FTS-based) for a quick first version.
It covers 80% of use cases. Upgrade to Approach A (Tree-sitter) when accuracy
matters.

**Storage**: Either:
- New `refs` table in LanceDB with columns: `symbol_name`, `def_chunk_id`,
  `usage_file`, `usage_line`, `usage_context`
- Or: compute refs on-the-fly using FTS (Approach B)

**New code needed**:
- `src/cli/refs.rs` -- CLI command
- `src/analysis/refs.rs` -- Symbol reference resolution
- Possibly `src/storage/lance.rs` -- New `refs` table (if pre-computed)
- `src/mcp/server.rs` -- Add `refs` MCP tool

**Existing code reused**:
- `VectorStore::search_fts()` for Approach B
- `VectorStore::get_chunks_for_file()` for listing symbols in a file
- `Parser` for Approach A (re-parse for identifier nodes)
- Chunk data (name, chunk_type) as the definition index

### Complexity

Medium (Approach B / FTS-based) to High (Approach A / Tree-sitter scanning).

**Recommendation**: Ship Approach B first. It provides immediate value for
AI agents and the `--definition` flag. Approach A can be a follow-up that
improves accuracy.

---

## Implementation Order

Recommended sequencing based on value, dependencies, and complexity:

```
1. similar      -- Low complexity, reuses search infra, immediate value
2. hotspots     -- Medium complexity, standalone, great for code health
3. review       -- Medium complexity, refactors context assembler, high AI value
4. impact       -- Medium complexity, benefits from graph (when it lands)
5. refs         -- Higher complexity, benefits from all the above
```

Features 1-3 can be worked in parallel by different polecats.
Feature 4 benefits from `bobbin-graph` landing first.
Feature 5 is the capstone that ties everything together.

## New Module Structure

```
src/
├── analysis/           # NEW: Analysis features
│   ├── mod.rs
│   ├── similar.rs      # Clone detection logic
│   ├── complexity.rs   # AST complexity metrics (for hotspots)
│   ├── impact.rs       # Multi-signal impact analysis
│   └── refs.rs         # Symbol cross-reference resolution
│
├── cli/
│   ├── similar.rs      # NEW
│   ├── hotspots.rs     # NEW
│   ├── review.rs       # NEW
│   ├── impact.rs       # NEW
│   └── refs.rs         # NEW
│
└── search/
    ├── review.rs       # NEW: Diff analysis + seed extraction
    └── context.rs      # MODIFIED: Extract assemble_from_seeds()
```

## MCP Tools Added

| Tool | Description |
|------|-------------|
| `similar` | Find semantically similar code chunks |
| `hotspots` | Identify high-churn, high-complexity code |
| `review` | Assemble context for a git diff |
| `impact` | Predict change impact across the codebase |
| `refs` | Find symbol definitions and usages |
