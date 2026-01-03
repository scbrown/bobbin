# Bobbin - Agent Instructions

## Project Overview

Bobbin is a semantic code indexing tool written in Rust. It indexes codebases for semantic search using embeddings stored in LanceDB.

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

```bash
cargo build           # Build the project
cargo test            # Run tests
cargo check           # Type check without building
cargo clippy          # Lint
```

## Landing the Plane (Session Completion)

**When ending a work session**, you MUST complete ALL steps below. Work is NOT complete until `git push` succeeds.

**MANDATORY WORKFLOW:**

1. **File issues for remaining work** - Create issues for anything that needs follow-up
2. **Run quality gates** (if code changed) - Tests, linters, builds
3. **Update issue status** - Close finished work, update in-progress items
4. **PUSH TO REMOTE** - This is MANDATORY:
   ```bash
   git pull --rebase
   bd sync
   git push
   git status  # MUST show "up to date with origin"
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

### Tambour Tenets

1. **Tambour enables workflows, it doesn't impose them.**
   The harness is agnostic to how you organize your work. It picks the next ready task by priority - no special filtering, no hardcoded labels. If you want to focus on a specific label, use `--label`. Your workflow, your rules.

2. **Tambour is distinct from any specific project.**
   It emerged from bobbin development but doesn't know or care about bobbin. It orchestrates agents working on beads issues - that's it.

3. **Tambour will eventually be extracted.**
   It lives here temporarily while the interface stabilizes. When it needs to orchestrate agents across multiple repositories, it becomes its own project.