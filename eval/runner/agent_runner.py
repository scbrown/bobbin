"""Claude Code headless invocation for eval runs."""


def run_agent(
    workspace: str,
    prompt: str,
    settings_file: str,
    model: str = "claude-sonnet-4-5-20250929",
    max_budget_usd: float = 2.00,
) -> dict:
    """Run Claude Code headless on a workspace with the given prompt.

    Returns a dict with session results (output, token usage, tool calls).
    """
    raise NotImplementedError("Agent runner not yet implemented")
