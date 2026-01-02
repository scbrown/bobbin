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
