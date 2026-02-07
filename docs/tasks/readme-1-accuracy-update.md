# Task: Fix README accuracy - document all features and commands

## Summary

The README is significantly out of date. Three commands are completely undocumented, the language table is wrong, and several shipped features aren't mentioned. Fix all factual gaps before polishing.

## File

`README.md`

## What's wrong

### Undocumented commands

**`bobbin related <FILE>`** - completely missing:
- Finds temporally coupled files via git history
- `--limit`, `--threshold` flags
- Outputs coupling scores and co-change counts
- JSON support

**`bobbin history <FILE>`** - completely missing:
- Shows commit history for a file
- Extracts issue references from commit messages
- Calculates churn rate (commits/month)
- Author statistics
- `-n/--limit` flag, JSON support

**`bobbin serve`** - completely missing:
- Starts MCP server for AI agent integration
- Exposes tools: search, grep, related, read_chunk
- Repository filtering across multi-repo indices

### Language support table is wrong

README says only: Rust, TypeScript, Python, Markdown

Actual support (from `src/index/parser.rs`):

| Language | Extensions | Status |
|----------|------------|--------|
| Rust | .rs | Full AST extraction |
| TypeScript | .ts, .tsx | Full AST extraction |
| JavaScript | .js, .jsx, .mjs | Detection + chunking |
| Python | .py | Full AST extraction |
| Go | .go | Full AST extraction |
| Java | .java | Full AST extraction |
| C++ | .cpp, .cc, .hpp | Full AST extraction |
| C | .c, .h | Line-based chunking |
| Markdown | .md | Semantic chunking (sections, tables, code blocks, frontmatter) |

### Stale roadmap

- "Multi-repo support" marked unchecked - **actually shipped** (bobbin-cmb.2)
- Contextual embeddings not listed - **shipped** (bobbin-cmb.4)
- Markdown semantic chunking not listed - **shipped** (bobbin-cmb.3)

### Undocumented features

- Multi-repo indexing (`bobbin index --repo <name>`, `--repo` flag on search/grep)
- Contextual embeddings (configurable context window for better retrieval)
- Markdown semantic chunking (heading-based sections, tables, code blocks, frontmatter)
- MCP server capabilities and tools

### Configuration gaps

Not documented in config section:
- `context_lines` and `enabled_languages` for contextual embeddings
- `--repo` flag usage across commands

## Acceptance Criteria

- [ ] All 8 commands documented (init, index, search, grep, related, history, status, serve)
- [ ] Language table matches actual parser support
- [ ] Roadmap reflects shipped features
- [ ] Multi-repo, contextual embeddings, markdown chunking documented
- [ ] Config section includes new options
