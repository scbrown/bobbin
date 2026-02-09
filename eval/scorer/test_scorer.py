"""Run repo tests and parse pass/fail results."""

from __future__ import annotations

import logging
import re
import subprocess
from pathlib import Path

logger = logging.getLogger(__name__)


class TestScorerError(Exception):
    """Raised when the test scorer encounters a fatal error."""


def _parse_pytest_output(output: str) -> dict:
    """Extract pass/fail counts from pytest output.

    Looks for the summary line like:
        "5 passed, 2 failed, 1 error in 3.45s"
        "10 passed in 1.23s"
    """
    # Match the final summary line.
    pattern = re.compile(
        r"(?:(\d+) passed)?"
        r"(?:,?\s*(\d+) failed)?"
        r"(?:,?\s*(\d+) error(?:s|ed)?)?"
        r"(?:,?\s*(\d+) skipped)?"
        r"\s+in\s+[\d.]+s"
    )
    match = pattern.search(output)
    if not match:
        return {}

    passed = int(match.group(1) or 0)
    failed = int(match.group(2) or 0)
    errors = int(match.group(3) or 0)
    skipped = int(match.group(4) or 0)

    return {
        "framework": "pytest",
        "passed": passed,
        "failed": failed + errors,
        "skipped": skipped,
        "total": passed + failed + errors + skipped,
    }


def _parse_cargo_test_output(output: str) -> dict:
    """Extract pass/fail counts from ``cargo test`` output.

    Looks for the summary line like:
        "test result: ok. 42 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out"
        "test result: FAILED. 1 passed; 2 failed; 0 ignored; ..."
    """
    pattern = re.compile(
        r"test result: \S+\.\s+"
        r"(\d+) passed;\s+"
        r"(\d+) failed;\s+"
        r"(\d+) ignored;"
    )
    # Cargo may emit multiple result lines (one per test binary).
    # Accumulate totals across all.
    passed = failed = ignored = 0
    found = False
    for m in pattern.finditer(output):
        found = True
        passed += int(m.group(1))
        failed += int(m.group(2))
        ignored += int(m.group(3))

    if not found:
        return {}

    return {
        "framework": "cargo-test",
        "passed": passed,
        "failed": failed,
        "skipped": ignored,
        "total": passed + failed + ignored,
    }


def _parse_output(output: str) -> dict:
    """Try each parser and return the first match."""
    for parser in (_parse_pytest_output, _parse_cargo_test_output):
        result = parser(output)
        if result:
            return result
    return {}


def run_tests(workspace: str, test_command: str, *, timeout: int = 600) -> dict:
    """Run the test command in the workspace and parse results.

    Parameters
    ----------
    workspace:
        Path to the git working copy.
    test_command:
        Shell command to run (e.g. ``"pytest tests/ -x"``).
    timeout:
        Maximum seconds before killing the test process.

    Returns a dict with keys:
        passed      — bool, whether the test suite passed (exit code 0)
        total       — total number of tests detected (0 if unparseable)
        failures    — number of failing tests detected (0 if unparseable)
        output      — combined stdout+stderr from the test run
        exit_code   — process exit code (-1 on timeout)
        timed_out   — whether the process was killed
        parsed      — dict of parsed framework-specific counts (empty if unparseable)
    """
    ws = Path(workspace)
    logger.info("Running tests in %s: %s", ws, test_command)

    timed_out = False
    try:
        proc = subprocess.run(
            ["sh", "-c", test_command],
            cwd=ws,
            capture_output=True,
            text=True,
            timeout=timeout,
        )
        exit_code = proc.returncode
        output = proc.stdout + proc.stderr
    except subprocess.TimeoutExpired as exc:
        timed_out = True
        exit_code = -1
        stdout = exc.stdout or ""
        stderr = exc.stderr or ""
        if isinstance(stdout, bytes):
            stdout = stdout.decode("utf-8", errors="replace")
        if isinstance(stderr, bytes):
            stderr = stderr.decode("utf-8", errors="replace")
        output = stdout + stderr
        logger.warning("Test command timed out after %ds in %s", timeout, ws)

    parsed = _parse_output(output)

    failures = parsed.get("failed", 0) if parsed else (0 if exit_code == 0 else -1)
    total = parsed.get("total", 0)

    return {
        "passed": exit_code == 0,
        "total": total,
        "failures": failures,
        "output": output,
        "exit_code": exit_code,
        "timed_out": timed_out,
        "parsed": parsed,
    }
