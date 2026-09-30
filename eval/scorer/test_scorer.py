"""Run repo tests and parse pass/fail results."""

from __future__ import annotations

import logging
import re
import subprocess
from pathlib import Path

logger = logging.getLogger(__name__)


class TestScorerError(Exception):
    """Raised when the test scorer encounters a fatal error."""


# A pytest summary: "N word[, N word]... in 1.23s", optionally framed by "=".
# Every count names its outcome, so the words are matched explicitly rather than
# assumed to arrive in a fixed order (aegis-bgk9ho: "4 passed, 2331 deselected
# in 0.09s" defeated the fixed-order pattern, and every filtered (-k) run of
# pandas/polars scored as zero tests executed).
_PYTEST_WORD = (
    r"(?:passed|failed|error(?:s|ed)?|skipped|deselected|xfailed|xpassed|warnings?|rerun)"
)
_PYTEST_SUMMARY = re.compile(
    rf"(?m)^[=\s]*((?:\d+ {_PYTEST_WORD}(?:, )?)+) in [\d.]+s\b"
)
_PYTEST_COUNT = re.compile(rf"(\d+) ({_PYTEST_WORD})")


def _parse_pytest_output(output: str) -> dict:
    """Extract pass/fail counts from pytest output.

    Reads the LAST summary line, like:
        "5 passed, 2 failed, 1 error in 3.45s"
        "==== 4 passed, 2331 deselected in 0.09s ===="
        "1 failed, 19 deselected in 0.04s"

    The line must start with a count and name only pytest outcomes, so cargo's
    "Finished `dev` profile ... in 0.23s" (aegis-mzdcm0) never matches.
    Deselected tests and warnings did not run and are not counted.
    """
    summaries = _PYTEST_SUMMARY.findall(output)
    if not summaries:
        return {}
    counts: dict[str, int] = {}
    for number, word in _PYTEST_COUNT.findall(summaries[-1]):
        key = {"error": "errors", "errored": "errors", "warning": "warnings"}.get(word, word)
        counts[key] = counts.get(key, 0) + int(number)

    passed = counts.get("passed", 0) + counts.get("xpassed", 0)
    failed = counts.get("failed", 0) + counts.get("errors", 0)
    skipped = counts.get("skipped", 0) + counts.get("xfailed", 0)
    return {
        "framework": "pytest",
        "passed": passed,
        "failed": failed,
        "skipped": skipped,
        "total": passed + failed + skipped,
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
    # aegis-mzdcm0: cargo first. Its summary line is unambiguous, whereas the
    # pytest pattern is permissive enough to match other frameworks' output.
    for parser in (_parse_cargo_test_output, _parse_pytest_output):
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

    # aegis-mzdcm0: a run in which ZERO tests executed is NOT a pass.  The
    # verdict used to be `exit_code == 0` alone, and `cargo test` with a filter
    # that matches nothing exits 0 -- so "no test ran" was indistinguishable
    # from "every test passed", and an agent that changed nothing scored the
    # same as one that fixed the bug.  A success metric that cannot fail is not
    # a measurement.  Only assert this when the output PARSED: an unrecognised
    # format tells us nothing about how many tests ran, so it keeps the old
    # exit-code behaviour rather than failing every unsupported framework.
    no_tests_executed = bool(parsed) and total == 0

    return {
        "passed": exit_code == 0 and not no_tests_executed,
        "total": total,
        "failures": failures,
        "no_tests_executed": no_tests_executed,
        "output": output,
        "exit_code": exit_code,
        "timed_out": timed_out,
        "parsed": parsed,
    }
