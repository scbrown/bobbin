"""Claude Code headless invocation for eval runs."""

from __future__ import annotations

import contextlib
import json
import logging
import os
import re
import shutil
import subprocess
import time
from pathlib import Path

logger = logging.getLogger(__name__)


@contextlib.contextmanager
def _isolate_global_settings():
    """Temporarily move ~/.claude/settings.json aside during eval runs.

    Claude Code's ``--settings`` flag is additive — it merges with the
    global settings rather than replacing them.  This means global hooks
    (like a user's personal bobbin hook) would fire alongside the eval
    settings, contaminating results.

    This context manager renames the global settings to a .bak file
    before the agent runs and restores it afterward.
    """
    global_settings = Path.home() / ".claude" / "settings.json"
    backup = global_settings.with_suffix(".json.eval-bak")

    if global_settings.exists():
        logger.info("Isolating global settings: %s → %s", global_settings, backup)
        global_settings.rename(backup)
    else:
        backup = None  # type: ignore[assignment]

    try:
        yield
    finally:
        if backup and backup.exists():
            logger.info("Restoring global settings: %s → %s", backup, global_settings)
            backup.rename(global_settings)


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


def parse_stream_json(raw: str) -> dict:
    """Parse JSONL output from ``claude --output-format stream-json --verbose``.

    Extracts tool-use information and the final result summary from the stream.

    Returns a dict with:
        result_line   — parsed ``type:result`` line (same schema as ``--output-format json``)
        tool_use_summary — dict with by_tool, tool_sequence, first_edit_turn, bobbin_commands
    """
    by_tool: dict[str, int] = {}
    tool_sequence: list[str] = []
    first_edit_turn: int | None = None
    bobbin_commands: list[str] = []
    result_line: dict | None = None
    turn = -1  # incremented on each assistant message

    for line in raw.splitlines():
        line = line.strip()
        if not line:
            continue
        try:
            obj = json.loads(line)
        except json.JSONDecodeError:
            continue

        msg_type = obj.get("type")

        if msg_type == "assistant":
            turn += 1
            content = (obj.get("message") or {}).get("content") or []
            for block in content:
                if block.get("type") != "tool_use":
                    continue
                name = block.get("name", "unknown")
                by_tool[name] = by_tool.get(name, 0) + 1
                tool_sequence.append(name)

                if first_edit_turn is None and name in ("Edit", "Write"):
                    first_edit_turn = turn

                if name == "Bash":
                    cmd = (block.get("input") or {}).get("command", "")
                    if re.search(r"\bbobbin\b", cmd):
                        bobbin_commands.append(cmd)

        elif msg_type == "result":
            result_line = obj

    return {
        "result_line": result_line,
        "tool_use_summary": {
            "by_tool": by_tool,
            "tool_sequence": tool_sequence,
            "first_edit_turn": first_edit_turn,
            "bobbin_commands": bobbin_commands,
        },
    }


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
        result        — parsed result from Claude (type:result line, or JSON fallback)
        output_raw    — raw stdout text (JSONL stream)
        stderr        — raw stderr text
        exit_code     — process return code (−1 on timeout)
        duration_seconds — wall-clock time
        timed_out     — whether the process was killed for timeout
        tool_use_summary — dict with by_tool, tool_sequence, first_edit_turn, bobbin_commands
    """
    claude = _find_claude()
    ws = Path(workspace)

    cmd = [
        claude,
        "-p", prompt,
        "--output-format", "stream-json",
        "--verbose",
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

    # Build a clean environment for the agent subprocess.  Remove CLAUDECODE
    # so the CLI doesn't refuse to start when eval is launched from inside an
    # existing Claude Code session.
    agent_env = {k: v for k, v in os.environ.items() if k != "CLAUDECODE"}

    with _isolate_global_settings():
        try:
            proc = subprocess.run(
                cmd,
                cwd=ws,
                capture_output=True,
                text=True,
                timeout=timeout,
                env=agent_env,
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

    # Parse stream-json JSONL output.
    parsed = parse_stream_json(stdout)
    result_line = parsed["result_line"]
    tool_use_summary = parsed["tool_use_summary"]

    # The type:result line contains the same data as --output-format json.
    result = result_line
    if result is None and stdout.strip():
        # Fallback: try parsing entire stdout as single JSON (old format compat).
        try:
            result = json.loads(stdout)
        except json.JSONDecodeError:
            logger.warning("No type:result line and failed JSON fallback, storing raw text")
            result = {"raw_text": stdout}

    return {
        "result": result,
        "output_raw": stdout,
        "stderr": stderr,
        "exit_code": exit_code,
        "duration_seconds": round(duration, 2),
        "timed_out": timed_out,
        "tool_use_summary": tool_use_summary,
    }
