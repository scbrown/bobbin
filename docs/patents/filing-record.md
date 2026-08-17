# Filing record — Provisional B 🎯 **the one being converted**

**Status:** ✅ **FILED 2026-08-17.** Of the four provisionals, **B is the only one
planned for conversion to a nonprovisional.** Everything below matters more here
than in the sibling repos.

> This repo is public. Personal fields (full legal name, residence, mailing
> address) are deliberately **not** recorded here.

## B — this repository

| Field | Value |
|---|---|
| **Application number** | **`64/135,383`** |
| Confirmation number | `2043` |
| Attorney docket | `SCB-002-PRV` |
| Title | Event-Driven Context Injection for Language-Model Coding Agents Using Version-Control Provenance, Self-Supervised Calibration, and Session-Scoped Delta Injection |
| Specification filed | [`provisional-B-retrieval.pdf`](provisional-B-retrieval.pdf), 37 pp · source [`provisional-retrieval-cluster.md`](provisional-retrieval-cluster.md) |
| Filed | 2026-08-17, 3:52:40 PM ET |
| Type | Provisional under 35 U.S.C. 111(b), Utility |
| Entity status | Small · Sole inventor · **Unassigned** |
| **Expires** | **2027-08-17. Not extendable.** |

A provisional is never examined and never publishes. It grants nothing; it fixes
a priority date of **2026-08-17** for whatever the specification supports.

## 🎯 The conversion plan

**Goal: a granted patent as a durable credential, not licensing revenue.** That
inverts the usual advice — **claim breadth is worthless and speed is everything.**
Breadth is what makes prosecution expensive, because breadth is what you fight the
examiner over for years. A narrow claim on one specific mechanism grants faster
and cheaper and reads identically on a résumé. **Aim narrow. Take the first
allowable subject matter offered.**

**Why B and not the siblings.** B's lead mechanism is the most concrete of the
four and therefore the hardest to dismiss as an abstract idea under §101 — the
failure mode that kills most software applications:

> classify a retrieved chunk as documentation → run a line-attribution query over
> **exactly that chunk's line span** → collect the commits that introduced those
> lines → expand each to its changed files → filter to source and test → inject at
> a fixed fraction of the seed score.

Specific operations, specific data structures, specific technical effect.

**Claim targets, in order:**

1. **Blame-provenance bridging** (above). Narrow independent claim.
2. **Self-supervised calibration from repository history** — commits as synthetic
   queries, changed-file sets as relevance ground truth, no human labelling.

🟢 **This repo's own ablation supports claim 1 empirically.**
[`../paper-context-injection.md`](../paper-context-injection.md) measures git
blame bridging at **−0.303 F1 when disabled**, second only to semantic search.
Not required for patentability, but useful.

**Execution:** a **registered patent agent** (not a law firm), flat fee,
**claims-only** — the 37-page specification already exists, so the scope is
claims, abstract and conforming edits rather than drafting from scratch. Budget
**Track One** prioritized examination (~$1,700 small entity); when the grant date
is the whole point, it is the highest-leverage spend. Expect at least one §101 or
prior-art rejection and amend toward the specific machinery.

⚠️ **A prior-art search must happen before the agent engagement.** The novelty
read on blame-bridging and commit-history-supervised calibration is an
impression, not a search. Start from `quipu/docs/patents/prior-art-search-notes.md`.
If it comes back badly, **A (`64/135,410`, quipu) is the designated backup.**

## ⚠️ Read `code-findings.md` before drafting claims

[`code-findings.md`](code-findings.md) records an adversarial doc-vs-code review
of this specification. Several mechanisms the provisional describes did **not**
behave as described at review time:

- The **intent doc-demotion factor had opposite semantics** on the local hook path
  versus the remote one — a 1.5 factor that strengthened demotion remotely
  *weakened* it locally.
- **Complementary-expansion dedup** missed non-adjacent duplicates, so one file
  could consume several of the five truncated slots.
- The **topic fingerprint** truncates its top-10 alphabetically rather than by
  score.

Both bugs are fixed on `main` (`7d99f03`, `cee3cdc`). **Claims should recite
mechanisms that work as the specification describes them.** Where they diverge,
the specification's "in the working embodiment" hedging is doing real work and
should be preserved rather than tightened.

## Disclosure and grace periods: satisfied

`quipu/docs/patents/disclosure-timeline.md` (rev 3, adversarially re-derived) gives
per-mechanism first-disclosure dates from this repo's public history:

| Mechanism | First disclosed | US deadline |
|---|---|---|
| Coupling expansion | **2026-01-02** (initial scaffolding) | 2027-01-02 |
| Blame-provenance bridging | 2026-02-11 | 2027-02-11 |
| Self-supervised calibration from repo history | 2026-02-25 | 2027-02-25 |

**Controlling date for the cluster: 2027-01-02**, because any claim reciting
coupling expansion as an element — including the combination claim — inherits the
January date.

✅ **All satisfied.** The 2026-08-17 filing lands inside every window, so nothing
in this repo's public history is prior art against this application. The
nonprovisional inherits the 2026-08-17 effective filing date for supported claims.

🔴 **Non-US rights are gone.** Absolute-novelty jurisdictions have no grace
period, and this repo has been public since 2026-01-02. Not recoverable, and not
part of the plan.

## The sibling filings

| | Repo | Application | Disposition |
|---|---|---|---|
| A | quipu | `64/135,410` | backup |
| B | **bobbin** (this one) | **`64/135,383`** | 🎯 **convert** |
| C | NeuralAmplifier | `64/135,421` | lapse |
| D | camayoc | `64/135,436` | lapse |

## For agents working in this repo

- Do **not** describe B as conferring protection. A provisional grants nothing.
- "Patent pending" is accurate only while an application is alive.
- Mechanisms added after 2026-08-17 that the filed specification does not support
  are **not** covered, and carry their own fresh 12-month disclosure clocks.
- If you change a mechanism named in the claim targets above, say so — it may
  affect what can be claimed.
