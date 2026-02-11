"""Bobbin init + index on a workspace for with-bobbin eval runs."""

from __future__ import annotations

import json
import logging
import re
import shutil
import subprocess
import time
from pathlib import Path
from typing import Any

logger = logging.getLogger(__name__)

_WORKSPACE_CLAUDE_MD = """\
# Project Tools

This project is indexed by **bobbin**, a code context engine.
Use these commands to explore the codebase:

- `bobbin search <query>` — find code by meaning (semantic search)
- `bobbin context <query>` — get a focused context bundle for a task
- `bobbin related <file>` — find files that frequently change together
- `bobbin refs <symbol>` — find definitions and usages of a symbol
- `bobbin grep <pattern>` — regex/keyword search across all files

Prefer bobbin tools over manual grep/find for navigating unfamiliar code.
"""


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


def _parse_profile(output: str) -> dict[str, Any] | None:
    """Extract profiling data from ``bobbin index -v`` output.

    Parses the ``Profile:`` block emitted when verbose mode is enabled.
    Returns a dict of phase timings (in ms) or None if no profile found.
    """
    profile: dict[str, Any] = {}
    in_profile = False
    for line in output.splitlines():
        stripped = line.strip()
        if stripped.startswith("Profile:"):
            in_profile = True
            continue
        if not in_profile:
            continue
        # TOTAL line: "  TOTAL:           310ms"
        m = re.match(r"^TOTAL:\s+(\d+)ms", stripped)
        if m:
            profile["total_ms"] = int(m.group(1))
            continue
        # embed throughput line: "  embed throughput: 123.4 chunks/s"
        m = re.match(r"^embed throughput:\s+([\d.]+)\s+chunks/s", stripped)
        if m:
            profile["embed_throughput_chunks_per_sec"] = float(m.group(1))
            continue
        # Each line looks like: "  file I/O:       123ms"
        # or "  embed:          456ms  (100 chunks in 2 batches)"
        # or sub-phase: "    tokenize:     123ms"
        m = re.match(r"^(\S[^:]+):\s+(\d+)ms", stripped)
        if m:
            key = m.group(1).strip().replace(" ", "_").lower()
            profile[key] = int(m.group(2))
            continue
        # Non-matching line after Profile block → end of profile
        if in_profile and stripped:
            break
    return profile if profile else None


def setup_bobbin(workspace: str, *, timeout: int = 1800) -> dict[str, Any]:
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

    # Write workspace CLAUDE.md for agent guidance.
    claude_dir = ws / ".claude"
    claude_dir.mkdir(exist_ok=True)
    claude_md = claude_dir / "CLAUDE.md"
    claude_md.write_text(_WORKSPACE_CLAUDE_MD, encoding="utf-8")
    logger.info("Wrote CLAUDE.md to %s", claude_md)

    logger.info("Indexing workspace %s", ws)
    t0 = time.monotonic()
    try:
        proc = subprocess.Popen(
            [bobbin, "index", "--verbose"],
            cwd=ws,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
        )
        # Stream stderr for real-time progress while accumulating stdout
        stderr_lines: list[str] = []
        assert proc.stderr is not None
        for line in proc.stderr:
            stripped = line.rstrip("\n")
            stderr_lines.append(stripped)
            if stripped.startswith("progress:"):
                logger.info("bobbin index %s", stripped)
        stdout_text = proc.stdout.read() if proc.stdout else ""
        proc.wait(timeout=timeout)
        if proc.returncode != 0:
            raise subprocess.CalledProcessError(
                proc.returncode, proc.args,
                output=stdout_text, stderr="\n".join(stderr_lines),
            )
    except subprocess.CalledProcessError as exc:
        raise BobbinSetupError(f"bobbin index failed: {exc.stderr.strip()}") from exc
    except subprocess.TimeoutExpired:
        proc.kill()
        raise BobbinSetupError(f"bobbin index timed out after {timeout}s")
    index_duration = time.monotonic() - t0

    # Capture bobbin status for metadata.
    metadata: dict[str, Any] = {"index_duration_seconds": round(index_duration, 2)}

    # Parse profiling data from verbose output.
    profile = _parse_profile(stdout_text)
    if profile:
        metadata["profile"] = profile
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
