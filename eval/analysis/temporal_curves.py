#!/usr/bin/env python3
"""Generate paper-quality temporal decay curves from sweep results.

Reads the JSON output from temporal_sweep.py and produces SVG figures
showing how F1/precision/recall change with coupling_depth and recency_weight.

Usage::

    python3 analysis/temporal_curves.py results/temporal-sweep.json
    python3 analysis/temporal_curves.py results/temporal-sweep.json --output-dir results/figures/
"""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path

import matplotlib
matplotlib.use("Agg")

import matplotlib.pyplot as plt  # noqa: E402
import numpy as np  # noqa: E402

# Dracula palette for consistency with other eval charts
DRACULA = {
    "bg": "#282a36",
    "fg": "#f8f8f2",
    "purple": "#bd93f9",
    "green": "#50fa7b",
    "cyan": "#8be9fd",
    "red": "#ff5555",
    "yellow": "#f1fa8c",
    "orange": "#ffb86c",
    "pink": "#ff79c6",
    "comment": "#6272a4",
    "current_line": "#44475a",
}

METRIC_COLORS = {
    "f1": DRACULA["green"],
    "precision": DRACULA["cyan"],
    "recall": DRACULA["purple"],
}


def apply_theme() -> None:
    """Set matplotlib rcParams for Dracula-themed plots."""
    plt.rcParams.update({
        "figure.facecolor": DRACULA["bg"],
        "axes.facecolor": DRACULA["bg"],
        "axes.edgecolor": DRACULA["comment"],
        "axes.labelcolor": DRACULA["fg"],
        "axes.grid": True,
        "grid.color": DRACULA["current_line"],
        "grid.alpha": 0.5,
        "text.color": DRACULA["fg"],
        "xtick.color": DRACULA["fg"],
        "ytick.color": DRACULA["fg"],
        "legend.facecolor": DRACULA["current_line"],
        "legend.edgecolor": DRACULA["comment"],
        "legend.labelcolor": DRACULA["fg"],
        "savefig.facecolor": DRACULA["bg"],
        "savefig.edgecolor": DRACULA["bg"],
        "svg.fonttype": "none",
        "font.family": "sans-serif",
        "font.size": 11,
    })


def aggregate_by_value(probes: list[dict]) -> dict[float, dict[str, float]]:
    """Aggregate probe results by sweep_value, computing mean metrics."""
    by_value: dict[float, list[dict]] = {}
    for p in probes:
        if p.get("error"):
            continue
        v = p["sweep_value"]
        by_value.setdefault(v, []).append(p)

    agg: dict[float, dict[str, float]] = {}
    for v in sorted(by_value.keys()):
        ps = by_value[v]
        n = len(ps)
        agg[v] = {
            "f1": sum(p["f1"] for p in ps) / n,
            "precision": sum(p["precision"] for p in ps) / n,
            "recall": sum(p["recall"] for p in ps) / n,
            "n": n,
        }
    return agg


def aggregate_by_repo(probes: list[dict]) -> dict[str, dict[float, dict[str, float]]]:
    """Aggregate probes by repo then by sweep_value."""
    by_repo: dict[str, list[dict]] = {}
    for p in probes:
        if p.get("error"):
            continue
        by_repo.setdefault(p["repo"], []).append(p)

    result: dict[str, dict[float, dict[str, float]]] = {}
    for repo, ps in sorted(by_repo.items()):
        result[repo] = aggregate_by_value(ps)
    return result


def plot_sweep_curve(
    agg: dict[float, dict[str, float]],
    *,
    xlabel: str,
    title: str,
    output_path: Path,
    xlog: bool = False,
) -> None:
    """Plot F1/precision/recall vs sweep parameter."""
    values = sorted(agg.keys())
    f1s = [agg[v]["f1"] for v in values]
    precisions = [agg[v]["precision"] for v in values]
    recalls = [agg[v]["recall"] for v in values]

    fig, ax = plt.subplots(figsize=(8, 5))

    ax.plot(values, f1s, "o-", color=METRIC_COLORS["f1"], linewidth=2,
            markersize=8, label="F1", zorder=3)
    ax.plot(values, precisions, "s--", color=METRIC_COLORS["precision"],
            linewidth=1.5, markersize=6, label="Precision", alpha=0.8)
    ax.plot(values, recalls, "^--", color=METRIC_COLORS["recall"],
            linewidth=1.5, markersize=6, label="Recall", alpha=0.8)

    if xlog:
        # Use symlog to handle 0 values gracefully
        ax.set_xscale("symlog", linthresh=50)

    ax.set_xlabel(xlabel)
    ax.set_ylabel("Score")
    ax.set_title(title)
    ax.set_ylim(-0.02, 1.02)
    ax.legend(loc="best")

    fig.tight_layout()
    fig.savefig(output_path, format="svg", dpi=150)
    plt.close(fig)
    print(f"  Saved: {output_path}")


def plot_per_repo_curves(
    by_repo: dict[str, dict[float, dict[str, float]]],
    *,
    xlabel: str,
    title: str,
    output_path: Path,
    xlog: bool = False,
) -> None:
    """Plot F1 curves per repo on a single chart."""
    repo_colors = [DRACULA["green"], DRACULA["cyan"], DRACULA["purple"],
                   DRACULA["orange"], DRACULA["pink"], DRACULA["yellow"]]

    fig, ax = plt.subplots(figsize=(8, 5))

    for i, (repo, agg) in enumerate(by_repo.items()):
        values = sorted(agg.keys())
        f1s = [agg[v]["f1"] for v in values]
        short_name = repo.split("/")[-1]
        color = repo_colors[i % len(repo_colors)]
        ax.plot(values, f1s, "o-", color=color, linewidth=1.5,
                markersize=6, label=short_name)

    if xlog:
        ax.set_xscale("symlog", linthresh=50)

    ax.set_xlabel(xlabel)
    ax.set_ylabel("F1 Score")
    ax.set_title(title)
    ax.set_ylim(-0.02, 1.02)
    ax.legend(loc="best")

    fig.tight_layout()
    fig.savefig(output_path, format="svg", dpi=150)
    plt.close(fig)
    print(f"  Saved: {output_path}")


def main() -> None:
    parser = argparse.ArgumentParser(description="Generate temporal decay curve figures")
    parser.add_argument("input", help="Path to temporal-sweep.json")
    parser.add_argument("--output-dir", default=None,
                        help="Output directory for SVG figures (default: same dir as input)")
    args = parser.parse_args()

    data = json.loads(Path(args.input).read_text())
    out_dir = Path(args.output_dir) if args.output_dir else Path(args.input).parent / "figures"
    out_dir.mkdir(parents=True, exist_ok=True)

    apply_theme()

    # Coupling depth curves
    if "coupling_depth" in data:
        probes = data["coupling_depth"]["probes"]
        agg = aggregate_by_value(probes)
        print(f"\nCoupling depth sweep ({len(probes)} probes):")
        for v in sorted(agg.keys()):
            m = agg[v]
            print(f"  depth={v:>6.0f}  F1={m['f1']:.3f}  P={m['precision']:.3f}"
                  f"  R={m['recall']:.3f}  (n={m['n']:.0f})")

        plot_sweep_curve(
            agg, xlabel="Coupling Depth (commits)",
            title="Search Quality vs. Coupling Depth",
            output_path=out_dir / "coupling-depth-curve.svg",
            xlog=True,
        )

        by_repo = aggregate_by_repo(probes)
        if len(by_repo) > 1:
            plot_per_repo_curves(
                by_repo, xlabel="Coupling Depth (commits)",
                title="F1 vs. Coupling Depth (per repo)",
                output_path=out_dir / "coupling-depth-per-repo.svg",
                xlog=True,
            )

    # Recency weight curves
    if "recency_weight" in data:
        probes = data["recency_weight"]["probes"]
        agg = aggregate_by_value(probes)
        print(f"\nRecency weight sweep ({len(probes)} probes):")
        for v in sorted(agg.keys()):
            m = agg[v]
            print(f"  weight={v:>5.1f}  F1={m['f1']:.3f}  P={m['precision']:.3f}"
                  f"  R={m['recall']:.3f}  (n={m['n']:.0f})")

        plot_sweep_curve(
            agg, xlabel="Recency Weight",
            title="Search Quality vs. Recency Weight",
            output_path=out_dir / "recency-weight-curve.svg",
        )

        by_repo = aggregate_by_repo(probes)
        if len(by_repo) > 1:
            plot_per_repo_curves(
                by_repo, xlabel="Recency Weight",
                title="F1 vs. Recency Weight (per repo)",
                output_path=out_dir / "recency-weight-per-repo.svg",
            )

    print(f"\nAll figures saved to {out_dir}/")


if __name__ == "__main__":
    main()
