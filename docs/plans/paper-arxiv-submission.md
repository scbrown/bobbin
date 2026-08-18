# arXiv submission plan — bobbin-58

> **Status (2026-08-18):** planned, not executed. **Blocked on content, not on
> mechanics** — see §0. Steps 2 and 3 of §8 are now done; the remaining
> blocker is the keeper decision in step 1, then citations and LaTeX. The mechanics below are transcribed from the quipu
> submission (arXiv:submit/7961151, filed 2026-08-17) which is the proven
> process; the blocking items are specific to this paper.

## 0. Do not submit yet

The submission machinery is understood and cheap. The paper is not ready, and
the reason is in `docs/plans/paper-measurement-validity.md` and
`docs/plans/paper-statistics.md`:

| Blocker | State | What clears it |
|---|---|---|
| No ablation arm survives multiple-comparison correction | Written into the draft honestly (§5.3) | Nothing — either accept the reframe or run §5.3's 14/22/32 |
| Tool-availability confound accounts for ~92% of the headline effect | Written into the draft (§5.2), unresolved | The fourth arm: installed + advertised, hook disabled |
| Flask tasks withdrawn; calibration sweep withdrawn | Done (§4.2, §5.7) | — |
| Run counts inconsistent (85 / 96 / 108) | **Cleared 2026-08-18** — recounted, all three wrong; census in Appendix A | — |
| Aggregate reported without SDs, cannot be tested | **Cleared 2026-08-18** — SDs recomputed, aggregate now tested (p = 0.395, §5.4) | — |
| Table 6 aggregate contaminated with withdrawn Flask runs | **Found and fixed 2026-08-18** (§5.4.1); found *by* the recount | — |
| **Ablation baseline is not model-matched** — arms are pure Sonnet 4.5, baseline mixes in Opus 4.6 | **Found 2026-08-18**, written up as §5.1.1 + Table 2. Three of six effects flip sign; the one nominally significant arm drops to p = 0.191 | Nothing available from the existing runs. A re-run that records the serving model per run and holds it fixed across arms |
| Related work has no citation keys or BibTeX | §7 drafted in prose | LaTeX conversion, below |

**The model-matching defect raises the stakes on the keeper decision below.**
It is not fixable from the artifacts — the runs that lack a model record cannot
be attributed retrospectively — so it lands in the paper as a stated limitation
whichever way step 1 goes. It also argues *for* the methodological framing: the
paper now carries three independently discovered ways an apparently reasonable
agent evaluation misleads (broken fixtures, a confounded control arm, an
unmatched baseline), which is a more useful contribution than a +0.072
aggregate. If the fourth arm is run, it should be run under a pinned model with
per-run attribution, and the existing arms re-run alongside it — otherwise it
inherits the same defect.

**Recommendation: run the fourth arm before submitting.** It is the single
cheapest thing that changes what the paper can claim — roughly 30 runs, ~$45,
half a day — and without it the paper's central mechanism is confounded with
tool availability by its own data. A reviewer who reads §5.2 will ask for it,
and the honest answer today is that we have not run it.

The alternative — submit as a negative/methodological result — is defensible
given how §5.2 and §5.3 are now written, but it is a materially different
paper. That is a keeper decision (ian), not a ranger one.

## 1. Categories

Primary **cs.SE**, cross-list **cs.IR** and **cs.AI**.

This is the reverse of quipu (primary cs.AI, cross-list cs.SE), which is right:
quipu is a knowledge-representation argument, this is a software-engineering
tooling-and-measurement paper. cs.IR is added because the decomposition is
fundamentally a retrieval-methods ablation and the RRF/hybrid-retrieval
audience is the one most likely to engage with §5.1.

**Endorsement**: already held for cs.AI from the quipu submission. cs.SE as
primary may require separate endorsement — check before filing rather than
discovering it at submit time. If cs.SE endorsement is not held and cannot be
obtained quickly, file cs.IR primary with cs.SE cross-list; the content
supports either.

## 2. Licence

Match quipu: **CC BY 4.0**. Do not select the arXiv default non-exclusive
licence — it blocks downstream reuse and is not what the rest of this work
uses.

## 3. LaTeX conversion

The draft is markdown (`docs/paper-context-injection.md`, ~530 lines). arXiv
strongly prefers LaTeX source.

Use `quipu/docs/paper/` as the template — it is a working, submitted arXiv
build:

```text
docs/paper/
  main.tex           # documentclass, packages, title block, \input list
  references.bib
  sections/
    00-abstract.tex
    01-introduction.tex
    ...
```

Conversion notes specific to this paper:

- **Section mapping** is 1:1 with the current markdown headings, which were
  renumbered for exactly this purpose: `00-abstract`, `01-introduction`,
  `02-architecture`, `03-methods`, `04-setup`, `05-results`, `06-discussion`,
  `07-related`, `08-future`, `09-conclusion`, plus `a-data` and `b-repro`.
- **Nine tables**, all currently markdown pipe tables. Convert to `booktabs`
  (`\toprule`/`\midrule`/`\bottomrule`), which `main.tex` already loads. Tables
  1-5 carry the argument and should be `\begin{table}[t]`; Tables 6-9 are
  reference data and can float freely.
- **Confidence intervals** contain en-dashes and ± signs. Use `$[-0.721, -0.047]$`
  in math mode rather than literal Unicode, and `\pm`. The current markdown
  uses `±` and `−` (U+2212) freely; a naive copy-paste will fail or render
  badly under pdflatex.
- **No figures currently exist.** Consider one: a forest plot of the six
  effects with their confidence intervals would communicate "five of six cross
  zero" instantly, and is the single highest-value addition to the paper's
  presentation. `main.tex` already loads TikZ. Plot **Tables 1 and 2 on the
  same axis** — published effect and model-matched effect side by side per arm
  — which shows the sign flips and the interval overlap in one image and is a
  stronger figure than either table alone.
- **Author block**: reuse quipu's, including the ORCID footnote and the
  assistance acknowledgement. Update the repository URL to
  `github.com/scbrown/bobbin`.
- **Build recipe**: copy quipu's `just paper` (tectonic → latexmk → pdflatex
  fallback chain) into this repo's justfile. Do not invent a new one.

## 4. Bibliography

§7 is drafted in prose with author-year references but **no citation keys and
no BibTeX entries**. Both must exist before conversion.

Reuse the quipu bibliography where it overlaps — `quipu/docs/paper/references.bib`
already carries RAG, agent-evaluation and knowledge-representation entries.
Expected overlap: Lewis et al. 2020, and the agent-evaluation entries.

Expected *new* entries, none of which quipu needs:

- Rank fusion: Cormack et al. 2009 (RRF)
- Dense/sparse retrieval: Karpukhin et al. 2020 (DPR), Formal et al. 2021 (SPLADE)
- Code search: Husain et al. 2019 (CodeSearchNet), Feng et al. 2020 (CodeBERT), Guo et al. 2021 (GraphCodeBERT)
- Context management: Packer et al. 2023 (MemGPT), Xu et al. 2024
- History mining: Zimmermann et al. 2005 (co-change)
- Monitor-Guided Decoding: Agrawal et al. 2023
- Agent benchmarks: Jimenez et al. 2024 (SWE-bench)

**Verify every entry against the real record before filing.** Author lists,
years and venues in §7 were written from working knowledge and have not been
checked against the published versions. Fabricated or misattributed citations
are the most damaging possible error in a submission and the easiest to avoid.

## 5. Build verification against the local artifact

Quipu's process, which caught real problems and should be repeated:

1. `just paper` builds clean — **zero** unresolved references, zero overfull
   boxes that reach the margin.
2. Every number in the PDF traces to an artifact. For this paper that means
   `scripts/paper_stats.py` output for §5.1-5.3, `scripts/paper_census.py` for
   §5.4-5.5 and Appendix A, and `eval/results/runs/` for anything about what a
   given arm actually injected. Re-run both scripts and diff against the
   tables; do not eyeball them. The Table 5 contamination found on 2026-08-18
   is what this step is for, and it survived two drafts of eyeballing.
3. The PDF compiles from a **clean checkout** of the submitted tarball, not
   from the working directory. Missing `\input` files and stale `.aux` are the
   usual failures.
4. Check the arXiv-generated PDF after upload, not just the local one. Their
   TeX Live version differs.

## 6. The External DOI trap

**The one that cost time on the quipu submission.** arXiv's submission form has
a "DOI" field. It is for the DOI of a *published version of this same paper*
(journal or conference). It is **not** for a Zenodo artifact DOI.

Putting the artifact DOI there makes arXiv display the paper as already
published elsewhere, which is wrong and awkward to correct after announcement.

The artifact DOI belongs in the **author footnote** (as quipu does) and
optionally in the comments field. Leave the DOI field **empty** unless there is
a genuine journal/conference DOI.

## 7. Artifact archival

Quipu archived to Zenodo and cited the version DOI in the author footnote, with
the concept DOI resolving to newest. Do the same, and note what "artifact"
means here — it is larger than quipu's:

- Source at the tagged release
- `eval/tasks/` **including `_quarantined/`** — the quarantine README is
  load-bearing evidence for §4.2 and must not be dropped from the archive
- `eval/results/runs/` per-run artifacts, which are what §5.2 rests on
- `scripts/paper_stats.py` and `scripts/paper_census.py`
- `docs/plans/paper-measurement-validity.md` and `paper-statistics.md`

The last item matters: §5.2's claim that one arm injected nothing is checkable
only against the `*_metrics.jsonl` files. Archive them or the claim is
unverifiable.

## 8. Order of operations

1. Keeper (ian) decides: run the fourth arm, or submit as a methodological
   result. **Blocking.**
2. ~~Recount runs from artifacts; fix the 85/96/108 discrepancy.~~ **Done
   2026-08-18.** `scripts/paper_census.py` recounts from
   `eval/results/runs/`; Appendix A carries the census. None of the three
   numbers was right — 85 is the whole study, 96 was never executed, 108 is
   unattributable, and the ablation is 19 runs.
3. ~~Recompute aggregate dispersion; either report SDs or drop Table 5's
   numbers.~~ **Done 2026-08-18.** Table 5 is rebuilt with SDs over the 41
   reported runs and is now testable: Δ = +0.072, p = 0.395. The recount also
   caught that the old 66-run aggregate included 25 withdrawn Flask runs and
   that its test-pass-rate finding was entirely that contamination (§5.4.1).
4. Verify every §7 citation against the published record; write `references.bib`.
5. Convert to LaTeX against the quipu template; add the forest-plot figure.
6. Build verification (§5).
7. Zenodo archival; capture version and concept DOIs.
8. File: cs.SE primary, cs.IR + cs.AI cross-list, CC BY 4.0, **DOI field empty**.
9. Check the arXiv-rendered PDF before announcement.
