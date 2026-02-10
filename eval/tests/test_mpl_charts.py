"""Tests for matplotlib chart generation module."""

from __future__ import annotations

import matplotlib
import matplotlib.pyplot as plt

from analysis.mpl_charts import (
    DRACULA,
    apply_dracula_theme,
    box_plot_chart,
    duration_chart,
    fig_to_svg,
    grouped_bar_chart,
    heatmap_chart,
    multi_metric_chart,
    trend_chart,
)


def test_apply_dracula_theme():
    """Verify Dracula theme sets expected rcParams."""
    apply_dracula_theme()
    assert plt.rcParams["figure.facecolor"] == DRACULA["bg"]
    assert plt.rcParams["axes.facecolor"] == DRACULA["bg"]
    assert plt.rcParams["text.color"] == DRACULA["fg"]
    assert plt.rcParams["svg.fonttype"] == "none"
    assert plt.rcParams["axes.grid"] is True


def test_fig_to_svg_returns_svg_string():
    """Verify fig_to_svg returns valid SVG markup."""
    apply_dracula_theme()
    fig, ax = plt.subplots()
    ax.plot([1, 2, 3], [1, 2, 3])
    svg = fig_to_svg(fig)
    assert isinstance(svg, str)
    assert "<svg" in svg
    assert "</svg>" in svg
    # Should not have XML declaration.
    assert not svg.startswith("<?xml")


def test_grouped_bar_chart_basic():
    """Verify grouped bar chart with two approaches and two tasks."""
    groups = [
        {"label": "flask-001", "values": {"no-bobbin": 0.8, "with-bobbin": 0.9}},
        {"label": "ruff-001", "values": {"no-bobbin": 0.6, "with-bobbin": 0.75}},
    ]
    svg = grouped_bar_chart(groups, title="F1 by Task")
    assert isinstance(svg, str)
    assert "<svg" in svg
    assert "F1 by Task" in svg


def test_multi_metric_chart_two_approaches():
    """Verify multi-metric chart shows precision, recall, F1."""
    stats = {
        "no-bobbin": {"avg_file_precision": 0.7, "avg_file_recall": 0.8, "avg_f1": 0.75},
        "with-bobbin": {"avg_file_precision": 0.9, "avg_file_recall": 0.85, "avg_f1": 0.87},
    }
    svg = multi_metric_chart(stats, title="Metric Comparison")
    assert isinstance(svg, str)
    assert "<svg" in svg
    assert "Precision" in svg
    assert "Recall" in svg
    assert "F1" in svg


def test_box_plot_chart_with_attempts():
    """Verify box plot chart with multiple attempts per approach."""
    data = {
        "no-bobbin": [0.6, 0.7, 0.65, 0.72, 0.68],
        "with-bobbin": [0.8, 0.85, 0.82, 0.9, 0.87],
    }
    svg = box_plot_chart(data, metric_name="F1", title="F1 Distribution")
    assert isinstance(svg, str)
    assert "<svg" in svg
    assert "F1 Distribution" in svg


def test_duration_chart_basic():
    """Verify duration chart with two approaches."""
    durations = {
        "no-bobbin": [120, 150, 130],
        "with-bobbin": [90, 100, 95],
    }
    svg = duration_chart(durations, title="Duration Comparison")
    assert isinstance(svg, str)
    assert "<svg" in svg
    assert "Duration" in svg


def test_trend_chart_multiple_runs():
    """Verify trend chart across 3+ historical runs."""
    runs = [
        {"run_id": "run-1", "date": "Feb 1", "values": {"no-bobbin": 0.6, "with-bobbin": 0.7}},
        {"run_id": "run-2", "date": "Feb 5", "values": {"no-bobbin": 0.65, "with-bobbin": 0.75}},
        {"run_id": "run-3", "date": "Feb 10", "values": {"no-bobbin": 0.7, "with-bobbin": 0.85}},
    ]
    svg = trend_chart(runs, metric="avg_f1", title="F1 Over Time")
    assert isinstance(svg, str)
    assert "<svg" in svg
    assert "F1 Over Time" in svg


def test_heatmap_chart_basic():
    """Verify heatmap chart with tasks x runs."""
    runs = [
        {"run_id": "run-1", "date": "Feb 1", "tasks": {"flask-001": 0.8, "ruff-001": 0.6}},
        {"run_id": "run-2", "date": "Feb 5", "tasks": {"flask-001": 0.85, "ruff-001": 0.7}},
    ]
    svg = heatmap_chart(runs, metric="F1", title="F1 Heatmap")
    assert isinstance(svg, str)
    assert "<svg" in svg
    assert "F1 Heatmap" in svg


def test_chart_empty_data_handling():
    """Verify all chart functions return empty string for empty data."""
    assert grouped_bar_chart([]) == ""
    assert multi_metric_chart({}) == ""
    assert box_plot_chart({}) == ""
    assert duration_chart({}) == ""
    assert trend_chart([]) == ""
    assert heatmap_chart([]) == ""
