"""Run repo tests and parse pass/fail results."""


def run_tests(workspace: str, test_command: str) -> dict:
    """Run the test command in the workspace and parse results.

    Returns a dict with keys: passed (bool), total, failures, output.
    """
    raise NotImplementedError("Test scorer not yet implemented")
