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
    raise NotImplementedError("fig_to_svg: render Figure to SVG via BytesIO, strip XML declaration")


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
    raise NotImplementedError(
        "grouped_bar_chart: create Figure with grouped bars, "
        "X-axis = task labels, bars grouped by approach/series"
    )


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
    raise NotImplementedError(
        "multi_metric_chart: three bar clusters (precision, recall, F1), "
        "one bar per approach, Y-axis 0-100%"
    )


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
    raise NotImplementedError(
        "box_plot_chart: box plots with jittered strip plot overlay, "
        "X-axis = approaches, Y-axis = metric values"
    )


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
    raise NotImplementedError(
        "duration_chart: horizontal bars with error bars, "
        "Y-axis = approaches, X-axis = seconds"
    )


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
    raise NotImplementedError(
        "trend_chart: line chart with markers, "
        "X-axis = run dates, one line per approach"
    )


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
    raise NotImplementedError(
        "heatmap_chart: tasks on Y-axis, runs on X-axis, "
        "color intensity = metric value, Dracula diverging colormap"
    )
