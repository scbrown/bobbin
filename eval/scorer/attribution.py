"""Serving-model attribution for eval run artifacts.

The eval study documented in ``docs/plans/paper-statistics.md`` §4b was
model-mixed because run artifacts did not record which model actually served
them, and the scorer pooled across the mixture.  This module is the single
implementation of the fix (bobbin-daa), shared by:

* the runner (``runner/cli.py``), which persists ``serving_model`` into every
  run artifact at write time, extracted from the agent's own usage record —
  never copied from the config that requested a model, because a field that
  declares intent is exactly what already failed;
* the scorer (``runner/cli.py score`` and ``scorer/aggregator.py``), which
  treats the serving model as part of a run's identity and refuses to pool
  runs whose serving models differ;
* the post-hoc census (``scripts/paper_census.py``), whose extraction logic
  this module absorbs.

A run without a usage record is *unattributed* (``None``).  It is excluded
from cross-arm comparison and reported as excluded — it is never assumed to
match the model the run was configured to use.
"""

from __future__ import annotations

from collections import Counter
from typing import Any

# Models that appear in Claude Code's usage record as secondary/internal
# helpers and are never the agent under test.
_SECONDARY_MARKERS = ("haiku",)


class ServingModelMismatch(ValueError):
    """Raised when runs with differing serving models would be pooled."""


def primary_models(model_usage: dict[str, Any] | None) -> list[str]:
    """Non-secondary model names from a Claude Code usage record, sorted.

    Haiku appears in ``modelUsage`` as Claude Code's own internal helper and
    is never the agent under test, so it is skipped whenever any primary
    model is present.
    """
    if not isinstance(model_usage, dict):
        return []
    return sorted(m for m in model_usage if not any(s in m for s in _SECONDARY_MARKERS))


def serving_model_from_usage(model_usage: dict[str, Any] | None) -> str | None:
    """The model that actually served a run, from its own usage record.

    Returns ``None`` — unattributed — when the usage record is absent or
    contains only secondary models.  A run served by more than one primary
    model gets a joined ``a+b`` label so its mixed identity is preserved
    rather than collapsed onto one of its parts.
    """
    primaries = primary_models(model_usage)
    if not primaries:
        return None
    return "+".join(primaries)


def record_serving_model(record: dict[str, Any]) -> str | None:
    """The serving model of one run artifact, or ``None`` if unattributed.

    Prefers the write-time ``serving_model`` field (present in artifacts
    written after bobbin-daa; an explicit ``null`` there means the run had no
    usage record and stays unattributed).  Older artifacts fall back to
    extraction from ``agent_result.model_usage``.  There is deliberately no
    fallback to the configured or manifest-declared model.
    """
    if "serving_model" in record:
        return record["serving_model"]
    usage = (record.get("agent_result") or {}).get("model_usage")
    return serving_model_from_usage(usage)


def partition_by_attribution(
    records: list[dict[str, Any]],
) -> tuple[list[dict[str, Any]], list[dict[str, Any]]]:
    """Split *records* into (attributed, unattributed)."""
    attributed: list[dict[str, Any]] = []
    unattributed: list[dict[str, Any]] = []
    for r in records:
        (attributed if record_serving_model(r) is not None else unattributed).append(r)
    return attributed, unattributed


def serving_model_counts(records: list[dict[str, Any]]) -> Counter:
    """Counter of serving models over the *attributed* runs in *records*."""
    return Counter(
        m for m in (record_serving_model(r) for r in records) if m is not None
    )


def describe_mix(counts_by_arm: dict[str, Counter]) -> str:
    """Human-readable per-arm serving-model counts for error messages."""
    lines = []
    for arm_name in sorted(counts_by_arm):
        counts = counts_by_arm[arm_name]
        rendered = ", ".join(f"{m}: {c}" for m, c in sorted(counts.items())) or "none attributed"
        lines.append(f"  {arm_name}: {rendered}")
    return "\n".join(lines)


def check_comparable(runs_by_arm: dict[str, list[dict[str, Any]]]) -> dict[str, Counter]:
    """Verify every attributed run across *runs_by_arm* shares one serving model.

    Unattributed runs are ignored here — callers must exclude them from the
    comparison and report the exclusion; they never count as matching.

    Returns per-arm serving-model counts on success.  Raises
    :class:`ServingModelMismatch` naming the mismatched models and per-arm
    counts when more than one serving model appears, within or across arms.
    """
    counts_by_arm = {arm: serving_model_counts(runs) for arm, runs in runs_by_arm.items()}
    distinct = {m for counts in counts_by_arm.values() for m in counts}
    if len(distinct) > 1:
        raise ServingModelMismatch(
            "refusing to aggregate arms served by different models "
            f"({', '.join(sorted(distinct))}); serving model is part of a run's "
            "identity (see docs/plans/paper-statistics.md §4b).\n"
            "Runs per arm by serving model:\n" + describe_mix(counts_by_arm)
        )
    return counts_by_arm
