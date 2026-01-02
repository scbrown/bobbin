# Agent Development Notes

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
