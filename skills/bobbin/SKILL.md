---
name: bobbin
description: >
  Semantic code search and codebase intelligence. Use this skill when asked to
  "search code", "find where X is defined", "what files are related to Y",
  "show me the impact of changing Z", "find duplicates", "search commits",
  "search beads/issues", "/bobbin <query>", or any codebase exploration that
  goes beyond simple grep. Routes queries to the right bobbin MCP tool.
---

# Bobbin Skill — Semantic Code Search

You are performing a codebase search using bobbin's semantic search engine.
Bobbin indexes code across all repos in the workspace and provides tools far
more powerful than grep: semantic search, symbol references, impact analysis,
commit history, coupling graphs, and more.

## Input

The user provided a query: `{{args}}`

## Routing Decision

Analyze the query and pick the RIGHT tool. Don't default to `search` for
everything — use the most specific tool available.

### Route 1: Symbol Lookup
**Trigger**: Query names a specific function, struct, class, or variable.
Examples: "find parse_config", "where is Config defined", "who calls handle_request"

**Action**: Use `bobbin find_refs` with the symbol name.
- Returns: definition location + all usage sites
- Follow up with `bobbin read_chunk` to show the actual code

### Route 2: Semantic Code Search
**Trigger**: Query describes code by what it does, not by exact name.
Examples: "authentication logic", "error handling with retries", "database connection pooling"

**Action**: Use `bobbin search` with the natural language query.
- Use `mode: "hybrid"` (default) for best results
- Use `type` filter if the user wants a specific kind (function, struct, etc.)
- Use `repo` filter if the user specifies a repo
- Use `bundle` filter if the user specifies a bundle scope

### Route 3: Exact Pattern Search
**Trigger**: Query is a literal string, regex, or exact identifier.
Examples: "TODO FIXME", "fn.*async", grep-style patterns

**Action**: Use `bobbin grep` with the pattern.
- Set `regex: true` if the pattern uses regex syntax
- Set `ignore_case: true` for case-insensitive searches

### Route 4: File Relationships
**Trigger**: Query asks what files are related, coupled, or co-changed.
Examples: "what changes with config.rs", "files related to auth"

**Action**: Use `bobbin related` with the file path.
- Follow up with `bobbin read_chunk` on interesting results

### Route 5: Impact Analysis
**Trigger**: Query asks what would break or be affected by a change.
Examples: "impact of changing auth.rs", "what breaks if I modify the parser"

**Action**: Use `bobbin impact` with the target file or file:function.
- Use `depth: 2` for transitive impact

### Route 6: Commit Search
**Trigger**: Query asks about git history semantically.
Examples: "when was auth added", "commits that changed error handling", "who refactored X"

**Action**: Use `bobbin commit_search` with the query.
- Use `author` filter if specified
- Use `file` filter if specified

### Route 7: Bead/Issue Search
**Trigger**: Query asks about issues, bugs, tasks, or work items.
Examples: "open auth bugs", "P1 issues", "beads about monitoring"

**Action**: Use `bobbin search_beads` with the query.
- Use priority/status/assignee/label filters as appropriate

### Route 8: Context Assembly
**Trigger**: Query asks for comprehensive context for a task or change.
Examples: "context for refactoring auth", "everything related to the deploy pipeline"

**Action**: Use `bobbin context` with the task description.
- Adjust `budget` based on scope (default 500 lines)

### Route 9: File Symbols
**Trigger**: Query asks what's defined in a specific file.
Examples: "symbols in main.rs", "what functions are in config.py", "API of auth module"

**Action**: Use `bobbin list_symbols` with the file path.

### Route 10: Duplicate Detection
**Trigger**: Query asks about duplicate or similar code.
Examples: "find code similar to X", "detect duplicates", "near-duplicate functions"

**Action**: Use `bobbin similar`.
- Single target: provide `target` as `file:function` or free text
- Scan mode: set `scan: true` to find clusters across the codebase

### Route 11: Dependency Analysis
**Trigger**: Query asks about imports, dependencies, or what depends on a file.
Examples: "what imports config.rs", "dependencies of auth module", "reverse deps"

**Action**: Use `bobbin dependencies` with the file path.
- Use `reverse: true` for "what depends on this"
- Use `both: true` for full picture

### Route 12: File History
**Trigger**: Query asks about the history of a specific file.
Examples: "history of config.rs", "who changed auth.py recently", "churn on main.rs"

**Action**: Use `bobbin file_history` with the file path.

### Route 13: Code Hotspots
**Trigger**: Query asks about risky, complex, or high-churn code.
Examples: "hotspots", "what needs refactoring", "riskiest files"

**Action**: Use `bobbin hotspots`.
- Adjust `since` for time window
- Adjust `threshold` to filter noise

### Route 14: Archive Search
**Trigger**: Query asks about chat logs, IRC discussions, or agent memory.
Examples: "what did we discuss about auth", "recent deploy discussions", "telegram messages"

**Action**: Use `bobbin archive_search` with the query.
- Use `source: "hla"` for chat logs, `source: "pensieve"` for agent memory
- Use date filters (`after`, `before`) as appropriate

### Route 15: Review Context
**Trigger**: Query asks for review context on current changes or a diff.
Examples: "review context for my changes", "what do I need to review this PR"

**Action**: Use `bobbin review`.
- Use `diff: "staged"` or `diff: "unstaged"` for working tree
- Use `diff: "branch:feature-x"` for branch comparison

## Output Guidelines

1. **Show the most relevant results first** — don't dump everything
2. **Include file paths and line numbers** so the user can navigate
3. **Read and show actual code** for the top 2-3 results using `read_chunk`
4. **Suggest follow-up queries** if the results are ambiguous or partial
5. **Chain tools** when appropriate — e.g., `search` -> `find_refs` -> `read_chunk`
6. **Be concise** — summarize what you found, don't just paste raw output
