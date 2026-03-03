# Advanced Query Syntax

**Status**: Design
**Author**: ian (PO)
**Date**: 2026-03-03
**Origin**: Stiwi directive via IRC — "I want a design for complex queries and adding support for filtering inline with the query"

## Current State

Bobbin search today is plain-text only:
- `q=some words` → hybrid semantic + FTS
- Filters are separate query params: `?repo=aegis&group=infra&type=function&tag=foo`
- No boolean operators, no inline filters, no quoted phrases, no wildcards

## Design Goals

1. Users can express complex queries in a single search string
2. Inline field filters (`repo:aegis lang:rust`) mixed with search terms
3. Boolean operators for combining conditions
4. Quoted phrases for exact matching
5. Standard features people expect from a search tool

## Industry Survey

What code search tools expose in their query string:

| Feature | GitHub | Sourcegraph | Elasticsearch | Bobbin (proposed) |
|---------|--------|-------------|---------------|-------------------|
| Boolean AND/OR/NOT | yes | yes | yes | **yes** |
| `field:value` filters | yes | yes | yes | **yes** |
| Quoted phrases | yes | yes | yes | **yes** |
| Wildcards | glob in path: | regex | `?` `*` | **yes** (glob) |
| Regex | `/pattern/` | native mode | `/pattern/` | **yes** |
| Negation prefix | `-qualifier:val` | `-filter:val` | `-field:val` or NOT | **yes** |
| Fuzzy/typo | no | no | `term~N` | **deferred** |
| Proximity | no | structural | `"a b"~N` | **deferred** |
| Boosting | no | no | `term^N` | **deferred** |

## Query Language Specification

### 1. Basic Terms

Bare words are search terms, combined with implicit AND:

```
context assembler          # finds chunks containing both "context" AND "assembler"
```

### 2. Quoted Phrases

Double quotes match exact phrases:

```
"context assembler"        # exact phrase match
"PostToolUse handler"      # exact phrase
```

### 3. Boolean Operators

Standard boolean operators (case-insensitive):

```
context AND assembler      # explicit AND (same as implicit)
context OR assembler       # either term
NOT assembler              # exclude term
context AND NOT assembler  # context without assembler
(redis OR memcached) AND cache   # grouping with parens
```

Shorthand with `+` and `-` prefixes:

```
+context -assembler        # must have context, must not have assembler
```

### 4. Inline Field Filters

`field:value` syntax, inspired by GitHub/Sourcegraph:

| Filter | Example | Description |
|--------|---------|-------------|
| `repo:` | `repo:aegis` | Filter by repository name |
| `lang:` | `lang:rust` | Filter by language |
| `type:` | `type:function` | Filter by chunk type (function, section, class, etc.) |
| `file:` | `file:handler` | Filter by file path (substring match) |
| `path:` | `path:src/http` | Filter by file path (substring match, alias for file:) |
| `group:` | `group:infra` | Filter by named repo group |
| `tag:` | `tag:reviewed` | Filter by tag |

Quoted values for spaces:

```
repo:"gas town"            # repo name with spaces
file:"src/cli/hook.rs"     # exact file path
```

Negation with `-`:

```
-repo:aegis                # exclude aegis repo
-lang:markdown             # exclude markdown files
-type:section              # exclude section chunks
```

Multiple values with comma:

```
lang:rust,go               # rust OR go
repo:aegis,bobbin          # aegis OR bobbin
```

### 5. Wildcards

Glob-style wildcards in filter values:

```
file:*.rs                  # all Rust files
path:src/cli/*             # files in src/cli/
file:*_test.go             # Go test files
```

### 6. Regex

Slash-delimited regex in search terms:

```
/fn\s+\w+_handler/         # function names ending in _handler
/impl.*Display/            # Display implementations
```

### 7. Combined Examples

Real-world queries showing feature composition:

```
# Find error handling in Rust files in the aegis repo
repo:aegis lang:rust "error handling"

# Find all config structs, excluding test files
type:struct config -file:*test*

# Find PostToolUse references in either bobbin or aegis
(repo:bobbin OR repo:aegis) PostToolUse

# Find function definitions related to search
type:function lang:rust search query

# Find markdown docs about deployment, not in book/
lang:markdown deployment -path:book/
```

## Implementation Plan

### Phase 1: Query Parser (P1)

Build a query parser that extracts structured components from the query string.

**Input**: Raw query string
**Output**: Parsed query struct:

```rust
struct ParsedQuery {
    /// Free-text search terms (after filter extraction)
    terms: Vec<Term>,
    /// Inline field filters
    filters: Vec<Filter>,
    /// Whether to use phrase matching
    phrases: Vec<String>,
}

enum Term {
    Word(String),
    Phrase(String),
    Not(Box<Term>),
    And(Box<Term>, Box<Term>),
    Or(Box<Term>, Box<Term>),
    Regex(String),
}

struct Filter {
    field: FilterField,
    value: String,
    negated: bool,
}

enum FilterField {
    Repo,
    Lang,
    Type,
    File,
    Path,
    Group,
    Tag,
}
```

**Approach**: Hand-written recursive descent parser. No need for a parser combinator library — the grammar is simple enough. Elasticsearch's `simple_query_string` is a good model: parse what we can, treat unparseable input as literal search terms (never error on user input).

### Phase 2: Filter-to-SQL Translation (P1)

Convert parsed filters into LanceDB `only_if` SQL clauses:

```rust
fn filters_to_sql(filters: &[Filter]) -> Option<String> {
    // repo:aegis → repo = 'aegis'
    // lang:rust,go → language IN ('rust', 'go')
    // -repo:test → repo != 'test'
    // file:*.rs → file_path LIKE '%.rs'
    // path:src/cli → file_path LIKE '%src/cli%'
}
```

This composes with existing §69 role filtering and group filtering.

### Phase 3: Boolean Query Execution (P2)

For boolean operators in search terms:

- **AND**: Already implicit in FTS. For hybrid, intersect result sets.
- **OR**: Run separate queries and merge/dedup results by score.
- **NOT**: Add exclusion terms to FTS query or post-filter.
- **Phrases**: Use FTS phrase matching (LanceDB supports this natively).

### Phase 4: UI Integration (P2)

Update the web UI at search.svc:

- Search bar with syntax highlighting for filters
- Filter chips that can be clicked to add/remove
- Autocomplete for filter keys (`repo:`, `lang:`, etc.)
- Autocomplete for filter values (repo names, languages from index)
- Help tooltip showing query syntax

### Phase 5: Regex Support (P3)

For `/pattern/` in search terms:

- Extract regex, compile, apply as post-filter on content
- Performance: run FTS first for candidate set, then regex filter
- Limit regex complexity (timeout, max backtracking)

## API Changes

### Backward Compatible

The `q` parameter continues to work as before for plain text. Advanced syntax is purely additive — a query without any special syntax behaves identically to today.

### New Response Fields

```json
{
  "query": "repo:aegis lang:rust PostToolUse",
  "parsed": {
    "terms": ["PostToolUse"],
    "filters": [
      {"field": "repo", "value": "aegis"},
      {"field": "lang", "value": "rust"}
    ]
  },
  "mode": "hybrid",
  "count": 5,
  "results": [...]
}
```

### New Endpoint: `/suggest`

```
GET /suggest?q=repo:&prefix=aeg
→ ["aegis"]

GET /suggest?q=lang:&prefix=ru
→ ["rust", "ruby"]
```

Returns completions for filter values based on indexed data.

## CLI Changes

The `bobbin search` command gets the same syntax:

```bash
bobbin search 'repo:aegis lang:rust "error handling"'
bobbin search 'type:function PostToolUse'
```

## Edge Cases

1. **Ambiguous `field:value`** — If someone searches for `http:server`, is that a filter or a search term? Answer: only recognize known filter fields. Unknown `field:value` is treated as a literal search term.

2. **Unbalanced quotes** — Treat unmatched `"` as a literal character, don't error.

3. **Empty filter values** — `repo:` with no value is ignored, treated as literal text.

4. **Case sensitivity** — Filter field names are case-insensitive (`Repo:` = `repo:`). Filter values are case-insensitive for `lang:` and `type:`, case-sensitive for `repo:`, `file:`, `path:`.

5. **Special characters in values** — Quote the value: `file:"path with spaces/file.rs"`.

## Rejected Alternatives

1. **Separate filter API only (Algolia/Meilisearch model)** — Stiwi specifically asked for inline filtering in the query string. The separate `?repo=` params continue to work for programmatic access.

2. **Full Lucene query syntax** — Too complex, error-prone for casual users. We follow GitHub/Sourcegraph's simpler model.

3. **Custom DSL** — Unnecessary when the `field:value` convention is universally understood.

## Dependencies

- LanceDB FTS already supports phrase queries
- LanceDB `only_if` SQL filter already supports all the SQL we need
- No new external dependencies required

## Beads

| ID | Title | Priority | Phase |
|----|-------|----------|-------|
| TBD | Query parser: extract filters, phrases, booleans from query string | P1 | 1 |
| TBD | Filter-to-SQL: translate parsed filters to LanceDB clauses | P1 | 2 |
| TBD | Boolean query execution: AND/OR/NOT for search terms | P2 | 3 |
| TBD | UI: search syntax highlighting and filter autocomplete | P2 | 4 |
| TBD | Regex support in search queries | P3 | 5 |
| TBD | /suggest endpoint for filter value autocomplete | P2 | 4 |
