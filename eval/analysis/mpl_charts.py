"""Matplotlib-based chart generation for eval mdbook pages.

Generates SVG charts using the Dracula color palette for consistent theming
with the mdbook documentation site. Replaces the hand-rolled SVG in svg_charts.py.

All chart functions return SVG strings suitable for embedding in markdown or
saving as .svg files referenced from markdown pages.
"""

from __future__ import annotations

import io
from typing import Any

import matplotlib

matplotlib.use("Agg")  # Headless backend — must be before pyplot import

import matplotlib.pyplot as plt  # noqa: E402
import numpy as np  # noqa: E402
from matplotlib.colors import LinearSegmentedColormap  # noqa: E402
from matplotlib.figure import Figure  # noqa: E402

# Dracula palette
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

# Default approach colors
APPROACH_COLORS = {
    "no-bobbin": DRACULA["purple"],
    "with-bobbin": DRACULA["green"],
}

# Fallback colors for additional approaches
_EXTRA_COLORS = [DRACULA["cyan"], DRACULA["orange"], DRACULA["pink"], DRACULA["yellow"]]


def apply_dracula_theme() -> None:
    """Set matplotlib rcParams for Dracula-themed plots.

    Call this before creating any figures to ensure consistent theming.
    """
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
        "svg.fonttype": "none",  # Use system fonts, don't embed
        "font.family": "sans-serif",
        "font.size": 11,
    })


def _get_approach_color(approach: str, index: int = 0) -> str:
    """Get the color for an approach name."""
    if approach in APPROACH_COLORS:
        return APPROACH_COLORS[approach]
    return _EXTRA_COLORS[index % len(_EXTRA_COLORS)]


def fig_to_svg(fig: Figure) -> str:
    """Render a matplotlib Figure to an inline SVG string.

    Strips the XML declaration so the SVG can be embedded directly
    in HTML or markdown. Closes the figure after rendering.

    Parameters
    ----------
    fig:
        The matplotlib Figure to render.

    Returns
    -------
    str
        SVG markup string.
    """
    buf = io.BytesIO()
    fig.savefig(buf, format="svg", bbox_inches="tight")
    plt.close(fig)
    svg = buf.getvalue().decode("utf-8")
    # Strip XML declaration for inline embedding.
    if svg.startswith("<?xml"):
        svg = svg[svg.index("?>") + 2:].lstrip()
    return svg


def grouped_bar_chart(
    groups: list[dict[str, Any]],
    *,
    title: str = "",
    metric_names: list[str] | None = None,
    figsize: tuple[float, float] = (8, 4),
) -> str:
    """Generate a grouped bar chart comparing metrics across tasks and approaches.

    Parameters
    ----------
    groups:
        List of dicts, each with:
        - ``label``: group label (e.g., task name)
        - ``values``: dict mapping series name to numeric value (0.0-1.0 for percentages)
    title:
        Chart title.
    metric_names:
        Optional list of metric names for the legend.
    figsize:
        Figure dimensions in inches.

    Returns
    -------
    str
        SVG string, or empty string if groups is empty.
    """
    if not groups:
        return ""

    apply_dracula_theme()

    labels = [g["label"] for g in groups]
    # Collect all series names across groups.
    series_names: list[str] = []
    for g in groups:
        for name in g["values"]:
            if name not in series_names:
                series_names.append(name)

    if metric_names is None:
        metric_names = series_names

    n_groups = len(labels)
    n_series = len(series_names)
    if n_series == 0:
        return ""

    x = np.arange(n_groups)
    width = 0.8 / n_series

    fig, ax = plt.subplots(figsize=figsize)

    for i, name in enumerate(series_names):
        values = [g["values"].get(name, 0) for g in groups]
        offset = (i - (n_series - 1) / 2) * width
        color = _get_approach_color(name, i)
        label = metric_names[i] if i < len(metric_names) else name
        ax.bar(x + offset, values, width, label=label, color=color)

    ax.set_xticks(x)
    ax.set_xticklabels(labels)
    ax.set_ylabel("Score")
    if title:
        ax.set_title(title)
    ax.legend()
    ax.set_ylim(0, 1.05)

    return fig_to_svg(fig)


def multi_metric_chart(
    stats_by_approach: dict[str, dict[str, float]],
    *,
    title: str = "Metric Comparison",
    figsize: tuple[float, float] = (6, 4),
) -> str:
    """Generate a multi-metric comparison chart.

    Shows precision, recall, and F1 as bar clusters, with one bar per approach.

    Parameters
    ----------
    stats_by_approach:
        Dict mapping approach name to dict with keys:
        ``avg_file_precision``, ``avg_file_recall``, ``avg_f1`` (values 0.0-1.0).
    title:
        Chart title.
    figsize:
        Figure dimensions in inches.

    Returns
    -------
    str
        SVG string, or empty string if stats_by_approach is empty.
    """
    if not stats_by_approach:
        return ""

    apply_dracula_theme()

    metrics = ["avg_file_precision", "avg_file_recall", "avg_f1"]
    metric_labels = ["Precision", "Recall", "F1"]
    approaches = list(stats_by_approach.keys())
    n_approaches = len(approaches)
    n_metrics = len(metrics)

    x = np.arange(n_metrics)
    width = 0.8 / n_approaches

    fig, ax = plt.subplots(figsize=figsize)

    for i, approach in enumerate(approaches):
        stats = stats_by_approach[approach]
        values = [stats.get(m, 0) * 100 for m in metrics]
        offset = (i - (n_approaches - 1) / 2) * width
        color = _get_approach_color(approach, i)
        ax.bar(x + offset, values, width, label=approach, color=color)

    ax.set_xticks(x)
    ax.set_xticklabels(metric_labels)
    ax.set_ylabel("Score (%)")
    ax.set_ylim(0, 105)
    if title:
        ax.set_title(title)
    ax.legend()

    return fig_to_svg(fig)


def box_plot_chart(
    data_by_approach: dict[str, list[float]],
    *,
    metric_name: str = "F1",
    title: str = "",
    figsize: tuple[float, float] = (6, 4),
) -> str:
    """Generate box plots showing metric distribution across attempts.

    Parameters
    ----------
    data_by_approach:
        Dict mapping approach name to list of metric values (one per attempt).
    metric_name:
        Name of the metric (for axis label).
    title:
        Chart title. Defaults to "{metric_name} Distribution".
    figsize:
        Figure dimensions in inches.

    Returns
    -------
    str
        SVG string, or empty string if data_by_approach is empty.
    """
    if not data_by_approach:
        return ""

    apply_dracula_theme()

    if not title:
        title = f"{metric_name} Distribution"

    approaches = list(data_by_approach.keys())
    data = [data_by_approach[a] for a in approaches]

    fig, ax = plt.subplots(figsize=figsize)

    bp = ax.boxplot(
        data,
        tick_labels=approaches,
        patch_artist=True,
        widths=0.5,
        medianprops={"color": DRACULA["fg"], "linewidth": 2},
        whiskerprops={"color": DRACULA["comment"]},
        capprops={"color": DRACULA["comment"]},
        flierprops={"markerfacecolor": DRACULA["red"], "markeredgecolor": DRACULA["red"]},
    )

    for i, (box, approach) in enumerate(zip(bp["boxes"], approaches)):
        color = _get_approach_color(approach, i)
        box.set_facecolor(color)
        box.set_alpha(0.7)

    # Jittered strip plot overlay.
    rng = np.random.default_rng(42)
    for i, values in enumerate(data):
        if values:
            jitter = rng.uniform(-0.15, 0.15, size=len(values))
            ax.scatter(
                np.full(len(values), i + 1) + jitter,
                values,
                color=DRACULA["fg"],
                alpha=0.6,
                s=20,
                zorder=3,
            )

    ax.set_ylabel(metric_name)
    ax.set_title(title)

    return fig_to_svg(fig)


def duration_chart(
    durations_by_approach: dict[str, list[float]],
    *,
    title: str = "Duration Comparison",
    figsize: tuple[float, float] = (6, 3),
) -> str:
    """Generate a horizontal bar chart comparing durations by approach.

    Shows mean duration with error bars (min/max across attempts).

    Parameters
    ----------
    durations_by_approach:
        Dict mapping approach name to list of duration values in seconds.
    title:
        Chart title.
    figsize:
        Figure dimensions in inches.

    Returns
    -------
    str
        SVG string, or empty string if durations_by_approach is empty.
    """
    if not durations_by_approach:
        return ""

    apply_dracula_theme()

    approaches = list(durations_by_approach.keys())
    means = []
    errors_low = []
    errors_high = []
    colors = []

    for i, approach in enumerate(approaches):
        vals = durations_by_approach[approach]
        mean = sum(vals) / len(vals) if vals else 0
        lo = mean - min(vals) if vals else 0
        hi = max(vals) - mean if vals else 0
        means.append(mean)
        errors_low.append(lo)
        errors_high.append(hi)
        colors.append(_get_approach_color(approach, i))

    y = np.arange(len(approaches))

    fig, ax = plt.subplots(figsize=figsize)
    ax.barh(
        y,
        means,
        xerr=[errors_low, errors_high],
        color=colors,
        capsize=4,
        error_kw={"ecolor": DRACULA["fg"], "capthick": 1.5},
        height=0.5,
    )
    ax.set_yticks(y)
    ax.set_yticklabels(approaches)
    ax.set_xlabel("Duration (seconds)")
    if title:
        ax.set_title(title)
    ax.invert_yaxis()

    return fig_to_svg(fig)


def trend_chart(
    runs_data: list[dict[str, Any]],
    *,
    metric: str = "avg_f1",
    title: str = "",
    figsize: tuple[float, float] = (8, 4),
) -> str:
    """Generate a line chart showing a metric across historical runs.

    Parameters
    ----------
    runs_data:
        List of dicts, each with:
        - ``run_id``: run identifier
        - ``date``: display label for X-axis
        - ``values``: dict mapping approach name to metric value
    metric:
        Metric name (for axis label and default title).
    title:
        Chart title. Defaults to "{metric} Over Time".
    figsize:
        Figure dimensions in inches.

    Returns
    -------
    str
        SVG string, or empty string if runs_data is empty.
    """
    if not runs_data:
        return ""

    apply_dracula_theme()

    if not title:
        title = f"{metric} Over Time"

    # Collect all approach names across runs.
    all_approaches: list[str] = []
    for rd in runs_data:
        for approach in rd.get("values", {}):
            if approach not in all_approaches:
                all_approaches.append(approach)

    dates = [rd.get("date", rd.get("run_id", "")) for rd in runs_data]
    x = np.arange(len(dates))

    fig, ax = plt.subplots(figsize=figsize)

    for i, approach in enumerate(all_approaches):
        values = [rd.get("values", {}).get(approach) for rd in runs_data]
        color = _get_approach_color(approach, i)
        ax.plot(x, values, marker="o", color=color, label=approach, linewidth=2, markersize=6)

    ax.set_xticks(x)
    ax.set_xticklabels(dates, rotation=45, ha="right")
    ax.set_ylabel(metric)
    ax.set_title(title)
    ax.legend()
    fig.tight_layout()

    return fig_to_svg(fig)


def heatmap_chart(
    runs_data: list[dict[str, Any]],
    *,
    metric: str = "avg_f1",
    title: str = "",
    figsize: tuple[float, float] = (10, 6),
) -> str:
    """Generate a heatmap of metric values across tasks and runs.

    Parameters
    ----------
    runs_data:
        List of dicts, each with:
        - ``run_id``: run identifier (column label)
        - ``date``: display label
        - ``tasks``: dict mapping task_id to metric value
    metric:
        Metric name (for colorbar label and default title).
    title:
        Chart title. Defaults to "{metric} Heatmap".
    figsize:
        Figure dimensions in inches.

    Returns
    -------
    str
        SVG string, or empty string if runs_data is empty.
    """
    if not runs_data:
        return ""

    apply_dracula_theme()

    if not title:
        title = f"{metric} Heatmap"

    # Collect all task IDs.
    all_tasks: list[str] = []
    for rd in runs_data:
        for task_id in rd.get("tasks", {}):
            if task_id not in all_tasks:
                all_tasks.append(task_id)

    if not all_tasks:
        return ""

    # Build data matrix: rows = tasks, columns = runs.
    data = np.full((len(all_tasks), len(runs_data)), np.nan)
    for j, rd in enumerate(runs_data):
        for i, task_id in enumerate(all_tasks):
            val = rd.get("tasks", {}).get(task_id)
            if val is not None:
                data[i, j] = val

    run_labels = [rd.get("date", rd.get("run_id", "")) for rd in runs_data]

    # Dracula-themed colormap: red → purple → green.
    cmap = LinearSegmentedColormap.from_list(
        "dracula",
        [DRACULA["red"], DRACULA["purple"], DRACULA["green"]],
    )

    fig, ax = plt.subplots(figsize=figsize)
    im = ax.imshow(data, cmap=cmap, aspect="auto", vmin=0, vmax=1)

    ax.set_xticks(np.arange(len(run_labels)))
    ax.set_xticklabels(run_labels, rotation=45, ha="right")
    ax.set_yticks(np.arange(len(all_tasks)))
    ax.set_yticklabels(all_tasks)

    # Annotate cells with values.
    for i in range(len(all_tasks)):
        for j in range(len(runs_data)):
            val = data[i, j]
            if not np.isnan(val):
                text_color = DRACULA["bg"] if val > 0.6 else DRACULA["fg"]
                ax.text(j, i, f"{val:.2f}", ha="center", va="center", color=text_color, fontsize=9)

    cbar = fig.colorbar(im, ax=ax)
    cbar.set_label(metric, color=DRACULA["fg"])
    cbar.ax.yaxis.set_tick_params(color=DRACULA["fg"])
    plt.setp(cbar.ax.yaxis.get_ticklabels(), color=DRACULA["fg"])

    ax.set_title(title)
    fig.tight_layout()

    return fig_to_svg(fig)
