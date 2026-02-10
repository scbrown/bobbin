"""Pure-Python SVG chart generation for eval mdbook pages.

.. deprecated::
    Use :mod:`analysis.mpl_charts` instead. This module will be removed
    after all callers are migrated to matplotlib-based charts.

No external dependencies — generates inline SVG strings using the Dracula
color palette for consistent theming with the mdbook site.
"""

from __future__ import annotations

from typing import Any

# Dracula palette
PURPLE = "#bd93f9"
GREEN = "#50fa7b"
CYAN = "#8be9fd"
RED = "#ff5555"
YELLOW = "#f1fa8c"
ORANGE = "#ffb86c"
FG = "#f8f8f2"
BG = "#282a36"
CURRENT_LINE = "#44475a"
COMMENT = "#6272a4"

# Default approach colors
APPROACH_COLORS = {
    "no-bobbin": PURPLE,
    "with-bobbin": GREEN,
}


def _escape(text: str) -> str:
    """Escape XML special characters."""
    return text.replace("&", "&amp;").replace("<", "&lt;").replace(">", "&gt;").replace('"', "&quot;")


def horizontal_bar(value: float, max_value: float, color: str = GREEN, width: int = 120, height: int = 16) -> str:
    """Generate an inline SVG horizontal bar (sparkline style).

    Suitable for embedding in markdown table cells.
    """
    if max_value <= 0:
        bar_width = 0
    else:
        bar_width = max(1, int(value / max_value * (width - 4)))

    return (
        f'<svg width="{width}" height="{height}" xmlns="http://www.w3.org/2000/svg">'
        f'<rect x="0" y="0" width="{width}" height="{height}" rx="3" fill="{CURRENT_LINE}"/>'
        f'<rect x="2" y="2" width="{bar_width}" height="{height - 4}" rx="2" fill="{color}"/>'
        f"</svg>"
    )


def grouped_bar_chart(
    groups: list[dict[str, Any]],
    *,
    width: int = 600,
    height: int = 300,
    title: str = "",
) -> str:
    """Generate an inline SVG grouped bar chart.

    Parameters
    ----------
    groups:
        List of dicts, each with:
        - ``label``: group label (e.g. task name)
        - ``values``: dict mapping series name to numeric value
    width, height:
        Chart dimensions in pixels.
    title:
        Optional chart title.

    Returns an SVG string.
    """
    if not groups:
        return ""

    # Determine series names from first group.
    series_names = list(groups[0].get("values", {}).keys())
    n_series = len(series_names)
    n_groups = len(groups)

    if n_series == 0 or n_groups == 0:
        return ""

    margin = {"top": 40 if title else 20, "right": 20, "bottom": 60, "left": 50}
    chart_w = width - margin["left"] - margin["right"]
    chart_h = height - margin["top"] - margin["bottom"]

    # Find max value for scaling.
    all_values = [
        v for g in groups for v in g.get("values", {}).values() if isinstance(v, (int, float))
    ]
    max_val = max(all_values) if all_values else 1.0
    if max_val <= 0:
        max_val = 1.0

    # Bar geometry.
    group_width = chart_w / n_groups
    bar_padding = 4
    bar_width = max(8, (group_width - bar_padding * (n_series + 1)) / n_series)

    # Assign colors.
    colors = [APPROACH_COLORS.get(s, [PURPLE, GREEN, CYAN, ORANGE][i % 4]) for i, s in enumerate(series_names)]

    parts = [
        f'<svg width="{width}" height="{height}" xmlns="http://www.w3.org/2000/svg">',
        f'<rect width="{width}" height="{height}" fill="{BG}" rx="6"/>',
    ]

    if title:
        parts.append(
            f'<text x="{width / 2}" y="24" text-anchor="middle" '
            f'fill="{FG}" font-size="14" font-weight="600">{_escape(title)}</text>'
        )

    # Y-axis gridlines.
    n_ticks = 5
    for i in range(n_ticks + 1):
        y = margin["top"] + chart_h - (i / n_ticks) * chart_h
        val = (i / n_ticks) * max_val
        parts.append(
            f'<line x1="{margin["left"]}" y1="{y:.1f}" '
            f'x2="{margin["left"] + chart_w}" y2="{y:.1f}" '
            f'stroke="{CURRENT_LINE}" stroke-width="1"/>'
        )
        label = f"{val:.0%}" if max_val <= 1.0 else f"{val:.0f}"
        parts.append(
            f'<text x="{margin["left"] - 6}" y="{y + 4:.1f}" '
            f'text-anchor="end" fill="{COMMENT}" font-size="10">{label}</text>'
        )

    # Bars.
    for gi, group in enumerate(groups):
        gx = margin["left"] + gi * group_width
        values = group.get("values", {})

        for si, series in enumerate(series_names):
            val = values.get(series, 0)
            bar_h = (val / max_val) * chart_h if max_val > 0 else 0
            bx = gx + bar_padding + si * (bar_width + bar_padding)
            by = margin["top"] + chart_h - bar_h

            parts.append(
                f'<rect x="{bx:.1f}" y="{by:.1f}" width="{bar_width:.1f}" '
                f'height="{bar_h:.1f}" fill="{colors[si]}" rx="2"/>'
            )

            # Value label above bar.
            if bar_h > 0:
                label = f"{val:.0%}" if max_val <= 1.0 else f"{val:.0f}"
                parts.append(
                    f'<text x="{bx + bar_width / 2:.1f}" y="{by - 4:.1f}" '
                    f'text-anchor="middle" fill="{colors[si]}" font-size="9">{label}</text>'
                )

        # Group label.
        label_x = gx + group_width / 2
        label_y = margin["top"] + chart_h + 16
        parts.append(
            f'<text x="{label_x:.1f}" y="{label_y:.1f}" text-anchor="middle" '
            f'fill="{FG}" font-size="11">{_escape(group.get("label", ""))}</text>'
        )

    # Legend.
    legend_y = height - 12
    legend_x = margin["left"]
    for si, series in enumerate(series_names):
        x = legend_x + si * 140
        parts.append(
            f'<rect x="{x}" y="{legend_y - 8}" width="12" height="12" rx="2" fill="{colors[si]}"/>'
        )
        parts.append(
            f'<text x="{x + 16}" y="{legend_y + 2}" fill="{FG}" font-size="11">'
            f"{_escape(series)}</text>"
        )

    parts.append("</svg>")
    return "\n".join(parts)
