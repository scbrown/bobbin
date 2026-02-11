"""Tests for run-based result storage."""

from __future__ import annotations

import json
import re

import pytest

from runner.cli import _generate_run_id, _load_results, _save_result, _write_manifest


def test_generate_run_id_format():
    """Verify run ID matches YYYYMMDD-HHMMSS-XXXX format."""
    run_id = _generate_run_id()
    assert re.fullmatch(r"\d{8}-\d{6}-[0-9a-f]{4}", run_id), (
        f"run_id {run_id!r} does not match YYYYMMDD-HHMMSS-XXXX pattern"
    )

    # Two consecutive IDs should differ (random suffix).
    run_id2 = _generate_run_id()
    assert run_id != run_id2 or run_id[:15] != run_id2[:15]


def test_save_result_creates_run_directory(tmp_path):
    """Verify _save_result creates results/runs/<run_id>/ and saves JSON."""
    result = {
        "task_id": "flask-001",
        "approach": "no-bobbin",
        "attempt": 0,
        "status": "completed",
    }
    run_id = "20260210-143052-a1b2"
    path = _save_result(result, tmp_path, run_id=run_id)

    assert path.exists()
    assert path.parent.name == run_id
    assert path.parent.parent.name == "runs"
    assert path.name == "flask-001_no-bobbin_0.json"

    saved = json.loads(path.read_text(encoding="utf-8"))
    assert saved["run_id"] == run_id
    assert saved["task_id"] == "flask-001"


def test_save_result_writes_manifest(tmp_path):
    """Verify manifest.json is written with correct metadata."""
    run_id = "20260210-143052-a1b2"
    path = _write_manifest(
        tmp_path,
        run_id,
        started_at="2026-02-10T14:30:52Z",
        completed_at="2026-02-10T15:45:12Z",
        model="claude-sonnet-4-5-20250929",
        budget=100.0,
        timeout=3600,
        index_timeout=600,
        attempts_per_approach=3,
        approaches=["no-bobbin", "with-bobbin"],
        tasks=["flask-001"],
        total_results=6,
        completed_results=6,
    )

    assert path.exists()
    assert path.name == "manifest.json"
    assert path.parent.name == run_id

    manifest = json.loads(path.read_text(encoding="utf-8"))
    assert manifest["run_id"] == run_id
    assert manifest["agent_config"]["model"] == "claude-sonnet-4-5-20250929"
    assert manifest["agent_config"]["budget_usd"] == 100.0
    assert manifest["agent_config"]["timeout_seconds"] == 3600
    assert manifest["agent_config"]["index_timeout_seconds"] == 600
    assert manifest["attempts_per_approach"] == 3
    assert manifest["approaches"] == ["no-bobbin", "with-bobbin"]
    assert manifest["tasks"] == ["flask-001"]
    assert manifest["total_results"] == 6
    assert manifest["completed_results"] == 6


def test_load_results_from_runs_dirs(tmp_path):
    """Verify _load_results scans results/runs/*/*.json."""
    run_dir = tmp_path / "runs" / "20260210-143052-a1b2"
    run_dir.mkdir(parents=True)

    result = {
        "task_id": "flask-001",
        "approach": "no-bobbin",
        "attempt": 0,
        "status": "completed",
        "run_id": "20260210-143052-a1b2",
    }
    (run_dir / "flask-001_no-bobbin_0.json").write_text(
        json.dumps(result), encoding="utf-8"
    )

    # Manifest should be skipped (no "status" key).
    manifest = {"run_id": "20260210-143052-a1b2", "started_at": "..."}
    (run_dir / "manifest.json").write_text(
        json.dumps(manifest), encoding="utf-8"
    )

    loaded = _load_results(tmp_path)
    assert len(loaded) == 1
    assert loaded[0]["task_id"] == "flask-001"
    assert loaded[0]["run_id"] == "20260210-143052-a1b2"


def test_load_results_legacy_fallback(tmp_path):
    """Verify _load_results falls back to results/*.json for old layout."""
    result = {
        "task_id": "flask-001",
        "approach": "with-bobbin",
        "attempt": 0,
        "status": "completed",
    }
    (tmp_path / "flask-001_with-bobbin_0.json").write_text(
        json.dumps(result), encoding="utf-8"
    )

    loaded = _load_results(tmp_path)
    assert len(loaded) == 1
    assert loaded[0]["task_id"] == "flask-001"


def test_load_all_runs_groups_by_run_id(tmp_path):
    """Verify results are grouped by run_id with correct structure."""
    # Create two runs with different IDs.
    for run_id in ("20260210-143052-a1b2", "20260215-091230-c3d4"):
        run_dir = tmp_path / "runs" / run_id
        run_dir.mkdir(parents=True)
        result = {
            "task_id": "flask-001",
            "approach": "no-bobbin",
            "attempt": 0,
            "status": "completed",
            "run_id": run_id,
        }
        (run_dir / "flask-001_no-bobbin_0.json").write_text(
            json.dumps(result), encoding="utf-8"
        )

    loaded = _load_results(tmp_path)
    assert len(loaded) == 2

    # Group by run_id to verify structure.
    by_run: dict[str, list[dict]] = {}
    for r in loaded:
        by_run.setdefault(r["run_id"], []).append(r)
    assert len(by_run) == 2
    assert "20260210-143052-a1b2" in by_run
    assert "20260215-091230-c3d4" in by_run
