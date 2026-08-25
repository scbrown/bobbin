# Parenthesised and Nested Boolean Query Grouping

> **Implementation status (2026-08-25):** ⛔ **Not implemented — spec only.**
> This document is the "spec" half of `bobbin-0a5`, which is explicitly
> *spec then build*. Nothing described here exists in `src/search/query.rs`
> yet. The build remains open, and deliberately so: several decisions below
> are marked **OPEN** and want a human answer before any parser is written.

**Status**: Design — awaiting review
**Origin**: `bobbin-0a5`, the last remaining half of
[#50](https://github.com/scbrown/bobbin/issues/50) (closed; its three numbered
items shipped 2026-08-25)
**Supersedes**: the `Term` sketch in
[`docs/plans/advanced-query-syntax.md`](../plans/advanced-query-syntax.md)
§"Phase 1", which was written before the flat parser shipped and does not
account for what it actually does

---

## 1. The problem, precisely

`(redis OR memcached) AND cache` does not mean what it looks like. It
mis-tokenises: `src/search/query.rs`'s `parse_word` treats `(redis` and
`memcached)` as literal words, parens and all, and `split_on_or` — a flat
string scan for `" OR "` — splits the string wherever those four bytes appear,
with no notion of nesting.

That is not a bug in `split_on_or`. It is the honest limit of a design that has
no tree. `ParsedQuery` today is a bag of parallel `Vec`s:

```rust
pub struct ParsedQuery {
    pub terms: Vec<String>,
    pub phrases: Vec<String>,
    pub filters: Vec<Filter>,
    pub negated_terms: Vec<String>,
    pub required_terms: Vec<String>,
    pub text_query: String,
    pub has_or: bool,
    pub or_branches: Vec<String>,
    pub regex_patterns: Vec<String>,
}
```

Every field in it is a *set*, and sets have no shape. `has_or` is a boolean
because there is exactly one place OR can occur: the top level. Grouping means
OR can occur anywhere, which means a boolean cannot describe it, which is why
this is a rewrite of the parser and not a patch to it.

### What "the build is large" actually means

Three things change together, and none of them can land alone:

1. **Parser** — a recursive-descent parser producing a `Node` tree, replacing
   the single-pass scanner.
2. **Planner** — a pass that decides which parts of the tree the store can
   answer (SQL over LanceDB) and which must be answered in memory, and in what
   order. Today this decision is hardcoded: filters go to
   `filters_to_sql`, everything else goes to `retain_matching`.
3. **Executor** — the OR strategy generalises from "run N top-level branches
   and merge by best score" to "evaluate a tree", and the cost model changes
   with it (see §7).

The spec below is organised so those three can be reviewed separately.

---

## 2. Grammar

EBNF, with the lexer's tokens as terminals. Case-insensitive keywords are
written uppercase.

```ebnf
query       = or_expr ;

or_expr     = and_expr , { OR , and_expr } ;
and_expr    = unary   , { [ AND ] , unary } ;      (* juxtaposition = AND *)
unary       = [ NOT | "-" | "+" ] , primary ;
primary     = group | filter | phrase | regex | word ;
group       = "(" , or_expr , ")" ;

filter      = [ "-" | "+" ] , field , ":" , value ;
field       = "repo" | "lang" | "type" | "file" | "path" | "group" | "tag"
            | (* plus the documented aliases *) ;
value       = quoted | bare_value ;                (* commas split inside *)

phrase      = '"' , { char } , '"' ;
regex       = "/" , { char } , "/" , ( WS | EOF ) ;
word        = { char - ( WS | "(" | ")" ) } ;
```

### 2.1 Precedence and associativity

From loosest to tightest:

| Level | Operator | Associativity |
|-------|----------|---------------|
| 1 (loosest) | `OR` | left |
| 2 | `AND`, and implicit AND by juxtaposition | left |
| 3 | `NOT`, `-`, `+` (prefix) | right (binds to one `primary`) |
| 4 (tightest) | `( … )`, phrase, regex, filter, word | — |

So `a OR b c` parses as `a OR (b AND c)`, which is the conventional reading and
matches GitHub, Sourcegraph, Lucene and Elasticsearch. `NOT a OR b` parses as
`(NOT a) OR b`.

**RECOMMENDATION.** Adopt exactly this table. It is what every search tool a
bobbin user has already used does, and the cost of being unusual here is that
people's queries silently mean something else.

**OPEN — mixing `AND` and `OR` without parens.** Some tools (Elasticsearch's
`query_string`, older Lucene) reject or warn on `a AND b OR c` because the
precedence surprises people. Bobbin's standing rule is *never error on user
input*. Options: (a) apply the table silently; (b) apply the table and echo the
effective grouping back in the `parsed` response so the UI can render it (see
§5). **Recommendation: (b)** — it keeps the never-error rule and still closes
the surprise, and the echo is nearly free once the tree exists. Wants Stiwi's
call because it is the only place this spec proposes new response surface for a
readability reason rather than a correctness one.

### 2.2 Never error, still

The parser's existing contract is load-bearing and does not change: *unparseable
input is treated as literal search text; the parser never returns an error.*
Grouping adds exactly one new way to be malformed — an unbalanced paren — and
it is handled the same way an unbalanced quote already is:

- `(redis OR memcached` — unclosed group: the `(` is treated as a literal
  character of the word `(redis`, i.e. **exactly today's behaviour**. This is
  deliberate: the failure mode of a half-typed query in a search box must be
  "found nothing useful", never "the parser reinterpreted my intent".
- `redis) OR memcached` — unmatched `)`: same, literal.
- `()` — empty group: parses to a node that matches everything, and is dropped
  by the planner rather than emitted as an empty SQL clause.

**RECOMMENDATION.** Do *not* attempt error recovery that "helpfully" inserts a
missing paren. A search box is edited character by character; every prefix of a
valid query is briefly invalid, and a parser that guesses will guess during
typing.

### 2.3 Escaping

A literal parenthesis is written inside quotes: `"fn foo()"` is a phrase and its
parens are characters. Outside quotes there is **no backslash escape**, because
there is not one today and adding one changes the meaning of every existing
query containing a backslash — notably regexes.

**OPEN.** Whether bare `\(` should be added later. Recommendation: no; quotes
already cover it.

---

## 3. Composition with what shipped in #50

This is where a grouping spec earns its keep. The four features that landed in
the #50 sweep each interact with grouping differently, and three of the four
have a defensible answer that is *not* "distribute it".

### 3.1 Required terms (`+`)

`+` today means: *this term also drives retrieval, and is then enforced as a
post-filter* (`required_terms` in `retain_matching`). Under grouping, "enforced
as a post-filter" needs a scope.

**RECOMMENDATION.** `+` binds to its `primary` and its enforcement scope is the
enclosing group. `(+redis OR memcached) cache` requires `redis` **of the left
branch only** — a document matching `memcached AND cache` is still a hit. This
is the reading that makes `+` inside a disjunction mean anything at all; the
alternative (global enforcement) makes `(+a OR b)` exactly equivalent to `+a`,
silently deleting the right branch.

Consequence: `required_terms` stops being a flat `Vec<String>` post-filter and
becomes a property of the node. See §5.

**OPEN.** Whether `+` on a *group* — `+(a OR b)` — is accepted. It is
well-defined under the rule above (the whole disjunction must match) and costs
nothing extra in the parser. Recommendation: accept it.

### 3.2 Negation (`-`, `NOT`)

**Does NOT distribute over a group, and is `-(a OR b)` accepted?**

**RECOMMENDATION: yes, accepted, and it distributes by De Morgan.**
`-(a OR b)` ≡ `(-a AND -b)`, and `-(a AND b)` ≡ `(-a OR -b)`.

This is the standard reading and it is also the *only* one that keeps negation
compositional. But it has a real cost that must be stated: today negation is a
cheap post-filter — `retain_matching` drops any result whose content contains a
negated term. `-(a AND b)` under De Morgan becomes a **disjunction of
negations**, which is not expressible as "drop results containing X" and needs
the evaluator to actually evaluate the tree per candidate. That is §7's problem,
not the grammar's, but it is the reason nested negation cannot be bolted onto
the existing executor.

**A hard case, called out because it is the one that bites.** Bobbin's negation
is *content-based* (does the chunk text contain the term) while its filters are
*metadata-based* (SQL over columns). `-(redis OR repo:aegis)` mixes them: it
means "not(content contains redis) AND not(repo = aegis)". Those halves execute
in different places. This is expressible — see §7.3 — but it means the planner
cannot assume a negation lives on one side of the SQL/in-memory line.

**OPEN.** Whether to accept mixed-plane negation at all in v1, or refuse it by
treating the inner filter as a literal term (the never-error fallback).
Recommendation: **accept it**; refusing produces a query that silently searches
for the literal text `repo:aegis`, which is worse than a slower plan.

### 3.3 Regex (`/pattern/`)

Regex is a pure content predicate and composes cleanly as a leaf. `(/fn \w+/ OR
/impl \w+/) repo:aegis` is well-defined.

**RECOMMENDATION.** Regex leaves are always evaluated in memory, always last,
and — unchanged from today — a regex that fails to compile is *skipped*, not an
error. Under grouping, "skipped" needs a definition: a skipped regex leaf
evaluates to **true** (matches everything), so it disappears from an AND and
does not silently empty a disjunction. Today's flat behaviour is equivalent, so
this is a formalisation, not a change.

### 3.4 Field filters — the hard one

> `(repo:a OR repo:b) foo` is a filter disjunction, which `filters_to_sql`
> cannot currently express (it ANDs clauses).

`filters_to_sql` returns `Vec<String>` and every caller joins with `AND`. That
signature *is* the limitation.

**RECOMMENDATION.** Change it to return a single SQL expression string built
from the tree, with explicit parens:

```rust
// today
pub fn filters_to_sql(filters: &[Filter]) -> Vec<String>;

// proposed — the tree, not a list
pub fn filters_to_sql(node: &Node) -> Option<String>;
//   (repo:a OR repo:b) → "(repo = 'a' OR repo = 'b')"
```

`Option` because a subtree with no metadata predicates in it contributes no SQL
at all. Callers that today do `extra_filters.extend(filters_to_sql(...))` and
join with `AND` keep working: they push one already-parenthesised string.

Three specific sub-cases:

- **`group:`** resolves to a *set of repos* (`extract_group_filters` →
  config lookup → `repo IN (...)`). Inside a disjunction this is just another
  parenthesised SQL fragment, so it composes. The current special-casing —
  `group:` is excluded from `filters_to_sql` and handled by the caller — has to
  move into the SQL builder, because the caller no longer sees a flat list to
  intercept. **This is the single largest mechanical change in the build.**
- **An unknown group name** currently produces a `400` from `/search` naming the
  available groups. That must survive: the planner resolves group names *before*
  building SQL, and an unresolvable name is still a `400`, not a silently empty
  branch.
- **`tag:`** maps to a list column; its SQL is already a containment expression
  and parenthesises like the rest.

**OPEN — what a disjunction of filters means for retrieval, not just
filtering.** `(repo:a OR repo:b) foo` is unambiguous: one search for `foo`
scoped by one SQL predicate. But `(repo:a redis OR repo:b memcached)` binds a
*different* text query to each branch. That is a genuine two-query plan, and the
current OR executor already runs branches separately — so it is expressible —
but it multiplies with nesting (§7.2). Recommendation: support it, with the
branch cap in §7.2. Flagging it because it is the case where a user can write
a short query that costs a lot.

---

## 4. Semantics of a group under hybrid search

A thing worth writing down before anyone codes: **bobbin is not a boolean
retrieval engine.** It is hybrid semantic + FTS with RRF fusion and scoring.
"Does this document match the query" is not the question it answers; "how
relevant is this document" is.

So a boolean tree over a ranked retriever has to define what a node *does*:

| Node | Retrieval effect | Filtering effect |
|------|------------------|------------------|
| `word`, `phrase` | contributes to the text query for its branch | none (scoring only) |
| `+word` | contributes to the text query | post-filter within its group |
| `-word` / `NOT word` | removed from the text query | post-filter within its group |
| `/regex/` | none | post-filter, in memory, last |
| `field:value` | none | SQL predicate |
| `OR` | splits into branches, results merged by best score | — |
| `AND` | terms concatenated into one text query | predicates ANDed |

**RECOMMENDATION.** Keep the existing merge rule for `OR` — *a chunk found by
two branches keeps its higher score, and the merged set is re-sorted* — and
apply it at every level of nesting, innermost first. It is already implemented
identically in `src/cli/search.rs::run_or_branches` and
`src/http/handlers/search.rs::execute_or_search`, and it is the rule least
likely to surprise: an OR never scores a document *worse* than its best branch
would have.

**OPEN — should a nested OR sum rather than max?** Summing (a chunk matching
both branches ranks above one matching either) is arguably better relevance and
is what a real boolean-over-BM25 engine does. Max is what bobbin does today.
Recommendation: **keep max for v1** and treat scoring changes as a separate,
measurable piece of work with the eval harness behind it — changing retrieval
quality inside a syntax change makes both unevaluable.

---

## 5. The `parsed` response field

`/search` echoes a `parsed` object, and `src/http/handlers/search.rs` builds it
from `ParsedQuery`'s flat vectors. Grouping breaks that shape: `terms: [...]`
cannot represent `(a OR b) c`.

**RECOMMENDATION — additive, not a replacement.** Keep every current field,
populated exactly as today for queries that contain no parens, and add one:

```json
{
  "parsed": {
    "terms": ["cache"],
    "phrases": [],
    "filters": [{"field": "repo", "values": ["aegis"], "negated": false}],
    "negated_terms": [],
    "required_terms": [],
    "has_or": true,
    "or_branches": ["redis cache", "memcached cache"],
    "regex_patterns": [],

    "tree": {
      "op": "and",
      "children": [
        {"op": "or", "children": [
          {"op": "word", "value": "redis"},
          {"op": "word", "value": "memcached"}
        ]},
        {"op": "word", "value": "cache"}
      ]
    }
  }
}
```

Why additive rather than a clean replacement:

- The filter-chip UI and `/suggest` autocomplete read `filters` and `terms`.
  Replacing the shape breaks a shipped UI in the same change that adds a parser,
  and then a bug in either is a bug in both.
- A client that does not care about grouping should not have to learn a tree.
- The flat fields remain *exactly right* for the flat queries that are the
  overwhelming majority.

**The honesty requirement.** For a query that *does* group, the flat fields are
necessarily lossy — `terms` for `(a OR b) c` cannot say where the OR was. They
must therefore be a flattening that is **true but incomplete**, never one that
is wrong:

- `terms` = every `word` leaf, in source order. True: each is a term in the
  query. Incomplete: their arrangement is gone.
- `has_or` = true if an `or` node appears **anywhere**, not just at the top.
- `or_branches` = the *top-level* branches only, each rendered as text. For
  `(a OR b) c` there is no top-level OR, so this is `[]` while `has_or` is
  `true`. **That combination is currently impossible and clients may be
  assuming it cannot happen.**

**OPEN — that last point is a real compatibility hazard.** Today
`has_or == true` implies `or_branches.len() > 1`, and both executors branch on
exactly that conjunction. Options: (a) let the pair diverge and document it;
(b) add a separate `has_nested_or` and keep `has_or` meaning "top-level OR", so
the existing invariant survives untouched. **Recommendation: (b).** It costs one
field and keeps a shipped contract literally true instead of subtly redefined.

---

## 6. Migration: what happens to queries that work today

**The bar: every query that is valid today must return the same results
tomorrow.** Not "similar" — the same. This is testable, and §8 makes it a test.

| Today's query | Contains parens? | Under the new parser |
|---|---|---|
| `context assembler` | no | identical — one AND node, one text query |
| `redis OR memcached` | no | identical — top-level OR, two branches, same merge |
| `repo:aegis lang:rust foo` | no | identical — filters AND'd, same SQL |
| `+context -assembler` | no | identical — same post-filters, scope is the whole query |
| `/fn \w+_handler/` | no | identical — regex is one leaf, still the only predicate |
| `src/cli/hook.rs` | no | identical — `parse_regex`'s "closing `/` must be followed by whitespace" rule still keeps paths from becoming regexes |
| `fn foo()` | **yes** | **CHANGES — see below** |

### 6.1 The one real break

A query containing a bare `(` or `)` that is *not* meant as grouping.
`fn foo()` today searches for the literal token `foo()`. Under the new grammar,
`()` is an empty group and the query becomes `fn` — different results.

**RECOMMENDATION.** Treat a paren as a grouping token **only when it can open or
close a group in that position**; otherwise it is a literal character of the
word. Concretely:

- `(` opens a group only at the start of a `primary` position — i.e. preceded by
  whitespace, `(`, or start-of-input. In `foo()` the `(` follows `o`, so it is a
  literal.
- `)` closes a group only when a group is open.
- An empty group `()` in a `primary` position is dropped (matches everything).

Under that rule, `fn foo()` is unchanged, `(redis OR memcached) cache` groups,
and `foo(bar OR baz)` is a literal word `foo(bar`, `OR`, `baz)` — the same
nonsense it produces today, which is the correct amount of change for an input
nobody meant.

This rule is not free: it means the lexer is position-sensitive, which the
current one is not. It is worth it. The alternative is that `bobbin search 'fn
foo()'` — an entirely ordinary code search — quietly starts meaning something
else the day this ships.

**OPEN.** Whether the position rule should also require that the matching `)`
exist before treating `(` as an opener (a one-token lookahead vs. a full scan).
Recommendation: full scan for a balanced closer; the queries are short and it
removes a whole class of "half-typed query changed meaning" surprises.

### 6.2 What does not need migrating

Nothing persisted. `ParsedQuery` is computed per request and never stored; there
are no saved queries, no query index, no cached parses. The migration surface is
exactly: the two executors, the `parsed` response, the filter-chip UI, and
`/suggest`. That is why this can be a rewrite rather than a versioned parser.

---

## 7. Execution: SQL versus in memory

### 7.1 The line

| Predicate | Where | Why |
|---|---|---|
| `repo:`, `lang:`, `type:`, `file:`, `path:`, `tag:`, `group:` | **SQL** (`only_if` on LanceDB) | metadata columns; filtering before retrieval is what makes the search cheap |
| text terms, phrases | **retrieval** | they *are* the query; they do not filter, they rank |
| `+required` | **in memory**, post-retrieval | needs the chunk content, which SQL does not have |
| `-negated` / `NOT` on a term | **in memory**, post-retrieval | same |
| `/regex/` | **in memory**, last | most expensive; runs on the smallest candidate set |
| negation of a *filter* (`-repo:x`) | **SQL** | already true today |
| negation of a *mixed group* (§3.2) | **split** — SQL part in SQL, content part in memory, ANDed | the only case that straddles |

**RECOMMENDATION.** The planner performs one rewrite before anything else:
push every metadata predicate as far toward the leaves as possible, collect the
maximal SQL-expressible subtree, and emit it as a single parenthesised
expression. Whatever is left is the in-memory predicate tree, evaluated against
each candidate's content in the order: required → negated → regex (cheapest
first, which is today's order in `retain_matching` and is already correct).

### 7.2 Branch multiplication — the cost that has to be capped

The current OR executor runs **one full search per top-level branch**. Nesting
multiplies: `(a OR b) AND (c OR d)` in the naive lowering is four searches.
Three levels of pairs is eight. A user can write that in twenty characters.

**RECOMMENDATION.** Do **not** lower a tree to a cross product. Instead:

1. **Normalise to DNF only for the SQL predicate**, where the store evaluates it
   in one pass and nesting costs nothing.
2. **For retrieval, split on OR only where the branches carry *different text
   queries*.** `(repo:a OR repo:b) foo` has one text query (`foo`) and one SQL
   predicate — it is **one search**, not two. The existing executor would
   already get this wrong by splitting; the planner is what makes it right.
3. **Cap the number of retrieval branches** (recommend **8**) and, on exceeding
   it, fall back to a single search on the union of the branches' text with the
   full predicate applied as a post-filter. Slower per result but bounded, and
   — the part that matters — **the cap is announced in the response**, the same
   way every other truncation in this codebase is (`omitted_files`,
   `trim_payload`'s `… [+N more]`). A query that quietly returned a degraded
   plan would be the exact defect bobbin keeps writing comments about.

**OPEN.** The cap's value, and whether it is config or constant.
Recommendation: constant 8 to start; make it config only when someone hits it.

### 7.3 Mixed-plane negation, concretely

`-(redis OR repo:aegis)` → De Morgan → `(-redis) AND (-repo:aegis)`:

- `-repo:aegis` → SQL `repo != 'aegis'`
- `-redis` → in-memory content post-filter

Both apply; they simply apply in different phases. The planner splits at the
AND, which is why De Morgan normalisation must run *before* the SQL/in-memory
partition rather than after.

---

## 8. Acceptance criteria for the build

The build is not done when `(a OR b) c` parses. It is done when:

1. **A golden-file regression suite** covers every query in §6's table plus
   every query appearing in `src/search/query.rs`'s existing ~45 tests, and
   asserts the *new* parser produces results identical to the flat parser's.
   This is the migration bar made executable, and it should be written
   **before** the new parser, against the current one.
2. `(redis OR memcached) AND cache`, `-(a OR b)`, `(repo:a OR repo:b) foo`, and
   `+(a OR b)` each have an explicit semantics test.
3. `fn foo()` and `src/cli/hook.rs` are tested as *not* grouping and *not*
   regex respectively — the two ordinary code searches this change could break.
4. Unbalanced parens are tested to fall back to literal text at every position.
5. The branch cap (§7.2) has a test that trips it and asserts the response says
   so.
6. Both surfaces are covered. `bobbin search` and `GET /search` call
   `query::parse` and `query::retain_matching`, so they get the parser for free
   — but "for free" is a claim, and it was false once already (the CLI did not
   call the parser at all until #50). One test per surface, on the same query,
   asserting the same result set.

---

## 9. Summary of open questions for Stiwi

Each of these is a decision the spec cannot make for itself.

| # | Question | Recommendation |
|---|---|---|
| 1 | Echo effective grouping in `parsed` so mixed `AND`/`OR` precedence is visible? (§2.1) | Yes |
| 2 | Accept `+(a OR b)`? (§3.1) | Yes |
| 3 | Accept mixed-plane negation `-(redis OR repo:aegis)`, or fall back to literal? (§3.2) | Accept |
| 4 | Nested OR scoring: max (today) or sum? (§4) | Keep max; measure separately |
| 5 | `has_or` for a *nested* OR — diverge from `or_branches`, or add `has_nested_or`? (§5) | Add `has_nested_or` |
| 6 | Full-scan for a balanced closer before treating `(` as an opener? (§6.1) | Yes |
| 7 | Retrieval branch cap value; constant or config? (§7.2) | 8, constant |
| 8 | Add a `\(` escape? (§2.3) | No |

Question **3** and question **6** are the two that change what existing queries
do. The rest change only what new queries can express.

---

## 10. Not in scope

- **Scoring changes.** See §4's open question. Any change to how a document
  ranks belongs behind the eval harness, not inside a syntax change.
- **Fuzzy, proximity, boosting.** Deferred in the parent design and still
  deferred.
- **`bobbin context`'s query.** Context assembly takes a natural-language task
  description, not a boolean expression. Grouping does not apply and should not
  be plumbed there.
- **The `tests/cli_search.rs` CI gap.** `bobbin-0a5` also notes that the CLI's
  result-returning integration tests self-skip when the ONNX embedding model
  cannot be downloaded, so they were never observed to run during the #50 work.
  That is a real gap and it is **not** fixed by this spec — it wants its own
  bead, because a grouping suite that self-skips proves nothing.
