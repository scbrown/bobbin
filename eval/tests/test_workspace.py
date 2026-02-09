"""Tests for eval.runner.workspace module.

Uses temporary git repos instead of hitting the network.
"""

from __future__ import annotations

import subprocess
from pathlib import Path

import pytest

from runner.workspace import (
    WorkspaceError,
    _cache_key,
    checkout_parent,
    clone_repo,
    diff_snapshot,
    setup_workspace,
    snapshot,
    verify_tests,
)


def _git(args: list[str], cwd: Path) -> str:
    """Helper to run git commands in tests."""
    result = subprocess.run(
        ["git"] + args,
        cwd=cwd,
        capture_output=True,
        text=True,
        check=True,
    )
    return result.stdout.strip()


@pytest.fixture()
def local_repo(tmp_path: Path) -> Path:
    """Create a small local git repo with a few commits."""
    repo = tmp_path / "upstream.git"
    repo.mkdir()
    _git(["init", "--bare"], repo)

    work = tmp_path / "work"
    work.mkdir()
    _git(["clone", str(repo), str(work)], tmp_path)
    _git(["config", "user.email", "test@test.com"], work)
    _git(["config", "user.name", "Test"], work)

    # Initial commit
    (work / "README.md").write_text("# Test Repo\n")
    (work / "run_tests.sh").write_text("#!/bin/sh\nexit 0\n")
    _git(["add", "."], work)
    _git(["commit", "-m", "Initial commit"], work)

    # Second commit (the "parent" state)
    (work / "lib.py").write_text("def hello():\n    return 'hello'\n")
    _git(["add", "."], work)
    _git(["commit", "-m", "Add lib"], work)

    # Third commit (the "target" commit)
    (work / "lib.py").write_text("def hello():\n    return 'world'\n")
    _git(["add", "."], work)
    _git(["commit", "-m", "Fix lib output"], work)

    _git(["push", "origin", "master"], work)
    return repo


@pytest.fixture()
def target_commit(local_repo: Path, tmp_path: Path) -> str:
    """Return the hash of the third (target) commit."""
    work = tmp_path / "work"
    return _git(["rev-parse", "HEAD"], work)


@pytest.fixture()
def parent_commit(local_repo: Path, tmp_path: Path) -> str:
    """Return the hash of the second (parent) commit."""
    work = tmp_path / "work"
    return _git(["rev-parse", "HEAD^"], work)


class TestCacheKey:
    def test_deterministic(self):
        assert _cache_key("astral-sh/ruff") == _cache_key("astral-sh/ruff")

    def test_different_repos_differ(self):
        assert _cache_key("astral-sh/ruff") != _cache_key("pallets/flask")


class TestCloneRepo:
    def test_clone_from_cached_mirror(self, local_repo: Path, tmp_path: Path):
        """Clone from a pre-populated cache entry (local bare repo)."""
        dest = tmp_path / "workspaces"
        dest.mkdir()

        # Pre-populate cache with a symlink to our local bare repo
        cache_dir = tmp_path / "cache"
        cache_dir.mkdir()
        cache_key_hex = _cache_key("local/test")
        (cache_dir / f"{cache_key_hex}.git").symlink_to(local_repo)

        ws = clone_repo("local/test", str(dest), cache_dir=cache_dir)
        assert ws.exists()
        assert (ws / "README.md").exists()
        assert ws.name == "local--test"

    def test_creates_subdirectory(self, tmp_path: Path):
        """Verify the dest path uses slug with -- separator."""
        assert "test--repo" == "test/repo".replace("/", "--")


class TestCheckoutParent:
    def test_checks_out_parent(self, local_repo: Path, tmp_path: Path, target_commit: str):
        work = tmp_path / "checkout_test"
        _git(["clone", str(local_repo), str(work)], tmp_path)

        parent_hash = checkout_parent(work, target_commit)
        current = _git(["rev-parse", "HEAD"], work)
        assert current == parent_hash

        # Verify we're at the parent (lib.py should say 'hello' not 'world')
        content = (work / "lib.py").read_text()
        assert "'hello'" in content

    def test_invalid_commit_raises(self, local_repo: Path, tmp_path: Path):
        work = tmp_path / "bad_checkout"
        _git(["clone", str(local_repo), str(work)], tmp_path)

        with pytest.raises(WorkspaceError, match="Cannot resolve parent"):
            checkout_parent(work, "0000000000000000000000000000000000000000")


class TestVerifyTests:
    def test_passing_tests(self, local_repo: Path, tmp_path: Path):
        work = tmp_path / "test_verify"
        _git(["clone", str(local_repo), str(work)], tmp_path)
        assert verify_tests(work, "exit 0") is True

    def test_failing_tests(self, local_repo: Path, tmp_path: Path):
        work = tmp_path / "test_verify_fail"
        _git(["clone", str(local_repo), str(work)], tmp_path)
        assert verify_tests(work, "exit 1") is False

    def test_timeout(self, local_repo: Path, tmp_path: Path):
        work = tmp_path / "test_verify_timeout"
        _git(["clone", str(local_repo), str(work)], tmp_path)
        assert verify_tests(work, "sleep 10", timeout=1) is False


class TestSnapshot:
    def test_snapshot_no_changes(self, local_repo: Path, tmp_path: Path):
        work = tmp_path / "snap_clean"
        _git(["clone", str(local_repo), str(work)], tmp_path)

        head_before = _git(["rev-parse", "HEAD"], work)
        snap = snapshot(work)
        assert snap == head_before

    def test_snapshot_with_changes(self, local_repo: Path, tmp_path: Path):
        work = tmp_path / "snap_dirty"
        _git(["clone", str(local_repo), str(work)], tmp_path)
        _git(["config", "user.email", "test@test.com"], work)
        _git(["config", "user.name", "Test"], work)

        head_before = _git(["rev-parse", "HEAD"], work)
        (work / "new_file.py").write_text("# new\n")
        snap = snapshot(work)
        assert snap != head_before

    def test_snapshot_captures_untracked(self, local_repo: Path, tmp_path: Path):
        work = tmp_path / "snap_untracked"
        _git(["clone", str(local_repo), str(work)], tmp_path)
        _git(["config", "user.email", "test@test.com"], work)
        _git(["config", "user.name", "Test"], work)

        (work / "untracked.txt").write_text("hello\n")
        snap = snapshot(work)
        # The snapshot commit should include the untracked file
        files = _git(["show", "--name-only", "--format=", snap], work)
        assert "untracked.txt" in files


class TestDiffSnapshot:
    def test_diff_shows_changes(self, local_repo: Path, tmp_path: Path):
        work = tmp_path / "diff_test"
        _git(["clone", str(local_repo), str(work)], tmp_path)
        _git(["config", "user.email", "test@test.com"], work)
        _git(["config", "user.name", "Test"], work)

        base = _git(["rev-parse", "HEAD"], work)
        (work / "lib.py").write_text("def hello():\n    return 'changed'\n")
        snap = snapshot(work)

        diff = diff_snapshot(work, base, snap)
        assert "changed" in diff
        assert "lib.py" in diff


class TestSetupWorkspace:
    def test_full_pipeline(self, local_repo: Path, tmp_path: Path, target_commit: str):
        """End-to-end: clone, checkout parent, verify tests."""
        dest = tmp_path / "pipeline_test"
        dest.mkdir()

        # Create a fake cache entry pointing to local bare repo
        cache_dir = tmp_path / "cache"
        cache_dir.mkdir()
        # Manually symlink so _ensure_cached_clone finds it
        cache_key_hex = _cache_key("local/test")
        mirror_link = cache_dir / f"{cache_key_hex}.git"
        mirror_link.symlink_to(local_repo)

        ws, parent = setup_workspace(
            repo="local/test",
            commit=target_commit,
            test_command="exit 0",
            dest=str(dest),
            cache_dir=cache_dir,
            verify=True,
        )

        assert ws.exists()
        content = (ws / "lib.py").read_text()
        assert "'hello'" in content  # At parent commit, not target

    def test_raises_on_test_failure(
        self, local_repo: Path, tmp_path: Path, target_commit: str
    ):
        dest = tmp_path / "pipeline_fail"
        dest.mkdir()

        cache_dir = tmp_path / "cache"
        cache_dir.mkdir()
        cache_key_hex = _cache_key("local/test")
        mirror_link = cache_dir / f"{cache_key_hex}.git"
        mirror_link.symlink_to(local_repo)

        with pytest.raises(WorkspaceError, match="Tests failed"):
            setup_workspace(
                repo="local/test",
                commit=target_commit,
                test_command="exit 1",
                dest=str(dest),
                cache_dir=cache_dir,
                verify=True,
            )

    def test_skip_verification(
        self, local_repo: Path, tmp_path: Path, target_commit: str
    ):
        dest = tmp_path / "pipeline_noverify"
        dest.mkdir()

        cache_dir = tmp_path / "cache"
        cache_dir.mkdir()
        cache_key_hex = _cache_key("local/test")
        mirror_link = cache_dir / f"{cache_key_hex}.git"
        mirror_link.symlink_to(local_repo)

        ws, parent = setup_workspace(
            repo="local/test",
            commit=target_commit,
            test_command="exit 1",  # Would fail, but verify=False
            dest=str(dest),
            cache_dir=cache_dir,
            verify=False,
        )
        assert ws.exists()
