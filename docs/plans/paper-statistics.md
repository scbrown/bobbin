# Significance and power for the ablation arms — bobbin-55

> **Status (2026-08-18):** computed, and the ablation figures are now
> **verified against the per-run artifacts** — all ten rows of Table 7
> reproduce exactly (N, mean, SD). Every figure in §1-3 and §5 below is
> derived from the means, standard deviations and run counts reported in
> `docs/paper-context-injection.md` Table 7; regenerate with
> `scripts/paper_stats.py`. §4's aggregate is derived from the raw runs;
> regenerate with `scripts/paper_census.py`.

`bobbin-55` asked for confidence intervals or appropriate tests on every
reported ablation effect, and for an explicit statement of which conditions are
underpowered. Here it is, and the answer is worse than "several conditions are
thin".

## 1. Method

Each ablation arm is compared against the `with-bobbin` baseline on ruff-001
(N=7, F1 0.636 ± 0.347) with **Welch's t-test** — unequal variances, which is
mandatory here since the arm SDs range from 0.000 to 0.385. Two-sided,
α = 0.05. Confidence intervals are on the difference in means.

The reported SDs are treated as sample standard deviations. If they are
population SDs the intervals are slightly narrower, which does not change any
conclusion below.

`blame_bridging=false` has SD exactly 0.000 across its 3 runs. Welch handles
this (the baseline still contributes variance), but a zero-variance arm at N=3
is itself weak evidence, not strong evidence.

## 2. Results

| Arm | N | Δ F1 | t | df | p | 95% CI of Δ |
|-----|:-:|-----:|----:|-----:|------:|-------------|
| `semantic_weight=0.0` | 4 | −0.384 | −2.61 | 8.40 | **0.030** | [−0.721, −0.047] |
| `blame_bridging=false` | 3 | −0.303 | −2.31 | 6.00 | 0.060 | [−0.624, **+0.018**] |
| `coupling_depth=0` | 3 | −0.247 | −1.73 | 7.61 | 0.123 | [−0.578, **+0.084**] |
| `doc_demotion=0.0` | 3 | −0.080 | −0.31 | 3.49 | 0.774 | [−0.839, **+0.679**] |
| `recency_weight=0.0` | 3 | −0.025 | −0.10 | 3.85 | 0.922 | [−0.700, **+0.650**] |
| `gate_threshold=1.0` | 3 | −0.025 | −0.10 | 3.85 | 0.922 | [−0.700, **+0.650**] |

**Five of six confidence intervals cross zero.** For those five the data are
consistent with the method *helping*, doing nothing, or *hurting*.

The three bottom rows are not weak results, they are absent results. A 95% CI
of [−0.70, +0.65] on a metric bounded in [0, 1] spans essentially the entire
achievable range: the study contains no information about recency boosting,
quality gating or doc demotion at all.

## 3. Multiple comparisons

Six arms are tested against one baseline, so the per-arm α must be corrected.
Holm–Bonferroni:

| Arm | p | Holm threshold | Outcome |
|-----|---:|---:|---|
| `semantic_weight=0.0` | 0.030 | 0.0083 | retain null |
| `blame_bridging=false` | 0.060 | 0.0100 | retain null |
| `coupling_depth=0` | 0.123 | 0.0125 | retain null |
| `doc_demotion=0.0` | 0.774 | 0.0167 | retain null |
| `recency_weight=0.0` | 0.922 | 0.0250 | retain null |
| `gate_threshold=1.0` | 0.922 | 0.0500 | retain null |

**After correction, no arm is significant.** Not one. The single nominally
significant result (`semantic_weight=0.0`, p = 0.030) fails its corrected
threshold of 0.0083 by nearly four-fold.

Correction is not optional here. The paper's framing — "which of six methods
carries the weight" — *is* a six-way comparison, and reporting the winner of six
uncorrected tests as a finding is the exact error the correction exists to stop.

## 4. The headline baseline result is also not significant

For completeness, ruff-001 `with-bobbin` (0.636 ± 0.347, N=7) vs `no-bobbin`
(0.324 ± 0.021, N=5):

```text
Δ = +0.312, t = 2.37, df = 6.06, p = 0.055, 95% CI [−0.009, +0.633]
```

p = 0.055, interval touching zero. This is the *strongest* task-level injection
effect in the study.

**Resolved 2026-08-18.** The 66-run aggregate (F1 0.695 → 0.722) was reported
without standard deviations or per-run values, and so could not be tested. It
can now: `scripts/paper_census.py` recomputes it from the per-run artifacts,
which carry everything the test needs. Two things came out of that recount.

The aggregate was **contaminated with the withdrawn Flask tasks** — 25 of the
66 — despite §4.2 claiming Flask was excluded everywhere. On the 41 reported
runs:

```text
with-bobbin 0.743 ± 0.270 (N=21) vs no-bobbin 0.671 ± 0.263 (N=20)
Δ = +0.072, t = 0.86, df = 39.0, p = 0.395, 95% CI [−0.097, +0.240]
```

The effect roughly doubles once the broken fixtures come out, and is still
comfortably inside the noise. The conclusion this section reached — the
aggregate is not defensible — survives; only the reason changes, from "cannot
be tested" to "tested, p = 0.395". Note also that pooling across tasks leaves
task difficulty as a between-arm confound, since the arms are not balanced per
task.

## 4b. The baseline is not model-matched — found 2026-08-18

Every figure in §2-4 above compares an ablation arm against the 7-run
`with-bobbin` baseline. **That baseline is model-mixed and the arms are not.**
From `model_usage` in the per-run artifacts (`scripts/paper_census.py` §6):

- All 19 ablation runs: `claude-sonnet-4-5-20250929`, confirmed.
- The 7-run `with-bobbin` baseline: 4 confirmed Sonnet 4.5, **1 confirmed
  `claude-opus-4-6`** (which scored F1 = 1.000, tied highest in the arm), 1
  declared Sonnet with no usage record, 1 with no model record at all.

Restricting the baseline to its 4 model-matched runs gives 0.530 ± 0.327
rather than 0.636 ± 0.347, and every arm moves:

| Arm | Δ published | Δ model-matched | p published | p matched |
|-----|---:|---:|---:|---:|
| `semantic_weight=0.0` | −0.384 | **−0.278** | 0.030 | **0.191** |
| `blame_bridging=false` | −0.303 | **−0.196** | 0.060 | 0.315 |
| `coupling_depth=0` | −0.247 | **−0.141** | 0.123 | 0.464 |
| `doc_demotion=0.0` | −0.080 | **+0.026** | 0.774 | 0.930 |
| `recency_weight=0.0` | −0.025 | **+0.081** | 0.922 | 0.768 |
| `gate_threshold=1.0` | −0.025 | **+0.081** | 0.922 | 0.768 |

**Three of six flip sign, and §2's one nominally significant arm loses its
nominal significance** — `semantic_weight=0.0` goes from p = 0.030 to 0.191,
failing at α = 0.05 *before* any correction. §3's conclusion (no arm survives
correction) is unchanged and now over-determined. The ruff-001 headline in §4
also halves: +0.312 (p = 0.055) becomes +0.212 (p = 0.285) model-matched.

**What survives is §6's recommended framing, strengthened.** The
retrieval/filtering grouping separates *by sign* on the matched baseline —
retrieval −0.141 to −0.278, filtering +0.026 to +0.081 — where on the
published baseline the groups shared a sign and differed only in magnitude.
The grouping is the claim to lead with; this is the sharpest test it has had.

**Neither set of magnitudes is established.** The matched baseline is N=4 and
is more underpowered than the thing it corrects. The difference between the
two columns is a lower bound on how much the model mixture moves the results,
not a corrected measurement. Any re-run must record the serving model per run
and refuse cross-model comparisons; 45 of the study's 85 runs cannot be
attributed to a model from their own artifact.

## 5. Power — what the study would have needed

Runs per arm required to detect the *observed* effect at 80% power, α = 0.05,
two-sample, using the baseline SD of 0.347:

| Arm | Observed \|Δ\| | Cohen's *d* | N needed | N had |
|-----|---:|---:|---:|---:|
| `semantic_weight=0.0` | 0.384 | 1.11 | **14** | 4 |
| `blame_bridging=false` | 0.303 | 0.87 | **22** | 3 |
| `coupling_depth=0` | 0.247 | 0.71 | **32** | 3 |
| `doc_demotion=0.0` | 0.080 | 0.23 | **297** | 3 |
| `recency_weight=0.0` | 0.025 | 0.07 | **3026** | 3 |
| `gate_threshold=1.0` | 0.025 | 0.07 | **3026** | 3 |

Two readings, both worth stating:

- The three retrieval-expansion arms are **within reach**. 14/22/32 runs per arm
  is roughly 70 runs total at ~$1.50 and ~5 minutes each — about $105 and a day
  of wall-clock. That is a fundable experiment, not a wish.
- The three filtering arms are **not worth powering**. Detecting a 0.025 F1
  effect needs ~3,000 runs per arm (~$9,000). The correct move is not to run
  them but to stop claiming them.

Note the circularity, and state it in the paper: powering a study on the effect
size observed in an underpowered pilot is optimistic, because pilots that reach
significance overestimate effect size (the winner's curse). Treat 14/22/32 as
lower bounds.

## 6. What this means for the paper's claims

The current draft says:

> All six methods contribute positively -- disabling any one hurts performance.

This is not supportable. Three of the six have confidence intervals spanning
most of the metric's range, and after correction none of the six is
distinguishable from zero. Six point estimates happening to fall on the same
side of zero is what one expects from six noisy estimates roughly half the
time, and is not evidence that all six contribute.

**Recommended rewrite of the claim** — as implemented in the revised paper:

- **Keep and lead with**: the *ranking* of the three retrieval-expansion
  methods, presented as a preliminary decomposition with intervals shown, plus
  the observation that they separate as a group from the three filtering
  methods. Directional, honestly labelled.
- **Keep as the strongest single number**: `semantic_weight=0.0` at −0.384,
  the one arm nominally significant before correction and the one closest to
  being powered. Report it with its interval and its corrected status.
- **Drop as claims**: recency boosting, quality gating, doc demotion. State
  that the study cannot measure effects of this size and give the required N.
  Bead 55 put this exactly right — three solid rows and an honest gap beat six
  rows of which half are noise.
- **Demote**: the aggregate effect, now +0.072 F1 on the reported tasks after
  the Flask contamination was removed, at p = 0.395 with an interval that
  admits harm.

## 7. Interaction with bobbin-54

Bead 54 argues the decomposition should be the thesis because "the effects are
large and unambiguous". The effects are large; they are not unambiguous. The
reframe is still right — the decomposition is far more interesting than a
+0.027 aggregate — but it has to be pitched as *a method for decomposing
injection systems, demonstrated on a pilot* rather than as settled magnitudes.
That framing survives review. "Semantic search contributes −0.384" as a bare
claim does not.
