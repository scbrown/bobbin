"""Metrics collection and aggregation for tambour.

This module provides tool use metrics collection and storage for context
intelligence. It subscribes to tool.* events from the event dispatcher
and persists them to JSONL storage for later analysis.

Architecture:
    Tambour Event Dispatcher
            | tool.used event
            v
    metrics-collector plugin (collector module)
            |
            v
    .tambour/metrics.jsonl
            |
            v (on-demand)
    metrics-aggregator (aggregator module)
            |
            v
    .tambour/metrics-agg.json (cached aggregations)
"""

from tambour.metrics.collector import MetricsCollector, MetricEvent
from tambour.metrics.extractors import extract_tool_fields
from tambour.metrics.aggregator import (
    MetricsAggregator,
    AggregationResult,
    FileStats,
    SessionStats,
    ToolStats,
    compute,
)

__all__ = [
    # Collection
    "MetricsCollector",
    "MetricEvent",
    "extract_tool_fields",
    # Aggregation
    "MetricsAggregator",
    "AggregationResult",
    "FileStats",
    "SessionStats",
    "ToolStats",
    "compute",
]
