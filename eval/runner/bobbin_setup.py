"""Bobbin init + index on a workspace for with-bobbin eval runs."""

from __future__ import annotations

import logging
import shutil
import subprocess
from pathlib import Path

logger = logging.getLogger(__name__)


class BobbinSetupError(Exception):
    """Raised when bobbin init or index fails."""


def _find_bobbin() -> str:
    """Find the bobbin binary, preferring PATH then common install locations."""
    found = shutil.which("bobbin")
    if found:
        return found
    cargo_bin = Path.home() / ".cargo" / "bin" / "bobbin"
    if cargo_bin.exists():
        return str(cargo_bin)
    raise BobbinSetupError("bobbin binary not found. Install with: cargo install bobbin")


def setup_bobbin(workspace: str, *, timeout: int = 300) -> None:
    """Run bobbin init and index on the given workspace.

    Parameters
    ----------
    workspace:
        Path to the git working copy where bobbin should be initialized.
    timeout:
        Max seconds for the index step (init is fast, index can be slow).

    Raises :class:`BobbinSetupError` if init or index fails.
    """
    ws = Path(workspace)
    bobbin = _find_bobbin()

    logger.info("Initializing bobbin in %s", ws)
    try:
        subprocess.run(
            [bobbin, "init"],
            cwd=ws,
            check=True,
            capture_output=True,
            text=True,
            timeout=30,
        )
    except subprocess.CalledProcessError as exc:
        raise BobbinSetupError(f"bobbin init failed: {exc.stderr.strip()}") from exc

    logger.info("Indexing workspace %s", ws)
    try:
        subprocess.run(
            [bobbin, "index"],
            cwd=ws,
            check=True,
            capture_output=True,
            text=True,
            timeout=timeout,
        )
    except subprocess.CalledProcessError as exc:
        raise BobbinSetupError(f"bobbin index failed: {exc.stderr.strip()}") from exc
    except subprocess.TimeoutExpired as exc:
        raise BobbinSetupError(f"bobbin index timed out after {timeout}s") from exc

    logger.info("Bobbin setup complete for %s", ws)
