# Agent Instructions

This project uses **bd** (beads) for issue tracking. Run `bd onboard` to get started.

## Quick Reference

```bash
bd ready              # Find available work
bd show <id>          # View issue details
bd update <id> --status in_progress  # Claim work
bd close <id>         # Complete work
bd sync               # Sync with git
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

# Agent Development Notes

## Tambour Tenets

1. **Tambour enables workflows, it doesn't impose them.**
   The harness is agnostic to how you organize your work. It picks the next ready task by priority - no special filtering, no hardcoded labels. If you want to focus on a specific label, use `--label`. Your workflow, your rules.

2. **Tambour is distinct from any specific project.**
   It emerged from bobbin development but doesn't know or care about bobbin. It orchestrates agents working on beads issues - that's it.

3. **Tambour will eventually be extracted.**
   It lives here temporarily while the interface stabilizes. When it needs to orchestrate agents across multiple repositories, it becomes its own project.

---

## Tambour: Temporary Home

The `scripts/` directory and `justfile` contain **tambour** - an agent harness for beads. This code lives here temporarily but will eventually become its own module/project.

### What Belongs to Tambour (not Bobbin)

```
scripts/
├── start-agent.sh      # Agent spawning with worktree isolation
├── finish-agent.sh     # Cleanup and merge workflow
└── health-check.sh     # Zombie task detection

justfile                # Tambour recipes (agent, health, finish, etc.)
docs/tambour.md         # Tambour vision and design
```

### Why It's Here

Tambour emerged organically while setting up multi-agent workflows for bobbin development. Rather than prematurely extract it, we're letting it grow here until:

1. The interface stabilizes
2. The scope becomes clear
3. Other projects need it

### Future: Tambour as Standalone

Tambour will eventually be a harness that coordinates:
- **Beads** - Issue tracking and task assignment
- **Bobbin** - Semantic code indexing for agent context
- **Agents** - Claude instances working in isolated worktrees

The extraction will happen when tambour needs to orchestrate agents across multiple repositories, not just bobbin.

### What Belongs to Bobbin

Everything else - the Rust codebase for semantic code indexing:
- `src/` - Core indexing engine
- `Cargo.toml` - Rust dependencies
- `docs/architecture.md` - Bobbin design
- CLI commands: `bobbin index`, `bobbin search`, etc.
