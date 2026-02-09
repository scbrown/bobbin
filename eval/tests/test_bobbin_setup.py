"""Tests for eval.runner.bobbin_setup module.

Uses mocks for subprocess since we don't invoke real bobbin binary.
"""

from __future__ import annotations

import subprocess
from pathlib import Path
from unittest.mock import MagicMock, patch

import pytest

from runner.bobbin_setup import BobbinSetupError, _find_bobbin, setup_bobbin


class TestFindBobbin:
    def test_found_on_path(self):
        with patch("runner.bobbin_setup.shutil.which", return_value="/usr/bin/bobbin"):
            assert _find_bobbin() == "/usr/bin/bobbin"

    def test_found_in_cargo_bin(self, tmp_path: Path):
        fake_bin = tmp_path / ".cargo" / "bin" / "bobbin"
        fake_bin.parent.mkdir(parents=True)
        fake_bin.touch()

        with (
            patch("runner.bobbin_setup.shutil.which", return_value=None),
            patch("runner.bobbin_setup.Path.home", return_value=tmp_path),
        ):
            assert _find_bobbin() == str(fake_bin)

    def test_not_found_raises(self, tmp_path: Path):
        with (
            patch("runner.bobbin_setup.shutil.which", return_value=None),
            patch("runner.bobbin_setup.Path.home", return_value=tmp_path),
        ):
            with pytest.raises(BobbinSetupError, match="bobbin binary not found"):
                _find_bobbin()


class TestSetupBobbin:
    @pytest.fixture()
    def mock_bobbin(self):
        """Patch _find_bobbin and subprocess.run."""
        with (
            patch("runner.bobbin_setup._find_bobbin", return_value="/usr/bin/bobbin"),
            patch("runner.bobbin_setup.subprocess.run") as mock_run,
        ):
            mock_run.return_value = subprocess.CompletedProcess(
                args=[], returncode=0, stdout="", stderr="",
            )
            yield mock_run

    def test_runs_init_then_index(self, mock_bobbin: MagicMock, tmp_path: Path):
        result = setup_bobbin(str(tmp_path))

        assert mock_bobbin.call_count == 3  # init, index, status --json
        init_cmd = mock_bobbin.call_args_list[0][0][0]
        index_cmd = mock_bobbin.call_args_list[1][0][0]
        status_cmd = mock_bobbin.call_args_list[2][0][0]
        assert init_cmd == ["/usr/bin/bobbin", "init"]
        assert index_cmd == ["/usr/bin/bobbin", "index"]
        assert status_cmd == ["/usr/bin/bobbin", "status", "--json"]
        assert isinstance(result, dict)
        assert "index_duration_seconds" in result

    def test_workspace_used_as_cwd(self, mock_bobbin: MagicMock, tmp_path: Path):
        setup_bobbin(str(tmp_path))

        for c in mock_bobbin.call_args_list:
            assert c[1]["cwd"] == tmp_path

    def test_init_failure_raises(self, mock_bobbin: MagicMock, tmp_path: Path):
        mock_bobbin.side_effect = subprocess.CalledProcessError(
            returncode=1, cmd=["bobbin", "init"], stderr="bad config",
        )

        with pytest.raises(BobbinSetupError, match="bobbin init failed"):
            setup_bobbin(str(tmp_path))

    def test_index_failure_raises(self, mock_bobbin: MagicMock, tmp_path: Path):
        # First call (init) succeeds, second (index) fails.
        mock_bobbin.side_effect = [
            subprocess.CompletedProcess(args=[], returncode=0, stdout="", stderr=""),
            subprocess.CalledProcessError(
                returncode=1, cmd=["bobbin", "index"], stderr="no repo found",
            ),
        ]

        with pytest.raises(BobbinSetupError, match="bobbin index failed"):
            setup_bobbin(str(tmp_path))

    def test_index_timeout_raises(self, mock_bobbin: MagicMock, tmp_path: Path):
        mock_bobbin.side_effect = [
            subprocess.CompletedProcess(args=[], returncode=0, stdout="", stderr=""),
            subprocess.TimeoutExpired(cmd=["bobbin", "index"], timeout=300),
        ]

        with pytest.raises(BobbinSetupError, match="timed out"):
            setup_bobbin(str(tmp_path))

    def test_returns_metadata_with_status(self, mock_bobbin: MagicMock, tmp_path: Path):
        """setup_bobbin returns metadata dict including bobbin status info."""
        import json
        status_json = json.dumps({
            "total_files": 42,
            "total_chunks": 100,
            "total_embeddings": 100,
            "languages": ["Rust", "Python"],
        })
        mock_bobbin.side_effect = [
            subprocess.CompletedProcess(args=[], returncode=0, stdout="", stderr=""),
            subprocess.CompletedProcess(args=[], returncode=0, stdout="", stderr=""),
            subprocess.CompletedProcess(args=[], returncode=0, stdout=status_json, stderr=""),
        ]
        result = setup_bobbin(str(tmp_path))
        assert result["total_files"] == 42
        assert result["total_chunks"] == 100
        assert result["languages"] == ["Rust", "Python"]

    def test_custom_timeout(self, mock_bobbin: MagicMock, tmp_path: Path):
        setup_bobbin(str(tmp_path), timeout=120)

        # The index call should use the custom timeout.
        index_call = mock_bobbin.call_args_list[1]
        assert index_call[1]["timeout"] == 120
