"""Combine all scorer outputs into a unified result."""


def aggregate(test_results: dict, diff_results: dict, judge_results: dict) -> dict:
    """Aggregate results from all scorers into a single summary.

    Returns a dict with combined metrics.
    """
    raise NotImplementedError("Aggregator not yet implemented")
