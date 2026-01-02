# Bobbin - Agent Instructions

## Working on Tasks

This project uses [beads](https://github.com/steveyegge/beads) for issue tracking with git worktrees for agent isolation.

### If you're in a worktree (working on a task)

Check which issue you're working on:
```bash
git branch --show-current  # Shows the issue ID (e.g., bobbin-j0x)
bd show $(git branch --show-current)  # Shows full issue details
```

When your work is complete:
1. Commit your changes
2. Inform the user the task is ready for review/merge

### If you're in the main repo

Use `bd ready` to see available tasks. Do NOT start working on code changes directly in main - use the worktree workflow instead.

To start working on a task (via tambour harness):
```bash
just tambour agent                  # Auto-picks next ready task by priority
just tambour agent-for bobbin-xx    # Work on specific issue
just tambour agent-label <label>    # Filter by label
```

Or manually with native beads commands:
```bash
bd worktree create ../bobbin-worktrees/bobbin-xx --branch bobbin-xx
cd ../bobbin-worktrees/bobbin-xx
bd update bobbin-xx --status in_progress
```

To finish and merge:
```bash
just tambour finish bobbin-xx
```

To abort/cancel (if started by mistake):
```bash
just tambour abort bobbin-xx
```

## Project Overview

Bobbin is a semantic code indexing tool written in Rust. It indexes codebases for semantic search using embeddings stored in LanceDB.

## Build Commands

```bash
cargo build           # Build the project
cargo test            # Run tests
cargo check           # Type check without building
cargo clippy          # Lint
```
