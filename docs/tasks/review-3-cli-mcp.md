# Task: `bobbin review` CLI Command + MCP Tool

## Summary

Wire diff-based context assembly into a CLI subcommand. This is the killer feature for AI-assisted code review: given a git diff, automatically assemble the surrounding context needed to understand the changes.

## Files

- `src/cli/review.rs` (new) -- CLI command
- `src/cli/mod.rs` (modify) -- register subcommand
- `src/mcp/server.rs` (modify) -- add `review` MCP tool

## CLI Interface

```bash
# Context for uncommitted changes
bobbin review

# Context for staged changes only
bobbin review --staged

# Context for a branch vs main
bobbin review --branch feature/auth

# Context for a specific commit range
bobbin review HEAD~3..HEAD

# Larger budget
bobbin review --budget 1000 --branch feature/auth
```

### Flags

| Flag | Short | Default | Description |
|------|-------|---------|-------------|
| `--branch` | `-b` | | Compare branch against main |
| `--staged` | | `false` | Only staged changes |
| `--budget` | | `500` | Max context lines |
| `--depth` | `-d` | `1` | Coupling expansion depth |
| `--content` | `-c` | `preview` | Content mode: full, preview, none |
| `--repo` | `-r` | all | Filter coupled files to repo |
| `--json` | | `false` | JSON output |

### Positional Argument

An optional commit range (e.g., `HEAD~3..HEAD`) as the first positional argument. If not provided and no `--branch`/`--staged`, defaults to unstaged changes.

## Implementation

### Command Flow

1. Parse diff spec from args → `DiffSpec`
2. Call `git_analyzer.get_diff_files(&spec)`
3. Call `map_diff_to_chunks(&diff_files, &vector_store, repo)`
4. Call `assembler.assemble_from_seeds(seeds, &config, repo)`
5. Format and display the `ContextBundle`

### Output Format

Same format as `bobbin context`, with annotations for changed files:

```
Review context for 3 changed files (branch: feature/auth)
  12 files, 24 chunks (387/500 lines)

--- src/auth.rs [changed: +45 -12] ---
  login_handler (function), lines 45-82
    pub async fn login_handler(req: LoginRequest) -> Result<Token> {
    ...

--- src/session.rs [coupled via git, score: 0.82] ---
  create_session (function), lines 12-34
    ...
```

Changed files are marked with `[changed: +N -M]`. Coupled files show `[coupled via git, score: X.XX]`.

### MCP Tool

Add `review` tool to MCP server. Accepts same parameters, returns JSON `ContextBundle`.

## Dependencies

- Requires `review-1-diff-parsing` and `review-2-assembler-refactor`

## Tests

- Integration test: create a diff, run `bobbin review`, verify output includes changed files
- Verify `--staged` only considers staged changes
- Verify `--branch` compares against main
- Verify `--json` produces valid JSON
- Verify coupled files appear in output when depth > 0

## Acceptance Criteria

- [ ] `bobbin review` works for unstaged, staged, branch, and range diffs
- [ ] Output format matches design (changed file annotations)
- [ ] Coupled files included via expansion
- [ ] `--budget`, `--depth`, `--content` flags work
- [ ] `--json` produces valid JSON
- [ ] MCP `review` tool registered and functional
- [ ] Help text is clear
