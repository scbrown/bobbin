# Bobbin Documentation Gaps — Beads to File

> Filed by stryder 2026-03-21. File as beads when Dolt is available.
> All are P2 except the primer update (P1). Label: `docs`.

## 1. [P1] Update bobbin primer (docs/primer.md)

Add config hierarchy summary, mention key advanced features (calibrate, reactions,
tags, RBAC, feedback, multi-repo calibration), and point to mdbook sections for
details. Currently a minimal overview that hides most features. The primer is what
agents see via `bobbin prime` — it's the front door to all documentation.

## 2. [P2] Document config override hierarchy

Global (`~/.config/bobbin/config.toml`) < per-repo (`.bobbin/config.toml`) <
`calibration.json` < CLI flags. Currently **zero mention** of global config or
cascade in any docs. The `Config::load_merged()` function implements deep TOML
merging (tables merge recursively, arrays replace). Agents didn't know global
config existed — they thought per-repo was the only level.

Update: `config/reference.md`, `guides/multi-repo.md`, primer.

## 3. [P2] Document calibrate command fully

Multi-repo support (`--repo`/`--source` flags), sweep modes (core/full/bridge),
`calibration.json` format and what it controls, auto-calibrate on index,
`CalibrationGuard` recalibration triggers, sweep cache and `--resume`. CLI
reference page exists but lacks operational guidance and the new multi-repo
features.

## 4. [P2] Document hooks advanced features

Reaction system (`reactions.toml`), noise filtering (`is_automated_message()`,
`skip_prefixes`), progressive reducing (session-level delta injection),
`feedback_prompt_interval`, `repo_affinity_boost`, `keyword_repos` scoping.
Hooks guide covers basics but misses advanced operational features.

## 5. [P2] Document tag effects and role-scoped overrides

How tag boosts/demotions work in scoring, scoped effects per role, exclude vs
demote semantics, the difference between `/search` (raw LanceDB scores, no tag
effects) and `/context` (with tag effects applied during assembly). Tags guide
exists but lacks the scoring mechanics that operators need.

## 6. [P2] Document role-based access control (RBAC)

`AccessConfig` roles, `allow`/`deny` repo glob patterns, `deny_paths`,
`default_allow`, interaction with groups and multi-repo filtering. No dedicated
RBAC documentation exists — only struct docstrings in config.rs.

## 7. [P2] Document feedback system

How ratings work, `feedback:hot`/`feedback:cold` tag assignment,
`feedback_prompt_interval` config, `FeedbackStore` schema, how feedback data
improves search quality over time. Currently undocumented anywhere.

## 8. [P2] Document archive integration

`ArchiveConfig` sources, YAML frontmatter schema matching, `name_field` for
chunk naming, webhook push notifications (`webhook_secret`), use cases (HLA
records, pensieve agent memories). Minimal inline docs only.
