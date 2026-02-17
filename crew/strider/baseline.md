# Observation Baseline

What "healthy" looks like for the Bobbin rig. Strider checks against this during patrol.

_Last verified: 2026-02-17_

## Bobbin System Components

| Component | Status | Notes |
|-----------|--------|-------|
| Rust codebase | Active development | Main language, built with `just` |
| LanceDB | Embedded | Vector storage for semantic search |
| MCP tools | Deployed | `dependencies`, `file_history`, `status`, `commit_search` |
| CLI | Functional | `bobbin index`, `bobbin search`, `bobbin context`, `bobbin log` |
| Eval suite | Growing | Go, pandas, polars task suites |
| mdbook | Scaffolded | Dracula theme + Intel One Mono |

## Rig Inventory

| Role | Agent | Status | Purpose |
|------|-------|--------|---------|
| Ranger | strider | NEW | System advocate (you) |
| Keeper | ian (aegis) | Active | Search & Context strategy |
| Polecats | rust, nitro | Active | Code execution |
| Refinery | bobbin | Active | Merge queue processing |
| Witness | bobbin | Active | Polecat health monitoring |

## Health Indicators

### Code Health

| Metric | Healthy | Warning | Action |
|--------|---------|---------|--------|
| `just build` | Compiles clean | Warnings present | File bead for warning cleanup |
| `just test` | All pass | Failures | File P1 bead for test fix |
| `just lint` | No warnings | Clippy warnings | File P3 bead for lint cleanup |
| Tech debt items | < 10 tracked | > 10 untracked | Update `docs/plans/bobbin-debt.md` |

### Bead Health

| Metric | Healthy | Warning | Action |
|--------|---------|---------|--------|
| Ready beads | < 20 | > 20 unworked | Triage with ian |
| In-progress beads | < 5 per polecat | > 5 | Check for stuck work |
| Stale beads | 0 (> 7d no activity) | Any stale | Nudge or close |
| Pitch beads pending | < 5 | > 5 unreviewed | Nudge ian for review |

### Planning Health

| Doc | Healthy | Warning |
|-----|---------|---------|
| `docs/plans/bobbin-roadmap.md` | Updated < 14d | Stale > 14d |
| `docs/plans/bobbin-debt.md` | Updated < 14d | Stale > 14d |
| Pitch beads | Filed regularly | No pitches in > 7d |
| Patrol reports | Sent every session | Missed sessions |

## Known State

_Snapshot as of 2026-02-17 — update during patrols._

- Bobbin is under active development with semantic indexing and MCP tool focus
- Recent work: MCP tools (dependencies, file_history, status, commit_search), CLI `log` command
- Eval suite expanding with Go and pandas task suites
- mdbook documentation scaffolded with Dracula theme
- This is strider's first deployment — initial planning docs not yet created

## Recovery Procedures

### Build fails
1. Check `just build verbose=true` for error details
2. If dependency issue: file P1 bead
3. If code issue: file appropriate priority bead for polecat

### Tests failing
1. Run `just test verbose=true` for failure details
2. File P1 bead with test name and error output
3. Note: polecats should NOT push code with failing tests

### Stale bead backlog
1. Run `bd list --status=open` to assess
2. Identify truly stale (no activity > 7d)
3. Comment on each: still relevant? blocked? needs re-spec?
4. Mail ian with summary and recommendations
