"""Cross-reference injected files against files the agent actually touched.

Measures whether the context injected by bobbin was actually consumed by the
agent.  This is distinct from the ground-truth overlap (which measures whether
the *right* files were injected) — here we measure whether injected files were
*used*.

Metrics produced:
    injection_precision — fraction of injected files the agent touched
    injection_recall    — fraction of agent-touched files that were injected
    injection_f1        — harmonic mean of precision and recall
"""

from __future__ import annotations

import logging

logger = logging.getLogger(__name__)


def score_injection_usage(
    injected_files: list[str],
    files_touched: list[str],
) -> dict:
    """Compare injected context files against agent-touched files.

    Parameters
    ----------
    injected_files:
        File paths that were injected into the agent's context by bobbin
        (from ``bobbin_metrics["injected_files"]``).  Expected to be
        workspace-relative paths.
    files_touched:
        File paths the agent actually modified (from
        ``diff_result["files_touched"]``).  Expected to be workspace-relative
        paths.

    Returns a dict with keys:
        injection_precision — |injected ∩ touched| / |injected|
        injection_recall    — |injected ∩ touched| / |touched|
        injection_f1        — harmonic mean of precision and recall
        injected_and_touched — sorted list of files in both sets
        injected_not_touched — sorted list of injected files agent ignored
        touched_not_injected — sorted list of agent-touched files not injected
    """
    injected = set(injected_files)
    touched = set(files_touched)
    overlap = injected & touched

    if not injected:
        precision = 0.0
    else:
        precision = len(overlap) / len(injected)

    if not touched:
        recall = 0.0
    else:
        recall = len(overlap) / len(touched)

    if precision + recall > 0:
        f1 = 2 * precision * recall / (precision + recall)
    else:
        f1 = 0.0

    return {
        "injection_precision": round(precision, 4),
        "injection_recall": round(recall, 4),
        "injection_f1": round(f1, 4),
        "injected_and_touched": sorted(overlap),
        "injected_not_touched": sorted(injected - touched),
        "touched_not_injected": sorted(touched - injected),
    }
