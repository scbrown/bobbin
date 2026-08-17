# Significance and power for the ablation arms — bobbin-55

> **Status (2026-08-17):** computed. Every figure below is derived from the
> means, standard deviations and run counts already reported in
> `docs/paper-context-injection.md` Table 4. Regenerate with
> `scripts/paper_stats.py`.

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

The 66-run aggregate (F1 0.695 → 0.722) cannot be tested at all: **Table 1
reports no standard deviations and no per-run values.** That is a reporting
defect independent of sample size, and it must be fixed before submission — an
aggregate mean with no dispersion is not a result a reviewer can evaluate.

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
- **Demote**: the aggregate +0.027 F1, which cannot be tested for lack of SDs
  and would not survive if it could.

## 7. Interaction with bobbin-54

Bead 54 argues the decomposition should be the thesis because "the effects are
large and unambiguous". The effects are large; they are not unambiguous. The
reframe is still right — the decomposition is far more interesting than a
+0.027 aggregate — but it has to be pitched as *a method for decomposing
injection systems, demonstrated on a pilot* rather than as settled magnitudes.
That framing survives review. "Semantic search contributes −0.384" as a bare
claim does not.
