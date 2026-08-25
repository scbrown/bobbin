---
title: Governed Path Boundaries
description: Surfacing Quipu's tripwire policies in injected context so an agent sees a boundary before it crosses one
tags: [quipu, governance, hooks, context-injection, guide]
status: draft
category: guide
related: [guides/quipu-integration.md, guides/hooks.md, guides/context-assembly.md]
---

# Governed Path Boundaries

A **tripwire** is a governance-plane policy that binds a *path boundary* to an
*effect*. It is an `aegis:Policy` at `aegis:boundary "action"` that carries
`aegis:appliesTo` path globs and **no** selector or predicate — touching the
path *is* the crossing, so no further evidence is needed. Quipu is the
canonical store for these; Yupana's pre-edit guard enforces the ones it can.

Bobbin does neither. Bobbin **tells the agent the boundary is there**, in the
same injected bundle as the code that boundary spans, so "I did not know that
path was governed" stops being available as either an excuse or a surprise.

## What you see

When a wire spans a file Bobbin is about to inject, a short section is
prepended to the injected context:

```text
Governed path boundaries (tripwires) spanning files in this context:

- tripwire-auth-boundary — effect: deny
    claim: no agent edit targets a path matching src/auth/**
    boundary: src/auth/**
    in context: aegis:src/auth/login.rs
    placement: hard, judged at the pre-action gate (before the edit lands)

Bobbin does not enforce these; it reports what the governance graph declares.
Enforcement, where it exists, is the governing host's (yupana's pre-edit guard
for the wires it can enforce).
source: quipu http://quipu.example, read live for this turn
```

Every line is a *declaration*, never a promise about what Bobbin will do.
Bobbin has no pre-edit hook and cannot block a write; a section that read like
a gate would be exactly the armed-but-inert control the tripwire concept exists
to prevent, inverted.

## Enabling it

This feature has no configuration of its own. The transport is the existing
`quipu_endpoint` key — the same one search spotlight annotations and the MCP
ontology tools use:

```toml
# .bobbin/config.toml
quipu_endpoint = "http://quipu.example"
```

`BOBBIN_QUIPU_REMOTE` overrides it for testing.

**With no endpoint configured there is no governance plane**: the section is
absent and no HTTP call is made, so an ungoverned deployment pays nothing on
the hook's hot path.

## Both injection paths, by construction

Bobbin injects context two ways — locally (`bobbin inject-context`) and as a
thin client (`bobbin inject-context --server <url>`, which asks a Bobbin server
for `/context`). **Both read the governance graph directly**, rather than the
thin client receiving wires from the server it queried.

Two reasons:

- **The paths must not disagree.** Routing governance through the server would
  make "which boundaries am I told about" depend on which Bobbin deployment
  answered. Reading from the same place in both paths makes them identical by
  construction rather than by review.
- **A search server should not be able to suppress a governance boundary.** The
  Quipu endpoint is an organisation fact, not a Bobbin-deployment detail.

The cost is that a thin client needs to reach Quipu itself. It already loads
local Bobbin config for every other hook setting, so this adds a reachability
requirement, not a configuration one.

## Freshness, and saying so

`bobbin inject-context` runs on every user prompt, so the projection is cached
at `.bobbin/tripwire-cache.json` with a five-minute TTL rather than fetched per
keystroke. Staleness is never hidden — the last line of the section always
states where the facts came from and how old they are:

| Situation | What the section says |
|---|---|
| Fetched this turn | `read live for this turn` |
| Cache inside the TTL | `cached projection 2m old (within refresh interval)` |
| Refresh failed, cache used | `cached projection 40m old — REFRESH FAILED (…). These boundaries may have changed since; treat them as last-known, not current.` |

When a refresh fails and **nothing** matched, the section still appears, saying
only that. "I could not look" and "I looked and there was nothing" are
different facts, and an agent that cannot tell them apart walks into a boundary
believing it checked.

## Wires Bobbin does not understand

Yupana refuses a projection carrying an effect it cannot enforce, and it is
right to: a dropped wire there is a boundary that reads as guarded and is not.
Here the failure runs the other way — a dropped wire is a boundary the agent is
never told about. Bobbin therefore surfaces everything:

- An effect outside `warn` / `deny` / `throttle` (the vocabulary also includes
  `allow`, `require-approval`, `escalate`, `record`) is **named verbatim**.
  Bobbin only has to name an effect, not execute it.
- A policy declaring no effect at all, or a `throttle` with no
  `aegis:backoffFormula` (which Quipu's placement gate refuses), is rendered
  with a `⚠ MALFORMED` line naming the defect.
- Rows for one policy that disagree on a single-valued field mark that wire as
  conflicted rather than resolving it by row order — and the *other* wires in
  the batch still render. Its accumulated boundary globs are still shown,
  because those were never in conflict: the agent still learns the boundary
  exists and learns not to trust its effect.

Only a *malformed glob* silently matches nothing, because a pattern that will
not compile has no boundary to report.

## A known ambiguity: repos

`aegis:appliesTo` globs are repo-relative and carry no repo scoping. Yupana is
per-tenant so this never arises there; Bobbin indexes many repos at once, so a
wire declared `src/auth/**` matches that relative path in **every** indexed
repo.

Bobbin does not guess. It names the repo alongside each matched path
(`aegis:src/auth/login.rs`) and, when the matches span more than one repo, adds
a note saying the globs carry no repo scoping so you can judge which matches
were meant. Fixing this properly needs a repo/module scoping term in the
upstream shape.

## See Also

- [Quipu Integration](quipu-integration.md)
- [Hooks](hooks.md)
- [Context Assembly](context-assembly.md)
