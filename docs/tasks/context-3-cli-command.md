# Task: Create CLI Command for `bobbin context`

## Summary

Wire the context assembler into bobbin's CLI as `bobbin context <QUERY>`.

## Files

- `src/cli/context.rs` (new)
- `src/cli/mod.rs` (modify)

## Implementation

### `src/cli/context.rs`

Follow the exact pattern of `src/cli/search.rs`:

```rust
#[derive(Args)]
pub struct ContextArgs {
    /// Natural language description of the task
    query: String,

    /// Maximum lines of content to include
    #[arg(long, short = 'b', default_value = "500")]
    budget: usize,

    /// Content mode: full, preview, none
    #[arg(long, short = 'c')]
    content: Option<ContentMode>,

    /// Coupling expansion depth (0 = no coupling)
    #[arg(long, short = 'd', default_value = "1")]
    depth: u32,

    /// Max coupled files per seed file
    #[arg(long, default_value = "3")]
    max_coupled: usize,

    /// Max initial search results
    #[arg(long, short = 'n', default_value = "20")]
    limit: usize,

    /// Min coupling score threshold
    #[arg(long, default_value = "0.1")]
    coupling_threshold: f32,

    /// Filter to specific repository
    #[arg(long, short = 'r')]
    repo: Option<String>,

    /// Directory to search in
    #[arg(default_value = ".")]
    path: PathBuf,
}
```

`run()` function:
1. Find repo root, load config (same as search.rs)
2. Open VectorStore, MetadataStore, load Embedder
3. Check empty index, model consistency (same as search.rs)
4. Build `ContextConfig` from args
5. Call `ContextAssembler::new(...).assemble(&query).await?`
6. Format output (JSON or human-readable)

### Content mode default logic

- If `--json` is set and no explicit `--content`: default to `Full`
- If no `--json` and no explicit `--content`: default to `Preview`
- Explicit `--content` always wins

### Human output format

```
Context for: <query>
  <N> files, <N> chunks (<used>/<budget> lines)

--- <file_path> [direct, score: 0.XX] ---
  <name> (<chunk_type>, lines N-M)
  <3-line preview if content=preview>

--- <file_path> [coupled via <source_file>] ---
  ...
```

Use `colored` crate: file paths in blue, chunk types in magenta, scores dimmed.

### `src/cli/mod.rs`

Add:
- `mod context;`
- `Context(context::ContextArgs)` to `Commands` enum with doc comment `/// Assemble task-relevant context from search and git history`
- Match arm: `Commands::Context(args) => context::run(args, output).await`

## Dependencies

- Requires Task 2 (context assembler module)

## Tests

- Test `ContentMode` clap parsing
- Reuse pattern from `search.rs` tests for serialization

## Acceptance Criteria

- [ ] `bobbin context "query"` works end-to-end
- [ ] `--json` produces valid JSON matching the schema
- [ ] Human output is readable with colors
- [ ] All flags work as documented
- [ ] `bobbin context --help` shows all options
- [ ] `bobbin help` lists the context command
