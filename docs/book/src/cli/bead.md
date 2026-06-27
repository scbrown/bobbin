---
title: bead
description: Record and mine bead-to-commit workflow telemetry
tags: [cli, bead, telemetry]
status: draft
category: cli-reference
related: [cli/hook.md]
commands: [bead]
feature: telemetry
source_files: [src/cli/bead.rs]
---

# bead

Record and mine bead-to-commit workflow telemetry (GH#9).

## Synopsis

```bash
bobbin bead <SUBCOMMAND> [OPTIONS]
```

## Description

The `bead` command captures the relationship between beads (units of work) and
the commits that resolve them. Each link is stored in the `bead_lineage` table
with the changeset it touched — files, line counts, and named symbols — so later
layers can mine which code matters for which kinds of work and reconstruct which
change introduced a bug.

Lineage is normally populated automatically by the [git post-commit
hook](./hook.md) (`bobbin bead auto-link`); the subcommands below are also
available directly.

## Subcommands

### link

Manually link a bead to a commit and its changeset.

```bash
bobbin bead link bo-abc123 <COMMIT> [--files a.rs,b.rs] [--type bug] \
    [--bundles slug1,slug2] [--action linked]
```

When `<COMMIT>` is given and `--files` is omitted, the changeset (files and line
counts) is read from git automatically. `--files` overrides git detection.

### auto-link

Link a commit to its bead, inferring the bead id from the commit. Invoked by the
post-commit hook; rarely run by hand.

```bash
bobbin bead auto-link --commit HEAD
```

The bead id is resolved by precedence: (a) a `Bead: <id>` trailer in the commit
message, (b) a parenthesized `(bo-xxxx)` token in the subject, (c) the first
bead-id token in the subject, (d) the branch name. If none match, no row is
recorded and the command exits 0. The link is idempotent: re-running on a commit
that already has a `commit` lineage row (e.g. after an amend or rebase) does not
create a duplicate.

### reconstruct-causality

Reconstruct bug causality and populate the `bug_causality` table — the
supervised signal for "risky change".

```bash
bobbin bead reconstruct-causality [--bug bo-abc123] [--limit 200]
```

For each bug bead, the command collects the files its fix touched, finds the
most-recent prior commit that touched each such file (the candidate culprit),
and scores confidence by how much of the fix's changeset that commit overlaps.
One row per `(bug, culprit_sha, file)` is upserted, so the job is idempotent and
safe to run periodically. With `--bug` it processes a single bug; otherwise it
scans bug beads found in lineage (capped by `--limit`).

> The current heuristic ranks culprits by file overlap and recency over
> `bead_lineage`. Git-blame sharpening of the exact introducing commit is a
> planned refinement.

### history

Show recorded lineage for a bead, or recent lineage across all beads.

```bash
bobbin bead history [BEAD_ID] [--commit <SHA>] [-n 20]
```

## See Also

- [hook](./hook.md) — installs the post-commit hook that auto-links commits.
