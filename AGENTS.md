# Bobbin - Agent Instructions

## Project Overview

Bobbin is a semantic code indexing tool written in Rust. It indexes codebases for semantic search using embeddings stored in LanceDB.

## Session Management

**At the start of each work session**, set a 3-word-or-less task summary using `/note`:

```
/note fix parser bug
```

Example summaries: "fix parser bug", "add tests", "refactor indexer"

This note appears in the status line to help track session context.

## Agent Workflow

This project uses [beads](https://github.com/steveyegge/beads) for issue tracking and **tambour** as an agent harness for worktree isolation.

### Finding and Starting Work

To start working on a task (via tambour harness):
```bash
just tambour agent                  # Auto-picks next ready task by priority
just tambour agent-for bobbin-xx    # Work on specific issue
just tambour agent-label <label>    # Filter by label
```

Or manually with native beads commands:
```bash
bd ready              # Find available work
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

To finish and merge (via tambour):
```bash
just tambour finish bobbin-xx
```

To abort/cancel (if started by mistake):
```bash
just tambour abort bobbin-xx
```

Manual completion (if not using tambour harness):
```bash
bd close <id>         # Complete work
```

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
   just tambour spinoff "Brief title" --blocks $(git branch --show-current)
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
just tambour spinoff "Title" [--type bug|feature|task|chore] [--priority P0-P4] [--labels label1,label2] [--blocks issue-id]
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
- `tambour` - Related to agent harness

## Landing the Plane (Session Completion)

**When ending a work session**, you MUST complete ALL steps below. Work is NOT complete until `git push` succeeds.

**MANDATORY WORKFLOW:**

1. **File issues for remaining work** - Create issues for anything that needs follow-up
2. **Run quality gates** (if code changed) - Tests, linters, builds
3. **Update issue status** - Close finished work, update in-progress items
4. **FINISH AND PUSH** - Use tambour to merge and sync:
   ```bash
   just tambour finish <issue-id>
   ```
5. **Clean up** - Clear stashes, prune remote branches
6. **Verify** - All changes committed AND pushed
7. **Hand off** - Provide context for next session

**CRITICAL RULES:**
- Work is NOT complete until `git push` succeeds
- NEVER stop before pushing - that leaves work stranded locally
- NEVER say "ready to push when you are" - YOU must push
- If push fails, resolve and retry until it succeeds

## Tambour Development Notes

The `scripts/` directory and `justfile` contain **tambour** - an agent harness for beads. This code lives here temporarily but will eventually become its own module/project.

### Running Tambour Tests

**DO NOT use system Python or try to install pytest globally.** macOS requires a virtual environment.

```bash
cd tambour

# If .venv doesn't exist, create it first:
python3 -m venv .venv
source .venv/bin/activate
pip install -e ".[dev]"

# If .venv exists, just activate and run:
source .venv/bin/activate
python -m pytest tests/ -v
```

**One-liner (always works):**
```bash
cd tambour && source .venv/bin/activate && python -m pytest tests/ -v
```

**Common mistakes to avoid:**
- `python -m pytest` won't work - use `python3` or activate the venv first
- `pip install pytest` will fail with "externally-managed-environment" error
- Always activate the venv before running tests

### Tambour Tenets

1. **Tambour enables workflows, it doesn't impose them.**
   The harness is agnostic to how you organize your work. It picks the next ready task by priority - no special filtering, no hardcoded labels. If you want to focus on a specific label, use `--label`. Your workflow, your rules.

2. **Tambour is distinct from any specific project.**
   It emerged from bobbin development but doesn't know or care about bobbin. It orchestrates agents working on beads issues - that's it.

3. **Tambour will eventually be extracted.**
   It lives here temporarily while the interface stabilizes. When it needs to orchestrate agents across multiple repositories, it becomes its own project.