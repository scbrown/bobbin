"""Tests for run-based result storage."""

from __future__ import annotations

import pytest


@pytest.mark.skip(reason="Stub — implement in bobbin-anvd")
def test_generate_run_id_format():
    """Verify run ID matches YYYYMMDD-HHMMSS-XXXX format."""


@pytest.mark.skip(reason="Stub — implement in bobbin-anvd")
def test_save_result_creates_run_directory():
    """Verify _save_result creates results/runs/<run_id>/ and saves JSON."""


@pytest.mark.skip(reason="Stub — implement in bobbin-anvd")
def test_save_result_writes_manifest():
    """Verify manifest.json is written with correct metadata."""


@pytest.mark.skip(reason="Stub — implement in bobbin-anvd")
def test_load_results_from_runs_dirs():
    """Verify _load_results scans results/runs/*/*.json."""


@pytest.mark.skip(reason="Stub — implement in bobbin-anvd")
def test_load_results_legacy_fallback():
    """Verify _load_results falls back to results/*.json for old layout."""


@pytest.mark.skip(reason="Stub — implement in bobbin-anvd")
def test_load_all_runs_groups_by_run_id():
    """Verify results are grouped by run_id with correct structure."""
