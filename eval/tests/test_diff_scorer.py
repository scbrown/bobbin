"""Tests for eval.scorer.diff_scorer module.

Uses real local git repos for integration-style testing of diff operations.
"""

from __future__ import annotations

import subprocess
from pathlib import Path

import pytest

from scorer.diff_scorer import (
    DiffScorerError,
    _files_changed_between,
    _files_changed_in_commit,
    score_diff,
)


@pytest.fixture()
def git_repo(tmp_path: Path) -> Path:
    """Create a local git repo with a known commit history.

    History:
        commit 1 (initial): file_a.py, file_b.py
        commit 2 (ground_truth): modifies file_a.py, adds file_c.py
    """
    repo = tmp_path / "repo"
    repo.mkdir()

    def _run(*args: str):
        subprocess.run(args, cwd=repo, check=True, capture_output=True, text=True)

    _run("git", "init")
    _run("git", "config", "user.email", "test@test.com")
    _run("git", "config", "user.name", "Test")

    # Initial commit.
    (repo / "file_a.py").write_text("original a\n")
    (repo / "file_b.py").write_text("original b\n")
    _run("git", "add", "-A")
    _run("git", "commit", "-m", "initial commit")

    # Ground truth commit: modifies file_a.py, adds file_c.py.
    (repo / "file_a.py").write_text("modified a\n")
    (repo / "file_c.py").write_text("new file c\n")
    _run("git", "add", "-A")
    _run("git", "commit", "-m", "ground truth fix")

    return repo


def _get_head(repo: Path) -> str:
    result = subprocess.run(
        ["git", "rev-parse", "HEAD"],
        cwd=repo, capture_output=True, text=True, check=True,
    )
    return result.stdout.strip()


def _get_parent(repo: Path, commit: str) -> str:
    result = subprocess.run(
        ["git", "rev-parse", f"{commit}^"],
        cwd=repo, capture_output=True, text=True, check=True,
    )
    return result.stdout.strip()


class TestFilesChangedInCommit:
    def test_detects_modified_and_added(self, git_repo: Path):
        head = _get_head(git_repo)
        files = _files_changed_in_commit(git_repo, head)
        assert files == {"file_a.py", "file_c.py"}

    def test_initial_commit_files(self, git_repo: Path):
        head = _get_head(git_repo)
        parent = _get_parent(git_repo, head)
        files = _files_changed_in_commit(git_repo, parent)
        assert files == {"file_a.py", "file_b.py"}


class TestFilesChangedBetween:
    def test_diff_between_commits(self, git_repo: Path):
        head = _get_head(git_repo)
        parent = _get_parent(git_repo, head)
        files = _files_changed_between(git_repo, parent, head)
        assert files == {"file_a.py", "file_c.py"}

    def test_no_changes(self, git_repo: Path):
        head = _get_head(git_repo)
        files = _files_changed_between(git_repo, head, head)
        assert files == set()


class TestScoreDiff:
    def test_perfect_match(self, git_repo: Path):
        """Agent touched exactly the same files as ground truth."""
        gt_commit = _get_head(git_repo)
        parent = _get_parent(git_repo, gt_commit)

        # Checkout parent, make the same changes, snapshot.
        subprocess.run(
            ["git", "checkout", "--force", parent],
            cwd=git_repo, check=True, capture_output=True,
        )
        (git_repo / "file_a.py").write_text("agent modified a\n")
        (git_repo / "file_c.py").write_text("agent new file c\n")
        subprocess.run(["git", "add", "-A"], cwd=git_repo, check=True, capture_output=True)
        subprocess.run(
            ["git", "commit", "-m", "agent snapshot"],
            cwd=git_repo, check=True, capture_output=True,
        )
        agent_snap = _get_head(git_repo)

        result = score_diff(str(git_repo), gt_commit, snapshot=agent_snap)

        assert result["file_precision"] == 1.0
        assert result["file_recall"] == 1.0
        assert result["f1"] == 1.0
        assert result["exact_file_match"] is True
        assert sorted(result["files_touched"]) == ["file_a.py", "file_c.py"]
        assert sorted(result["ground_truth_files"]) == ["file_a.py", "file_c.py"]

    def test_partial_match(self, git_repo: Path):
        """Agent touched one correct file and one extra file."""
        gt_commit = _get_head(git_repo)
        parent = _get_parent(git_repo, gt_commit)

        subprocess.run(
            ["git", "checkout", "--force", parent],
            cwd=git_repo, check=True, capture_output=True,
        )
        # Touch file_a.py (correct) and file_b.py (extra), miss file_c.py.
        (git_repo / "file_a.py").write_text("agent modified a\n")
        (git_repo / "file_b.py").write_text("agent modified b\n")
        subprocess.run(["git", "add", "-A"], cwd=git_repo, check=True, capture_output=True)
        subprocess.run(
            ["git", "commit", "-m", "agent snapshot"],
            cwd=git_repo, check=True, capture_output=True,
        )
        agent_snap = _get_head(git_repo)

        result = score_diff(str(git_repo), gt_commit, snapshot=agent_snap)

        # Precision: 1/2 (file_a is correct, file_b is extra)
        assert result["file_precision"] == 0.5
        # Recall: 1/2 (file_a found, file_c missed)
        assert result["file_recall"] == 0.5
        assert result["exact_file_match"] is False

    def test_no_overlap(self, git_repo: Path):
        """Agent touched completely wrong files."""
        gt_commit = _get_head(git_repo)
        parent = _get_parent(git_repo, gt_commit)

        subprocess.run(
            ["git", "checkout", "--force", parent],
            cwd=git_repo, check=True, capture_output=True,
        )
        # Only touch file_b.py (not in ground truth).
        (git_repo / "file_b.py").write_text("agent modified b\n")
        subprocess.run(["git", "add", "-A"], cwd=git_repo, check=True, capture_output=True)
        subprocess.run(
            ["git", "commit", "-m", "agent snapshot"],
            cwd=git_repo, check=True, capture_output=True,
        )
        agent_snap = _get_head(git_repo)

        result = score_diff(str(git_repo), gt_commit, snapshot=agent_snap)

        assert result["file_precision"] == 0.0
        assert result["file_recall"] == 0.0
        assert result["f1"] == 0.0

    def test_no_changes_by_agent(self, git_repo: Path):
        """Agent made no changes at all."""
        gt_commit = _get_head(git_repo)
        parent = _get_parent(git_repo, gt_commit)

        # Snapshot is just the parent (no changes).
        result = score_diff(str(git_repo), gt_commit, snapshot=parent)

        assert result["file_precision"] == 0.0
        assert result["file_recall"] == 0.0
        assert result["files_touched"] == []

    def test_working_tree_mode(self, git_repo: Path):
        """When no snapshot given, compare working tree against parent."""
        gt_commit = _get_head(git_repo)
        parent = _get_parent(git_repo, gt_commit)

        subprocess.run(
            ["git", "checkout", "--force", parent],
            cwd=git_repo, check=True, capture_output=True,
        )
        # Modify file_a.py in the working tree (don't commit).
        (git_repo / "file_a.py").write_text("working tree change\n")

        result = score_diff(str(git_repo), gt_commit)

        assert "file_a.py" in result["files_touched"]
        assert result["file_recall"] == 0.5  # file_a found, file_c missed

    def test_invalid_commit_raises(self, git_repo: Path):
        with pytest.raises(DiffScorerError, match="Cannot read ground truth"):
            score_diff(str(git_repo), "deadbeef" * 5)

    def test_f1_computation(self, git_repo: Path):
        """Verify F1 is the harmonic mean of precision and recall."""
        gt_commit = _get_head(git_repo)
        parent = _get_parent(git_repo, gt_commit)

        subprocess.run(
            ["git", "checkout", "--force", parent],
            cwd=git_repo, check=True, capture_output=True,
        )
        # Touch file_a.py only (correct). Miss file_c.py.
        (git_repo / "file_a.py").write_text("agent modified a\n")
        subprocess.run(["git", "add", "-A"], cwd=git_repo, check=True, capture_output=True)
        subprocess.run(
            ["git", "commit", "-m", "agent snapshot"],
            cwd=git_repo, check=True, capture_output=True,
        )
        agent_snap = _get_head(git_repo)

        result = score_diff(str(git_repo), gt_commit, snapshot=agent_snap)

        # Precision: 1/1, Recall: 1/2. F1 = 2*(1*0.5)/(1+0.5) = 0.6667
        assert result["file_precision"] == 1.0
        assert result["file_recall"] == 0.5
        assert result["f1"] == pytest.approx(0.6667, abs=0.001)
