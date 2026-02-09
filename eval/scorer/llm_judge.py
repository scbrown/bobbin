"""Pairwise LLM-as-judge scoring with flip-and-draw bias mitigation."""

from __future__ import annotations

import json
import logging
import re
from pathlib import Path

import jinja2

logger = logging.getLogger(__name__)

DIMENSIONS = ("consistency", "completeness", "minimality")

_TEMPLATE_DIR = Path(__file__).resolve().parent.parent / "prompts"
_TEMPLATE_NAME = "pairwise_judge.md.j2"


class LLMJudgeError(Exception):
    """Raised when the LLM judge encounters a fatal error."""


def _load_template() -> jinja2.Template:
    """Load the pairwise judge Jinja2 prompt template."""
    env = jinja2.Environment(
        loader=jinja2.FileSystemLoader(str(_TEMPLATE_DIR)),
        autoescape=False,
        undefined=jinja2.StrictUndefined,
    )
    return env.get_template(_TEMPLATE_NAME)


def _render_prompt(
    diff_a: str,
    diff_b: str,
    *,
    repo: str = "",
    description: str = "",
    language: str = "",
) -> str:
    """Render the judge prompt with the given diffs and context."""
    template = _load_template()
    return template.render(
        repo=repo,
        description=description,
        language=language,
        diff_a=diff_a,
        diff_b=diff_b,
    )


def _extract_json(text: str) -> dict:
    """Extract JSON from an LLM response that may contain markdown fences."""
    # Try to find a JSON block in markdown fences first.
    fence_match = re.search(r"```(?:json)?\s*\n(.*?)\n\s*```", text, re.DOTALL)
    if fence_match:
        text = fence_match.group(1)

    try:
        return json.loads(text)
    except json.JSONDecodeError as exc:
        raise LLMJudgeError(f"Failed to parse judge response as JSON: {exc}") from exc


def _validate_judgement(data: dict) -> dict:
    """Validate and normalise the parsed judgement dict.

    Ensures required keys exist and scores are within bounds.
    Returns a cleaned copy.
    """
    if "dimensions" not in data:
        raise LLMJudgeError("Judge response missing 'dimensions' key")
    if "overall_winner" not in data:
        raise LLMJudgeError("Judge response missing 'overall_winner' key")

    dims = data["dimensions"]
    for dim in DIMENSIONS:
        if dim not in dims:
            raise LLMJudgeError(f"Judge response missing dimension '{dim}'")
        entry = dims[dim]
        for label in ("a", "b"):
            score = entry.get(label)
            if score is None:
                raise LLMJudgeError(f"Dimension '{dim}' missing score for '{label}'")
            if not isinstance(score, (int, float)) or not (1 <= score <= 5):
                raise LLMJudgeError(
                    f"Dimension '{dim}' score for '{label}' must be 1-5, got {score}"
                )

    winner = data["overall_winner"].strip().lower()
    if winner not in ("a", "b", "tie"):
        raise LLMJudgeError(f"overall_winner must be 'a', 'b', or 'tie', got '{winner}'")

    return {
        "dimensions": {
            dim: {
                "a": dims[dim]["a"],
                "b": dims[dim]["b"],
                "reasoning": dims[dim].get("reasoning", ""),
            }
            for dim in DIMENSIONS
        },
        "overall_winner": winner,
        "reasoning": data.get("reasoning", ""),
    }


def _call_llm(prompt: str, *, model: str = "claude-sonnet-4-5-20250929") -> str:
    """Call the Anthropic API and return the text response.

    Raises LLMJudgeError on API failures.
    """
    try:
        import anthropic
    except ImportError as exc:
        raise LLMJudgeError("anthropic package is required: pip install anthropic") from exc

    try:
        client = anthropic.Anthropic()
        message = client.messages.create(
            model=model,
            max_tokens=2048,
            messages=[{"role": "user", "content": prompt}],
        )
        return message.content[0].text
    except anthropic.APIError as exc:
        raise LLMJudgeError(f"Anthropic API error: {exc}") from exc


def _judge_single_ordering(
    diff_a: str,
    diff_b: str,
    context: dict,
    *,
    model: str = "claude-sonnet-4-5-20250929",
) -> dict:
    """Run a single judge call and return the validated result."""
    prompt = _render_prompt(
        diff_a,
        diff_b,
        repo=context.get("repo", ""),
        description=context.get("description", ""),
        language=context.get("language", ""),
    )
    response = _call_llm(prompt, model=model)
    raw = _extract_json(response)
    return _validate_judgement(raw)


def _flip_result(result: dict) -> dict:
    """Flip a judgement result so that A/B labels are swapped.

    This un-does the position swap, mapping the flipped ordering's labels
    back to the original A/B semantics.
    """
    flipped_dims = {}
    for dim in DIMENSIONS:
        entry = result["dimensions"][dim]
        flipped_dims[dim] = {
            "a": entry["b"],
            "b": entry["a"],
            "reasoning": entry.get("reasoning", ""),
        }

    winner = result["overall_winner"]
    if winner == "a":
        flipped_winner = "b"
    elif winner == "b":
        flipped_winner = "a"
    else:
        flipped_winner = "tie"

    return {
        "dimensions": flipped_dims,
        "overall_winner": flipped_winner,
        "reasoning": result.get("reasoning", ""),
    }


def _merge_results(forward: dict, flipped_back: dict) -> dict:
    """Merge two judgement results (forward and position-flipped) into a final verdict.

    Scores are averaged.  If the two orderings disagree on the winner, the
    merged result is a tie (bias detected).
    """
    merged_dims = {}
    for dim in DIMENSIONS:
        fwd = forward["dimensions"][dim]
        rev = flipped_back["dimensions"][dim]
        merged_dims[dim] = {
            "a": round((fwd["a"] + rev["a"]) / 2, 2),
            "b": round((fwd["b"] + rev["b"]) / 2, 2),
            "reasoning": fwd.get("reasoning", ""),
        }

    # Determine winner: if both orderings agree, use that; otherwise tie.
    if forward["overall_winner"] == flipped_back["overall_winner"]:
        winner = forward["overall_winner"]
    else:
        winner = "tie"
        logger.info(
            "Position bias detected: forward=%s, flipped=%s → tie",
            forward["overall_winner"],
            flipped_back["overall_winner"],
        )

    return {
        "dimensions": merged_dims,
        "overall_winner": winner,
        "reasoning": forward.get("reasoning", ""),
        "bias_detected": forward["overall_winner"] != flipped_back["overall_winner"],
    }


def judge_pairwise(
    diff_a: str,
    diff_b: str,
    context: dict,
    *,
    model: str = "claude-sonnet-4-5-20250929",
) -> dict:
    """Compare two diffs using an LLM judge (pairwise comparison).

    Uses flip-and-draw protocol to counter position bias: the judge is called
    twice — once with (A, B) order and once with (B, A) order — and the two
    results are reconciled.

    Parameters
    ----------
    diff_a:
        The diff for solution A.
    diff_b:
        The diff for solution B.
    context:
        Dict with optional keys ``repo``, ``description``, ``language`` used
        to populate the prompt template.
    model:
        Anthropic model identifier for the judge.

    Returns a dict with keys:
        winner         — "a", "b", or "tie"
        dimensions     — per-dimension scores (averaged across orderings)
        reasoning      — reasoning from the forward pass
        bias_detected  — whether the two orderings disagreed on the winner
    """
    if not diff_a.strip():
        raise LLMJudgeError("diff_a is empty")
    if not diff_b.strip():
        raise LLMJudgeError("diff_b is empty")

    # Forward pass: A in position 1, B in position 2.
    logger.info("LLM judge: forward pass (A first)")
    forward = _judge_single_ordering(diff_a, diff_b, context, model=model)

    # Flipped pass: B in position 1, A in position 2.
    logger.info("LLM judge: flipped pass (B first)")
    flipped_raw = _judge_single_ordering(diff_b, diff_a, context, model=model)
    flipped_back = _flip_result(flipped_raw)

    # Merge the two results.
    return _merge_results(forward, flipped_back)
