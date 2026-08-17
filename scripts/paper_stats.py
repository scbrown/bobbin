#!/usr/bin/env python3
"""Significance and power for the context-injection paper's ablation arms.

Regenerates every figure in ``docs/plans/paper-statistics.md`` and the
statistics reported in ``docs/paper-context-injection.md`` §5.2 (bobbin-55).

The inputs are the means, standard deviations and run counts already published
in the paper's Table 4, transcribed once below.  They are kept here rather than
recomputed from ``eval/results/`` deliberately: the point of this script is to
show what the *reported* numbers do and do not support, so a reader can check
the arithmetic without the raw runs.  If the study is re-run, update ARMS and
BASELINE from the new report and re-run this.

Usage:
    python3 scripts/paper_stats.py

Requires scipy.
"""

from __future__ import annotations

import math
import sys

try:
    from scipy import stats
except ImportError:  # pragma: no cover - guidance path
    sys.exit("scipy required: pip install scipy")

# --- Inputs, transcribed from docs/paper-context-injection.md Table 4 --------

# (label, N, mean F1, sd)
BASELINE = ("with-bobbin", 7, 0.636, 0.347)
NO_BOBBIN = ("no-bobbin", 5, 0.324, 0.021)

ARMS = [
    ("semantic_weight=0.0", 4, 0.252, 0.134),
    ("blame_bridging=false", 3, 0.333, 0.000),
    ("coupling_depth=0", 3, 0.389, 0.096),
    ("doc_demotion=0.0", 3, 0.556, 0.385),
    ("recency_weight=0.0", 3, 0.611, 0.347),
    ("gate_threshold=1.0", 3, 0.611, 0.347),
]

ALPHA = 0.05


def welch(m1: float, s1: float, n1: int, m2: float, s2: float, n2: int):
    """Welch's t-test. Returns (delta, t, df, p, ci_low, ci_high).

    Unequal variances is not a stylistic choice here — the arm SDs span
    0.000 to 0.385, so Student's pooled-variance form is not applicable.
    """
    se = math.sqrt(s1**2 / n1 + s2**2 / n2)
    t = (m1 - m2) / se
    num = (s1**2 / n1 + s2**2 / n2) ** 2
    den = (s1**2 / n1) ** 2 / (n1 - 1) + (s2**2 / n2) ** 2 / (n2 - 1)
    df = num / den
    p = 2 * (1 - stats.t.cdf(abs(t), df))
    crit = stats.t.ppf(1 - ALPHA / 2, df)
    return (m1 - m2), t, df, p, (m1 - m2) - crit * se, (m1 - m2) + crit * se


def n_for_power(delta: float, sd: float, power: float = 0.80) -> int | None:
    """Smallest per-arm N reaching `power` for a two-sample test of `delta`."""
    if abs(delta) < 1e-9:
        return None
    effect = abs(delta) / sd
    for n in range(2, 200_000):
        df = 2 * n - 2
        crit = stats.t.ppf(1 - ALPHA / 2, df)
        ncp = effect * math.sqrt(n / 2)
        achieved = 1 - stats.nct.cdf(crit, df, ncp) + stats.nct.cdf(-crit, df, ncp)
        if achieved >= power:
            return n
    return None


def main() -> None:
    _, bn, bm, bsd = BASELINE

    print("Ablation arms vs with-bobbin baseline (Welch, two-sided, a=0.05)\n")
    header = f"{'arm':<22} {'N':>2} {'delta':>7} {'t':>6} {'df':>5} {'p':>6}  95% CI"
    print(header)
    print("-" * len(header))

    results = []
    for name, n, m, sd in ARMS:
        delta, t, df, p, lo, hi = welch(m, sd, n, bm, bsd, bn)
        results.append((name, p))
        crosses = " (crosses 0)" if lo < 0 < hi else ""
        print(
            f"{name:<22} {n:>2} {delta:>+7.3f} {t:>6.2f} {df:>5.2f} "
            f"{p:>6.3f}  [{lo:+.3f}, {hi:+.3f}]{crosses}"
        )

    print(f"\nHolm-Bonferroni across {len(results)} arms (a={ALPHA})\n")
    still_rejecting = True
    for i, (name, p) in enumerate(sorted(results, key=lambda r: r[1])):
        threshold = ALPHA / (len(results) - i)
        reject = still_rejecting and p <= threshold
        if not reject:
            still_rejecting = False
        verdict = "REJECT null" if reject else "retain null (ns)"
        print(f"  {name:<22} p={p:.3f}  threshold={threshold:.4f}  -> {verdict}")

    print("\nBaseline contrast on ruff-001\n")
    _, nn, nm, nsd = NO_BOBBIN
    delta, t, df, p, lo, hi = welch(bm, bsd, bn, nm, nsd, nn)
    print(
        f"  with-bobbin vs no-bobbin: delta={delta:+.3f} t={t:.2f} "
        f"df={df:.2f} p={p:.4f} 95% CI [{lo:+.3f}, {hi:+.3f}]"
    )

    print(f"\nPer-arm N for 80% power at the OBSERVED effect (sd={bsd})\n")
    print("  (lower bounds: pilots that reach significance overestimate effect size)\n")
    for name, n, m, _sd in ARMS:
        needed = n_for_power(m - bm, bsd)
        if needed is None:
            continue
        cohen = abs(m - bm) / bsd
        print(f"  {name:<22} |d|={abs(m - bm):.3f}  Cohen d={cohen:.2f}  N={needed:>5}  (had {n})")


if __name__ == "__main__":
    main()
