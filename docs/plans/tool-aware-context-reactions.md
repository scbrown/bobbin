# Tool-Aware Context Reactions

**Date**: 2026-03-02
**Author**: aegis/crew/stryder
**Status**: Draft v2 — incorporates Stiwi feedback
**Related**: aegis-fxnt3x (tags phase 4), aegis-nbltyp (feedback loop)

## Problem

Agents make changes through tools (MCP calls, bash commands, file edits) that
have downstream consequences they don't always know about. Examples:

- `batch_probe` modifies a container's config → but the change isn't reflected
  in goldblum IaC, so next Terraform apply reverts it
- `service_restart` on a container → but the service has a known issue tracked
  in a bead that the agent doesn't know about
- Agent edits a file in one repo → but a coupled file in another repo needs
  the same change (cross-repo coupling)
- Agent runs `apt install` on a container → but the container is managed by
  Ansible and the package should be declared in a playbook

The common thread: **the tool use itself is a signal that specific guidance and
context would be valuable right now**, not at some later prompt submission.

## Goal

Extend bobbin's PostToolUse hook to react to specific tool uses with targeted
guidance and contextual file injection **immediately after the tool call**.
When an agent does X, bobbin says "here's what you should also consider" and
provides the relevant files. The agent sees cause and effect in the same turn.

## Design Principles

1. **Immediate** — reactions fire in PostToolUse, not deferred to next prompt.
   Polecats may not have another UserPromptSubmit. Cause and effect must be
   adjacent in context for the agent to connect them.
2. **Non-dismissable** — agents cannot suppress reactions. Quality control
   happens through bobbin feedback (useful/noise/harmful ratings via ian's
   feedback loop). Bad reactions get tuned, not silenced.
3. **Index-validated** — if a reaction's search query returns no indexed results,
   the reaction still fires but emits a warning (to ops/metrics). Missing index
   coverage is a signal that something should be indexed, not that the reaction
   is wrong.
4. **Leverages existing bobbin features** — temporal coupling, provenance
   bridging, tag-scoped search, and the context assembly pipeline. No parallel
   infrastructure.

## Existing Bobbin Features Used

### Temporal Coupling (co-change data)

Bobbin already tracks which files change together via `GitAnalyzer::analyze_coupling()`.
The coupling data lives in SQLite and is queryable via `MetadataStore::get_coupling(file, limit)`.

**Reaction use**: When an agent edits a file, query coupling data to find files
that historically change with it. Surface them as "you may also need to update
these files." This replaces the static `match = { file_path = "*/api/*.go" }`
rules in v1 — coupling data is empirical, not hand-authored.

### Provenance Bridging (doc → commit → source)

Bobbin bridges from documentation chunks to source files via git blame/diff-tree.
`BridgeMode::BoostInject` discovers related source through commit provenance.

**Reaction use**: When an agent reads or edits a doc, bridging can surface the
source files that doc describes. This is already in the context assembly pipeline
but doesn't currently fire on PostToolUse.

### Tag-Scoped Search

Tags (phases 1-3, shipped) allow reactions to scope their context searches:
- `search_tags = ["auto:config"]` → only config files
- `search_tags = ["user:canonical"]` → prefer canonical docs
- Tag effects (boost/exclude) apply per role

### Context Assembly Pipeline

`ContextAssembler::assemble(query, repo)` handles the full pipeline: hybrid
search → RRF → recency boost → tag effects → coupling expansion → bridging →
budget assembly. Reactions use this directly — no custom search logic.

### PostToolUse Hook (existing)

`cli/hook.rs` already handles PostToolUse events. It receives JSON on stdin:
```json
{"tool_name": "Edit", "tool_input": {"file_path": "...", "old_string": "...", "new_string": "..."}, "cwd": "...", "session_id": "..."}
```
Currently dispatches by tool_name to extract intent (file paths, search queries).
Reactions extend this dispatch with rule evaluation.

## Reaction Rule Format

Rules live in `.bobbin/reactions.toml` (per-index) or a shared config:

```toml
# Rule: container modifications should be reflected in IaC
[[reactions]]
name = "iac-drift-check"
tool = "mcp__homelab__batch_probe"        # Tool name pattern (glob)
match = { command = "*" }                 # Parameter matching (optional)
guidance = """
You just made a change to a container via batch_probe.
Ensure this change is reflected in goldblum IaC so it persists
across reprovisioning. Check the relevant Terraform/Ansible files.
"""
search_query = "terraform container {args.container}"
search_group = "goldblum"                 # Which index group to search
search_tags = ["auto:config"]             # Prefer config-tagged files
max_context_lines = 50                    # Budget for injected context

[[reactions]]
name = "service-known-issues"
tool = "mcp__homelab__service_restart"
guidance = """
Service restarted. Check if there are known issues or recent changes
that could affect this service.
"""
search_query = "{args.service} {args.container} configuration"
search_tags = ["auto:config", "user:ops-docs"]

[[reactions]]
name = "package-iac-declaration"
tool = "Bash"
match = { command = "apt install *" }     # Regex on command parameter
guidance = """
You installed a package directly. If this container is managed by
Ansible/Terraform, add the package to the relevant IaC declaration
so it persists across reprovisioning.
"""
search_query = "ansible package {matched.package} container"
search_group = "goldblum"

[[reactions]]
name = "terraform-plan-check"
tool = "Edit"
match = { file_path = "*.tf" }
guidance = """
Terraform file modified. Run `terraform plan` to verify the change
before applying. Check for dependent resources.
"""
search_query = "terraform {file_stem} resource"

# Coupling-based reaction (uses temporal coupling data, not static patterns)
[[reactions]]
name = "coupled-files"
tool = "Edit"
use_coupling = true                       # Query temporal coupling for edited file
coupling_threshold = 0.3                  # Minimum coupling score
guidance = """
This file has historically changed alongside other files.
Review these coupled files for necessary updates.
"""
```

### Parameter Templating

Rules can reference tool call parameters using `{args.param_name}`:
- `{args.container}` — the container argument from batch_probe
- `{args.command}` — the command from Bash
- `{args.file_path}` — the file path from Edit/Write
- `{file_stem}` — filename without extension (derived)
- `{matched.*}` — regex capture groups from match patterns

### Rule Matching

```
For each PostToolUse event:
  1. Match tool name against rule.tool (glob pattern)
  2. If rule has match conditions, evaluate against tool args
  3. Collect all matching rules (multiple can fire)
  4. Dedup: if same rule already fired this turn, skip
  5. For each match:
     a. Render guidance text (template substitution)
     b. If use_coupling: query MetadataStore::get_coupling()
     c. Else: execute bobbin search via ContextAssembler
     d. If search returns no results: emit warning metric, still show guidance
     e. Assemble context: guidance header + search results / coupled files
  6. Inject immediately as PostToolUse hook output
```

### Dedup Strategy

Within a single agent turn (between UserPromptSubmit events):
- Track which `(rule.name, key_args)` combinations have fired
- Same rule + same args → skip (e.g., 5 batch_probe calls to same container)
- Same rule + different args → fire each (different containers)
- State stored in memory during hook invocation (no file needed — PostToolUse
  hook is invoked per-tool-call, so dedup state is passed via a session-scoped
  file at `.bobbin/session/<session_id>/reactions.jsonl`)

### Injection Format

```
=== Reaction: iac-drift-check ===

You just made a change to a container via batch_probe.
Ensure this change is reflected in goldblum IaC so it persists
across reprovisioning. Check the relevant Terraform/Ansible files.

--- Relevant files (3 results) ---

goldblum/terraform/containers/monitoring.tf:15-40 (score 0.92)
  resource "proxmox_lxc" "monitoring" {
    hostname    = "monitoring"
    ...

goldblum/ansible/roles/monitoring/tasks/main.yml:1-20 (score 0.85)
  - name: Install monitoring packages
    ...

=== End Reaction ===
```

### Index Validation

When a reaction's search returns zero results:

```
=== Reaction: iac-drift-check ===

You just made a change to a container via batch_probe.
Ensure this change is reflected in goldblum IaC so it persists
across reprovisioning. Check the relevant Terraform/Ansible files.

⚠ No indexed files found for this reaction.
  Query: "terraform container monitoring"
  Group: goldblum
  This may indicate missing index coverage.

=== End Reaction ===
```

Also push a warning metric:
```
bobbin_reaction_no_results_total{rule="iac-drift-check"} 1
```

### Feedback Integration

Reactions are a form of injection. They participate in the feedback loop:
1. Each reaction injection gets an `injection_id` (same as regular injections)
2. Agents can submit feedback via `bobbin_feedback_submit` referencing the ID
3. Feedback accumulates per-rule: `bobbin_reaction_feedback{rule="...",rating="..."}`
4. Rules with consistently "noise" feedback get flagged for human review
5. Agents **cannot dismiss** reactions — feedback is the quality control mechanism

## Implementation Plan

### Phase 1: Rule Engine + Coupling Integration
- Define `reactions.toml` schema and parser (ReactionRule struct, TOML serde)
- Implement rule matching: tool name glob + parameter regex conditions
- Template rendering for guidance text and search queries
- Coupling-based reactions: query `MetadataStore::get_coupling()` for Edit tools
- Unit tests for matching, rendering, coupling queries

### Phase 2: PostToolUse Hook Integration
- Extend `cli/hook.rs` PostToolUse handler to evaluate reaction rules
- Execute bobbin searches via existing `ContextAssembler` for matching rules
- Format and inject reaction context immediately (PostToolUse output)
- Session-scoped dedup via `.bobbin/session/<id>/reactions.jsonl`
- Index validation: emit warning when search returns no results
- Budget management: per-reaction `max_context_lines`, global cap

### Phase 3: Built-in Rules + Metrics
- Ship default rules for common patterns:
  - Container modification → IaC check (goldblum group)
  - Service restart → known issues
  - Package install → IaC declaration
  - Terraform edit → plan check
  - Edit any file → temporal coupling check
- Rules are overridable/disableable in `reactions.toml`
- Pushgateway metrics:
  - `bobbin_reaction_fired_total{rule="..."}`
  - `bobbin_reaction_no_results_total{rule="..."}`
  - `bobbin_reaction_latency_seconds{rule="..."}`

### Phase 4: Feedback Loop + Auto-Tuning
- Wire reactions into ian's feedback loop (injection_id per reaction)
- Track per-rule feedback ratings
- Dashboard: which rules are useful vs noise
- Auto-flag rules with >50% noise rating for human review
- Metric: `bobbin_reaction_feedback{rule="...",rating="useful|noise|harmful"}`

## Resolved Questions

1. **When do reactions fire?** → Immediately in PostToolUse. Not deferred.
   Polecats may not get another UserPromptSubmit. Cause and effect must be
   adjacent.

2. **Can agents dismiss reactions?** → No. Feedback is the quality control
   mechanism. Agents submit feedback:noise via bobbin's feedback loop.

3. **What if the search returns nothing?** → Guidance still shows. A warning
   is emitted to metrics. Missing index coverage should be investigated, not
   silently swallowed.

4. **How does coupling integrate?** → Phase 1, not a late addition. The
   `use_coupling = true` rule type queries existing `MetadataStore::get_coupling()`
   to find files that historically change together. This is bobbin's strongest
   empirical signal for "you should also look at this."

## Open Questions

- Should there be a global reaction budget (max total lines per turn)?
- Can reactions trigger other tools? (e.g., auto-run `terraform plan`)
  Probably not in v1 — keep reactions advisory only.
- Should coupling-based reactions use a minimum co-change count threshold
  in addition to score threshold?
- How to handle reactions for cross-index searches when the target group
  isn't available on the current bobbin instance?
