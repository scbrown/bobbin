"""Pairwise LLM-as-judge scoring."""


def judge_pairwise(diff_a: str, diff_b: str, context: dict) -> dict:
    """Compare two diffs using an LLM judge (pairwise comparison).

    Uses flip-and-draw protocol to counter position bias.

    Returns a dict with keys: winner (a/b/tie), reasoning, dimensions.
    """
    raise NotImplementedError("LLM judge not yet implemented")
