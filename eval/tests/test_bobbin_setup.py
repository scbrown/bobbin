"""Tests for eval.runner.bobbin_setup module.

Uses mocks for subprocess since we don't invoke real bobbin binary.
"""

from __future__ import annotations

import subprocess
from pathlib import Path
from unittest.mock import MagicMock, patch

import pytest

from runner.bobbin_setup import BobbinSetupError, _find_bobbin, _parse_profile, setup_bobbin


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
        """Patch _find_bobbin, subprocess.run, and subprocess.Popen.

        setup_bobbin uses subprocess.run for init and status, and
        subprocess.Popen for index (to stream stderr progress).
        """
        popen_inst = MagicMock()
        popen_inst.stderr = iter([])
        popen_inst.stdout.read.return_value = ""
        popen_inst.wait.return_value = None
        popen_inst.returncode = 0

        with (
            patch("runner.bobbin_setup._find_bobbin", return_value="/usr/bin/bobbin"),
            patch("runner.bobbin_setup.subprocess.run") as mock_run,
            patch("runner.bobbin_setup.subprocess.Popen") as mock_popen,
        ):
            mock_run.return_value = subprocess.CompletedProcess(
                args=[], returncode=0, stdout="", stderr="",
            )
            mock_popen.return_value = popen_inst
            yield mock_run, mock_popen

    def test_runs_init_then_index(self, mock_bobbin, tmp_path: Path):
        mock_run, mock_popen = mock_bobbin
        result = setup_bobbin(str(tmp_path))

        # init + status via subprocess.run
        assert mock_run.call_count == 2
        init_cmd = mock_run.call_args_list[0][0][0]
        status_cmd = mock_run.call_args_list[1][0][0]
        assert init_cmd == ["/usr/bin/bobbin", "init"]
        assert status_cmd == ["/usr/bin/bobbin", "status", "--json"]

        # index via subprocess.Popen
        assert mock_popen.call_count == 1
        index_cmd = mock_popen.call_args[0][0]
        assert index_cmd == ["/usr/bin/bobbin", "index", "--verbose"]

        assert isinstance(result, dict)
        assert "index_duration_seconds" in result

    def test_workspace_used_as_cwd(self, mock_bobbin, tmp_path: Path):
        mock_run, mock_popen = mock_bobbin
        setup_bobbin(str(tmp_path))

        for c in mock_run.call_args_list:
            assert c[1]["cwd"] == tmp_path
        assert mock_popen.call_args[1]["cwd"] == tmp_path

    def test_init_failure_raises(self, mock_bobbin, tmp_path: Path):
        mock_run, mock_popen = mock_bobbin
        mock_run.side_effect = subprocess.CalledProcessError(
            returncode=1, cmd=["bobbin", "init"], stderr="bad config",
        )

        with pytest.raises(BobbinSetupError, match="bobbin init failed"):
            setup_bobbin(str(tmp_path))

    def test_index_failure_raises(self, mock_bobbin, tmp_path: Path):
        mock_run, mock_popen = mock_bobbin
        # Make the Popen index step return non-zero.
        popen_inst = mock_popen.return_value
        popen_inst.returncode = 1

        with pytest.raises(BobbinSetupError, match="bobbin index failed"):
            setup_bobbin(str(tmp_path))

    def test_index_timeout_raises(self, mock_bobbin, tmp_path: Path):
        mock_run, mock_popen = mock_bobbin
        popen_inst = mock_popen.return_value
        popen_inst.wait.side_effect = subprocess.TimeoutExpired(
            cmd=["bobbin", "index"], timeout=300,
        )

        with pytest.raises(BobbinSetupError, match="timed out"):
            setup_bobbin(str(tmp_path))

    def test_returns_metadata_with_status(self, mock_bobbin, tmp_path: Path):
        """setup_bobbin returns metadata dict including bobbin status info."""
        import json
        mock_run, mock_popen = mock_bobbin
        status_json = json.dumps({
            "total_files": 42,
            "total_chunks": 100,
            "total_embeddings": 100,
            "languages": ["Rust", "Python"],
        })
        mock_run.side_effect = [
            subprocess.CompletedProcess(args=[], returncode=0, stdout="", stderr=""),
            subprocess.CompletedProcess(args=[], returncode=0, stdout=status_json, stderr=""),
        ]
        result = setup_bobbin(str(tmp_path))
        assert result["total_files"] == 42
        assert result["total_chunks"] == 100
        assert result["languages"] == ["Rust", "Python"]

    def test_custom_timeout(self, mock_bobbin, tmp_path: Path):
        mock_run, mock_popen = mock_bobbin
        setup_bobbin(str(tmp_path), timeout=120)

        # The Popen.wait() call should use the custom timeout.
        popen_inst = mock_popen.return_value
        popen_inst.wait.assert_called_once_with(timeout=120)

    def test_returns_profile_when_available(self, mock_bobbin, tmp_path: Path):
        """setup_bobbin captures profiling data from -v output."""
        mock_run, mock_popen = mock_bobbin
        verbose_output = (
            "  Checking embedding model...\n"
            "  Found 50 files matching patterns\n"
            "\n"
            "Profile:\n"
            "  file I/O:         10ms\n"
            "  parse:            20ms\n"
            "  context:           5ms\n"
            "  embed:           100ms  (200 chunks in 4 batches)\n"
            "    tokenize:       30ms\n"
            "    inference:      65ms\n"
            "    pooling:         5ms\n"
            "  lance delete:     15ms\n"
            "  lance insert:     30ms\n"
            "  git coupling:     50ms\n"
            "  git commits:      25ms\n"
            "  deps:             10ms\n"
            "  compact:          40ms\n"
            "  other/overhead:    5ms\n"
            "  TOTAL:           310ms\n"
            "  embed throughput: 2000.0 chunks/s\n"
        )
        popen_inst = mock_popen.return_value
        popen_inst.stdout.read.return_value = verbose_output

        mock_run.side_effect = [
            subprocess.CompletedProcess(args=[], returncode=0, stdout="", stderr=""),
            subprocess.CompletedProcess(args=[], returncode=0, stdout="{}", stderr=""),
        ]
        result = setup_bobbin(str(tmp_path))
        assert "profile" in result
        assert result["profile"]["file_i/o"] == 10
        assert result["profile"]["embed"] == 100
        assert result["profile"]["inference"] == 65
        assert result["profile"]["total_ms"] == 310
        assert result["profile"]["embed_throughput_chunks_per_sec"] == 2000.0


class TestParseProfile:
    def test_parses_full_output(self):
        output = (
            "Profile:\n"
            "  file I/O:         10ms\n"
            "  parse:            20ms\n"
            "  context:           5ms\n"
            "  embed:           100ms  (200 chunks in 4 batches)\n"
            "    tokenize:       30ms\n"
            "    inference:      65ms\n"
            "    pooling:         5ms\n"
            "  lance delete:     15ms\n"
            "  lance insert:     30ms\n"
            "  git coupling:     50ms\n"
            "  git commits:      25ms\n"
            "  deps:             10ms\n"
            "  compact:          40ms\n"
            "  other/overhead:    5ms\n"
            "  TOTAL:           310ms\n"
            "  embed throughput: 2000.0 chunks/s\n"
        )
        result = _parse_profile(output)
        assert result is not None
        assert result["file_i/o"] == 10
        assert result["parse"] == 20
        assert result["context"] == 5
        assert result["embed"] == 100
        assert result["tokenize"] == 30
        assert result["inference"] == 65
        assert result["pooling"] == 5
        assert result["lance_delete"] == 15
        assert result["lance_insert"] == 30
        assert result["git_coupling"] == 50
        assert result["git_commits"] == 25
        assert result["deps"] == 10
        assert result["compact"] == 40
        assert result["other/overhead"] == 5
        assert result["total_ms"] == 310
        assert result["embed_throughput_chunks_per_sec"] == 2000.0

    def test_returns_none_for_no_profile(self):
        assert _parse_profile("just some output\nno profile here") is None

    def test_returns_none_for_empty_string(self):
        assert _parse_profile("") is None
