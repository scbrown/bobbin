"""Workspace manager: clone, checkout parent commit, verify tests, snapshot.

Handles repo cloning with a local cache to avoid re-downloading large repos,
checks out the parent of a target commit, verifies the test suite passes at
that state, and takes git-based snapshots for later diffing.
"""

from __future__ import annotations

import hashlib
import logging
import shutil
import subprocess
from pathlib import Path

logger = logging.getLogger(__name__)

DEFAULT_CACHE_DIR = Path.home() / ".cache" / "bobbin-eval" / "repos"


class WorkspaceError(Exception):
    """Raised when a workspace operation fails."""


def _run(
    cmd: list[str],
    *,
    cwd: Path | None = None,
    check: bool = True,
    capture: bool = True,
    timeout: int = 600,
) -> subprocess.CompletedProcess[str]:
    """Run a subprocess with sensible defaults."""
    logger.debug("Running: %s (cwd=%s)", " ".join(cmd), cwd)
    return subprocess.run(
        cmd,
        cwd=cwd,
        check=check,
        capture_output=capture,
        text=True,
        timeout=timeout,
    )


def _cache_key(repo: str) -> str:
    """Deterministic cache directory name for a repo slug (e.g. 'astral-sh/ruff')."""
    return hashlib.sha256(repo.encode()).hexdigest()[:16]


def _ensure_cached_clone(repo: str, cache_dir: Path) -> Path:
    """Ensure a bare mirror of *repo* exists in the cache. Returns its path.

    Uses ``git clone --mirror`` so subsequent workspaces can be created with a
    cheap local clone instead of hitting the network every time.
    """
    cache_dir.mkdir(parents=True, exist_ok=True)
    mirror_path = cache_dir / f"{_cache_key(repo)}.git"

    if mirror_path.exists():
        logger.info("Cache hit for %s — fetching updates", repo)
        try:
            _run(["git", "remote", "update", "--prune"], cwd=mirror_path, timeout=300)
        except (subprocess.CalledProcessError, subprocess.TimeoutExpired) as exc:
            logger.warning("Cache fetch failed, will re-clone: %s", exc)
            shutil.rmtree(mirror_path)
        else:
            return mirror_path

    url = f"https://github.com/{repo}.git"
    logger.info("Cloning %s into cache (%s)", url, mirror_path)
    _run(["git", "clone", "--mirror", url, str(mirror_path)], timeout=600)
    return mirror_path


def clone_repo(
    repo: str,
    dest: str,
    *,
    cache_dir: Path | None = None,
) -> Path:
    """Clone *repo* into *dest* using the local cache.

    Parameters
    ----------
    repo:
        GitHub slug, e.g. ``"astral-sh/ruff"``.
    dest:
        Directory where the working copy will be created.  A subdirectory
        named after the repo slug (with ``/`` replaced by ``--``) is created
        inside *dest*.
    cache_dir:
        Override the default cache location.

    Returns the :class:`Path` of the new working copy.
    """
    cache = cache_dir or DEFAULT_CACHE_DIR
    mirror = _ensure_cached_clone(repo, cache)

    dest_path = Path(dest) / repo.replace("/", "--")
    if dest_path.exists():
        shutil.rmtree(dest_path)

    logger.info("Creating workspace at %s from cache", dest_path)
    _run(["git", "clone", str(mirror), str(dest_path)], timeout=120)

    # Point the remote at the real upstream so future fetches work normally.
    _run(
        ["git", "remote", "set-url", "origin", f"https://github.com/{repo}.git"],
        cwd=dest_path,
    )
    return dest_path


def checkout_parent(workspace: str | Path, commit: str) -> str:
    """Checkout the parent of *commit* in the workspace.

    Parameters
    ----------
    workspace:
        Path to the git working copy.
    commit:
        The target commit hash. We check out ``commit^`` (its first parent).

    Returns the resolved parent commit hash.

    Raises :class:`WorkspaceError` if the commit has no parent or is invalid.
    """
    ws = Path(workspace)

    # Resolve the parent hash.
    try:
        result = _run(["git", "rev-parse", f"{commit}^"], cwd=ws)
    except subprocess.CalledProcessError as exc:
        raise WorkspaceError(
            f"Cannot resolve parent of {commit}: {exc.stderr.strip()}"
        ) from exc
    parent_hash = result.stdout.strip()

    logger.info("Checking out parent %s of commit %s", parent_hash[:12], commit[:12])
    _run(["git", "checkout", "--force", parent_hash], cwd=ws)
    return parent_hash


def verify_tests(workspace: str | Path, test_command: str, *, timeout: int = 600) -> bool:
    """Run the task's test command and return True if it exits 0.

    This is used to confirm the parent commit is in a clean state before
    handing the workspace to an agent.
    """
    ws = Path(workspace)
    logger.info("Verifying tests at workspace %s: %s", ws, test_command)
    try:
        _run(
            ["sh", "-c", test_command],
            cwd=ws,
            timeout=timeout,
        )
    except subprocess.CalledProcessError:
        logger.warning("Test verification failed in %s", ws)
        return False
    except subprocess.TimeoutExpired:
        logger.warning("Test verification timed out (%ds) in %s", timeout, ws)
        return False
    return True


def snapshot(workspace: str | Path) -> str:
    """Create a snapshot of the current workspace state.

    Records a temporary commit on a detached HEAD so the exact tree can be
    recovered later for diffing.  Returns the snapshot commit hash.
    """
    ws = Path(workspace)

    # Stage everything including untracked files.
    _run(["git", "add", "-A"], cwd=ws)

    # Check if there's anything to commit.
    status = _run(["git", "status", "--porcelain"], cwd=ws)
    if not status.stdout.strip():
        # Nothing changed — return current HEAD as the snapshot.
        head = _run(["git", "rev-parse", "HEAD"], cwd=ws)
        return head.stdout.strip()

    _run(
        ["git", "commit", "-m", "bobbin-eval snapshot", "--allow-empty"],
        cwd=ws,
        # Set author info so snapshots are deterministic and don't depend on
        # the user's git config.
        capture=True,
    )
    result = _run(["git", "rev-parse", "HEAD"], cwd=ws)
    snap_hash = result.stdout.strip()
    logger.info("Snapshot created: %s", snap_hash[:12])
    return snap_hash


def diff_snapshot(workspace: str | Path, base: str, snapshot_hash: str) -> str:
    """Return the unified diff between *base* and *snapshot_hash*.

    Useful for comparing the agent's changes against the ground-truth commit.
    """
    ws = Path(workspace)
    result = _run(["git", "diff", base, snapshot_hash], cwd=ws)
    return result.stdout


def setup_workspace(
    repo: str,
    commit: str,
    test_command: str,
    dest: str,
    *,
    cache_dir: Path | None = None,
    verify: bool = True,
    test_timeout: int = 600,
) -> tuple[Path, str]:
    """Full workspace setup pipeline for a single eval run.

    1. Clone repo (using cache)
    2. Checkout parent of target commit
    3. Optionally verify tests pass at parent

    Parameters
    ----------
    repo:
        GitHub slug.
    commit:
        Target commit hash.
    test_command:
        Shell command to verify tests.
    dest:
        Base directory for workspaces.
    cache_dir:
        Override cache location.
    verify:
        If True (default), run test_command and raise on failure.
    test_timeout:
        Timeout in seconds for test verification.

    Returns ``(workspace_path, parent_hash)``.

    Raises :class:`WorkspaceError` if verification is enabled and tests fail.
    """
    ws = clone_repo(repo, dest, cache_dir=cache_dir)
    parent = checkout_parent(ws, commit)

    if verify:
        if not verify_tests(ws, test_command, timeout=test_timeout):
            raise WorkspaceError(
                f"Tests failed at parent commit {parent[:12]} in {ws}. "
                "This task's parent state may be broken."
            )

    return ws, parent
