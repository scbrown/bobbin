"""Compare diffs against ground truth (files touched, precision/recall)."""

from __future__ import annotations

import logging
import subprocess
from pathlib import Path

logger = logging.getLogger(__name__)


class DiffScorerError(Exception):
    """Raised when diff scoring encounters a fatal error."""


def _files_changed_in_commit(workspace: Path, commit: str) -> set[str]:
    """Return the set of file paths changed in a single commit.

    Uses ``--root`` to handle root commits (no parent) correctly.
    """
    result = subprocess.run(
        ["git", "diff-tree", "--no-commit-id", "--name-only", "-r", "--root", commit],
        cwd=workspace,
        capture_output=True,
        text=True,
        check=True,
    )
    return {f for f in result.stdout.strip().splitlines() if f}


def _files_changed_between(workspace: Path, base: str, head: str) -> set[str]:
    """Return the set of file paths changed between two commits."""
    result = subprocess.run(
        ["git", "diff", "--name-only", base, head],
        cwd=workspace,
        capture_output=True,
        text=True,
        check=True,
    )
    return {f for f in result.stdout.strip().splitlines() if f}


def score_diff(
    workspace: str,
    ground_truth_commit: str,
    *,
    snapshot: str | None = None,
    baseline: str | None = None,
) -> dict:
    """Compare the workspace diff against the ground truth commit.

    Computes file-level precision and recall by comparing which files the agent
    touched versus which files the ground truth commit touched.

    Parameters
    ----------
    workspace:
        Path to the git working copy.  Must contain both the ground truth commit
        and the agent's snapshot commit.
    ground_truth_commit:
        The commit hash whose changes represent the correct solution.
    snapshot:
        The agent's snapshot commit hash.  If provided, compares files changed
        between the baseline and *snapshot*.  If None, compares the current
        working tree against the baseline.
    baseline:
        The baseline commit to diff against.  If provided, agent files are
        computed as changes between *baseline* and *snapshot* (or working tree).
        This is useful for with-bobbin runs where bobbin setup modifies the
        workspace before the agent starts.  If None, defaults to
        ``ground_truth_commit^`` (the parent commit).

    Returns a dict with keys:
        file_precision     — fraction of agent-touched files that are in ground truth
        file_recall        — fraction of ground truth files that the agent touched
        f1                 — harmonic mean of precision and recall
        files_touched      — sorted list of files the agent changed
        ground_truth_files — sorted list of files in the ground truth commit
        exact_file_match   — whether the file sets are identical
    """
    ws = Path(workspace)

    # Get ground truth files.
    try:
        gt_files = _files_changed_in_commit(ws, ground_truth_commit)
    except subprocess.CalledProcessError as exc:
        raise DiffScorerError(
            f"Cannot read ground truth commit {ground_truth_commit}: {exc.stderr.strip()}"
        ) from exc

    # Determine diff base: use explicit baseline if provided, else parent commit.
    if baseline:
        base = baseline
    else:
        try:
            base = subprocess.run(
                ["git", "rev-parse", f"{ground_truth_commit}^"],
                cwd=ws,
                capture_output=True,
                text=True,
                check=True,
            ).stdout.strip()
        except subprocess.CalledProcessError as exc:
            raise DiffScorerError(
                f"Cannot resolve parent of {ground_truth_commit}: {exc.stderr.strip()}"
            ) from exc

    # Get agent files.
    if snapshot:
        agent_files = _files_changed_between(ws, base, snapshot)
    else:
        # Compare working tree + staged changes against base.
        result = subprocess.run(
            ["git", "diff", "--name-only", base],
            cwd=ws,
            capture_output=True,
            text=True,
            check=True,
        )
        agent_files = {f for f in result.stdout.strip().splitlines() if f}

    # Exclude bobbin/claude infrastructure files from both sets so that
    # tool scaffolding (.bobbin/, .claude/) doesn't affect precision/recall.
    _INFRA_PREFIXES = (".bobbin/", ".claude/")
    agent_files = {f for f in agent_files if not any(f.startswith(p) for p in _INFRA_PREFIXES)}
    gt_files = {f for f in gt_files if not any(f.startswith(p) for p in _INFRA_PREFIXES)}

    # Compute precision / recall.
    if not agent_files:
        precision = 0.0
    else:
        precision = len(agent_files & gt_files) / len(agent_files)

    if not gt_files:
        recall = 0.0
    else:
        recall = len(agent_files & gt_files) / len(gt_files)

    if precision + recall > 0:
        f1 = 2 * precision * recall / (precision + recall)
    else:
        f1 = 0.0

    return {
        "file_precision": round(precision, 4),
        "file_recall": round(recall, 4),
        "f1": round(f1, 4),
        "files_touched": sorted(agent_files),
        "ground_truth_files": sorted(gt_files),
        "exact_file_match": agent_files == gt_files,
    }
