# Escalation Protocol

How and when to escalate as a ranger.

## Escalation Chain

```
strider (you)
  → ian (keeper, Search & Context) — first escalation point
    → goldblum (orchestrator) — if ian can't resolve
      → Stiwi (human) — via ntfy/Matrix
```

**Default**: Escalate to ian. Only bypass to goldblum for emergencies or if ian
is unresponsive for > 4 hours.

## Priority Matrix

| Level | Meaning | Channel | Response | Example |
|-------|---------|---------|----------|---------|
| **P0** | Emergency | Mail ian + goldblum | Act immediately | Build completely broken, data loss risk |
| **P1** | Needs decision | Mail ian | Wait 4h | Architecture question, conflicting priorities |
| **P2** | Proposal | Mail ian | Async | Pitch review request, roadmap update |
| **P3** | FYI | Mail ian (batch) | No wait | Patrol findings, status updates |

## When to Escalate

**Always escalate (P1+):**
- Any action outside your guardrails
- Priority conflict between beads
- Uncertainty about Bobbin's strategic direction
- Cross-rig dependency blocking Bobbin work
- 3+ failed attempts at the same planning problem

**Escalate as P0:**
- Build system completely broken (polecats can't work)
- Test suite catastrophically failing
- Critical MCP tool down
- Data integrity concern in LanceDB

## When NOT to Escalate

- Routine patrol completed → just send report
- Planning doc updated → commit and push
- Pitch bead filed → it goes through normal flow
- New tech debt identified → file bead and track it
- Minor issue found → file bead at appropriate priority

## Message Format

```
[P{0-3}] {one-line summary}

Context: {what happened, what state the system is in}
Tried: {what was attempted, if anything}
Proposed: {what you want to do next}
Need: {what's needed from ian — direction, approval, review}
```

## Channels

| Recipient | Command | Use For |
|-----------|---------|---------|
| ian (keeper) | `gt mail send aegis/crew/ian -s "Subject" -m "..."` | Primary escalation, patrol reports, pitches |
| goldblum | `gt mail send aegis/crew/goldblum -s "Subject" -m "..."` | Emergency bypass, cross-rig coordination |
| witness | `gt mail send bobbin/witness -s "Subject" -m "..."` | Polecat health concerns in bobbin rig |
| maldoon (warden) | `gt mail send aegis/crew/maldoon -s "Subject" -m "..."` | QA questions, rubric input requests |

## Example Escalations

### P1 — Architecture decision needed
```bash
gt mail send aegis/crew/ian -s "[P1] Bobbin embedding model choice" -m "
Context: Current model produces low-quality results for Rust code.
Tried: Reviewed alternatives in docs/plans/bobbin-roadmap.md.
Proposed: Switch to code-specialized model (see pitch bead bo-xxx).
Need: Direction on model selection — affects indexing pipeline significantly."
```

### P2 — Pitch review request
```bash
gt mail send aegis/crew/ian -s "[P2] New pitches for review" -m "
Context: Filed 3 new pitch beads this session.
Beads: bo-abc, bo-def, bo-ghi
Proposed: Review at your convenience.
Need: Approve/iterate/reject so I can plan next patrol."
```

### P3 — Routine report
```bash
gt mail send aegis/crew/ian -s "strider patrol report" -m "
Bobbin status: healthy, 2 polecats active
Active work: bo-xyz (MCP tool improvement), bo-abc (test coverage)
Pitches filed: 1 new (bo-def — PDF indexing support)
Concerns: none"
```
