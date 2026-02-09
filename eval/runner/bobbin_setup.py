"""Bobbin init + index on a workspace for with-bobbin eval runs."""

from __future__ import annotations

import json
import logging
import shutil
import subprocess
import time
from pathlib import Path
from typing import Any

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


def setup_bobbin(workspace: str, *, timeout: int = 300) -> dict[str, Any]:
    """Run bobbin init and index on the given workspace.

    Parameters
    ----------
    workspace:
        Path to the git working copy where bobbin should be initialized.
    timeout:
        Max seconds for the index step (init is fast, index can be slow).

    Returns a metadata dict with index timing and bobbin status info.

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
    t0 = time.monotonic()
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
    index_duration = time.monotonic() - t0

    # Capture bobbin status for metadata.
    metadata: dict[str, Any] = {"index_duration_seconds": round(index_duration, 2)}
    try:
        status_result = subprocess.run(
            [bobbin, "status", "--json"],
            cwd=ws,
            capture_output=True,
            text=True,
            timeout=30,
        )
        if status_result.returncode == 0:
            status_data = json.loads(status_result.stdout)
            metadata["total_files"] = status_data.get("total_files")
            metadata["total_chunks"] = status_data.get("total_chunks")
            metadata["total_embeddings"] = status_data.get("total_embeddings")
            metadata["languages"] = status_data.get("languages", [])
    except (subprocess.CalledProcessError, subprocess.TimeoutExpired, json.JSONDecodeError) as exc:
        logger.warning("Could not capture bobbin status: %s", exc)

    logger.info("Bobbin setup complete for %s (indexed in %.1fs)", ws, index_duration)
    return metadata
