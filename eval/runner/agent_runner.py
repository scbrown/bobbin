"""Claude Code headless invocation for eval runs."""

from __future__ import annotations

import json
import logging
import shutil
import subprocess
import time
from pathlib import Path

logger = logging.getLogger(__name__)


class AgentRunnerError(Exception):
    """Raised when the agent invocation fails."""


def _find_claude() -> str:
    """Find the claude CLI binary."""
    found = shutil.which("claude")
    if found:
        return found
    for candidate in [
        Path.home() / ".local" / "bin" / "claude",
        Path("/usr/local/bin/claude"),
    ]:
        if candidate.exists():
            return str(candidate)
    raise AgentRunnerError("claude CLI not found. Install Claude Code first.")


def run_agent(
    workspace: str,
    prompt: str,
    settings_file: str | None = None,
    model: str = "claude-sonnet-4-5-20250929",
    max_budget_usd: float = 2.00,
    *,
    timeout: int = 600,
    permission_mode: str = "bypassPermissions",
) -> dict:
    """Run Claude Code headless on a workspace with the given prompt.

    Parameters
    ----------
    workspace:
        Path to the git working copy to run Claude in.
    prompt:
        The task prompt to send to Claude.
    settings_file:
        Optional path to a Claude Code settings JSON file.  Used to toggle
        bobbin integration: with-bobbin runs pass a settings file that
        configures the bobbin MCP server; no-bobbin runs pass None or omit.
    model:
        Claude model to use.
    max_budget_usd:
        Maximum dollar spend for this run.
    timeout:
        Maximum wall-clock seconds before killing the process.
    permission_mode:
        Permission mode for Claude Code.  Defaults to ``"bypassPermissions"``
        which is required for unattended headless eval runs.

    Returns a dict with:
        result        — parsed JSON output from Claude (or None on failure)
        output_raw    — raw stdout text
        stderr        — raw stderr text
        exit_code     — process return code (−1 on timeout)
        duration_seconds — wall-clock time
        timed_out     — whether the process was killed for timeout
    """
    claude = _find_claude()
    ws = Path(workspace)

    cmd = [
        claude,
        "-p", prompt,
        "--output-format", "json",
        "--model", model,
        "--max-budget-usd", str(max_budget_usd),
        "--permission-mode", permission_mode,
    ]

    if settings_file:
        cmd.extend(["--settings", str(settings_file)])

    logger.info(
        "Running agent in %s (model=%s, budget=$%.2f, timeout=%ds)",
        ws, model, max_budget_usd, timeout,
    )
    logger.debug("Command: %s", " ".join(cmd))

    start = time.monotonic()
    timed_out = False

    try:
        proc = subprocess.run(
            cmd,
            cwd=ws,
            capture_output=True,
            text=True,
            timeout=timeout,
        )
        exit_code = proc.returncode
        stdout = proc.stdout
        stderr = proc.stderr
    except subprocess.TimeoutExpired as exc:
        timed_out = True
        exit_code = -1
        stdout = exc.stdout or ""
        stderr = exc.stderr or ""
        if isinstance(stdout, bytes):
            stdout = stdout.decode("utf-8", errors="replace")
        if isinstance(stderr, bytes):
            stderr = stderr.decode("utf-8", errors="replace")
        logger.warning("Agent timed out after %ds in %s", timeout, ws)

    duration = time.monotonic() - start

    if stderr:
        logger.debug("Agent stderr: %s", stderr[:500])

    # Parse JSON output from Claude Code.
    result = None
    if stdout.strip():
        try:
            result = json.loads(stdout)
        except json.JSONDecodeError:
            logger.warning("Failed to parse agent JSON output, storing raw text")
            result = {"raw_text": stdout}

    return {
        "result": result,
        "output_raw": stdout,
        "stderr": stderr,
        "exit_code": exit_code,
        "duration_seconds": round(duration, 2),
        "timed_out": timed_out,
    }
