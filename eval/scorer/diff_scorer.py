"""Compare diffs against ground truth (files touched, precision/recall)."""


def score_diff(workspace: str, ground_truth_commit: str) -> dict:
    """Compare the workspace diff against the ground truth commit.

    Returns a dict with keys: file_precision, file_recall, files_touched, ground_truth_files.
    """
    raise NotImplementedError("Diff scorer not yet implemented")
