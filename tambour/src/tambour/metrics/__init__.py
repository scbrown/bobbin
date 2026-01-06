"""Metrics collection for tambour.

This module provides tool use metrics collection and storage for context
intelligence. It subscribes to tool.* events from the event dispatcher
and persists them to JSONL storage for later analysis.

Architecture:
    Tambour Event Dispatcher
            | tool.used event
            v
    metrics-collector plugin (this module)
            |
            v
    .tambour/metrics.jsonl
"""

from tambour.metrics.collector import MetricsCollector, MetricEvent
from tambour.metrics.extractors import extract_tool_fields

__all__ = [
    "MetricsCollector",
    "MetricEvent",
    "extract_tool_fields",
]
