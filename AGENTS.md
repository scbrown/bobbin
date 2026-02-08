# Bobbin - Agent Instructions

## Project Overview

Bobbin is a semantic code indexing tool written in Rust. It indexes codebases for semantic search using embeddings stored in LanceDB.

## Session Management

**In worktrees**: Session notes are set **automatically** from the issue title when a Claude session starts in a worktree. No action needed.

**In main repo**: Set a session note manually using `/note <summary>` (3 words or less):

```
/note fix parser bug
```

This note appears in the status line. Examples: "fix parser bug", "add tests", "refactor indexer"

## Agent Workflow

This project uses [beads](https://github.com/steveyegge/beads) for issue tracking.

### Finding and Starting Work

```bash
bd ready              # Find available work (no blockers)
bd show <id>          # View issue details
bd update <id> --status in_progress  # Claim work
```

### Working on Tasks

If you're in a worktree (working on a task):
```bash
git branch --show-current  # Shows the issue ID (e.g., bobbin-j0x)
bd show $(git branch --show-current)  # Shows full issue details
```

**Do NOT start working on code changes directly in main** - use the worktree workflow instead.

### Finishing Work

```bash
bd close <id>         # Mark issue complete
```

## Development Guidelines

See [CONTRIBUTING.md](CONTRIBUTING.md) for development setup, build commands, and the **Feature Integration Checklist** (important: review the `context` command when adding new features).

Task specs for planned work live in `docs/tasks/`.

## Build Commands

**Always use `just` instead of raw `cargo` commands.** The justfile is configured with quiet output by default to save context - you only see errors and warnings, not compilation progress.

```bash
just build           # Build (quiet output)
just test            # Run tests (quiet output)
just check           # Type check (quiet output)
just lint            # Lint with clippy (quiet output)
```

For verbose output when debugging build issues:
```bash
just build verbose=true
just test verbose=true
```

## Context Budget Management

**Monitor your context usage.** If you drop below 50% context remaining:

1. **Stop expanding scope** - finish what you can with remaining context
2. **Create a blocker issue** for any unfinished subtask:
   ```bash
   bd create --title "Unfinished: <brief title>" --type task --blocks <current-issue>
   ```
3. **Document handoff context** in the new issue's description
4. **Land the plane** - commit, push, and close/pause current work
5. **Wait for input** - let the human decide next steps

### Creating Spinoff Issues

When you discover issues, improvements, or subtasks during work, **create issues immediately** rather than trying to address everything in one session. This keeps work focused and prevents context exhaustion.

**When to create a new issue:**
- You spot a bug unrelated to current work → create with `bug` type and appropriate priority
- You think of an improvement → create with `feature` or `chore` type
- Current task is bigger than expected → split off a blocker issue
- You notice tech debt → create with `chore` type and `tech-debt` label
- Tests reveal other failures → create issues for each distinct problem

**Issue creation command:**
```bash
bd create --title "Title" --type bug|feature|task|chore --priority P0-P4 --labels label1,label2
```

**Default issue template** (fill in relevant sections):
```
## Summary
One-line description of what needs to happen.

## Context
Why this matters. Link to parent issue if relevant.

## Acceptance Criteria
- [ ] Criterion 1
- [ ] Criterion 2

## Notes
Any implementation hints, gotchas, or constraints discovered.
```

### Priority Guidelines

- **P0**: System down, data loss, security vulnerability
- **P1**: Major feature broken, blocking other work
- **P2**: Normal priority (default)
- **P3**: Nice to have, minor improvements
- **P4**: Backlog, someday/maybe

### Common Labels

- `bug` - Something is broken
- `tech-debt` - Cleanup, refactoring
- `enhancement` - Improvement to existing feature
- `docs` - Documentation work

## Landing the Plane (Session Completion)

**When ending a work session**, you MUST complete ALL steps below. Work is NOT complete until `git push` succeeds.

**MANDATORY WORKFLOW:**

1. **File issues for remaining work** - Create issues for anything that needs follow-up
2. **Run quality gates** (if code changed) - Tests, linters, builds
3. **Update issue status** - Close finished work, update in-progress items
4. **Commit and push**:
   ```bash
   git add <files>
   git commit -m "<type>: <description> (<issue-id>)"
   git push
   ```
5. **Clean up** - Clear stashes, prune remote branches
6. **Verify** - All changes committed AND pushed
7. **Hand off** - Provide context for next session

**CRITICAL RULES:**
- Work is NOT complete until `git push` succeeds
- NEVER stop before pushing - that leaves work stranded locally
- NEVER say "ready to push when you are" - YOU must push
- If push fails, resolve and retry until it succeeds
