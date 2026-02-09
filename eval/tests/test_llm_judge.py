"""Tests for eval.scorer.llm_judge module.

Uses mocked Anthropic API calls to test the pairwise LLM judge logic
including flip-and-draw bias mitigation, JSON extraction, and validation.
"""

from __future__ import annotations

import json
from unittest.mock import MagicMock, patch

import pytest

from scorer.llm_judge import (
    DIMENSIONS,
    LLMJudgeError,
    _extract_json,
    _flip_result,
    _merge_results,
    _render_prompt,
    _validate_judgement,
    judge_pairwise,
)


# ---------------------------------------------------------------------------
# Fixtures
# ---------------------------------------------------------------------------

def _make_judgement(winner: str = "a", a_scores=(4, 3, 5), b_scores=(3, 4, 2)) -> dict:
    """Build a valid judgement dict for testing."""
    dims = {}
    for i, dim in enumerate(DIMENSIONS):
        dims[dim] = {"a": a_scores[i], "b": b_scores[i], "reasoning": f"{dim} reasoning"}
    return {
        "dimensions": dims,
        "overall_winner": winner,
        "reasoning": "A is better overall",
    }


SAMPLE_DIFF_A = """\
--- a/file.py
+++ b/file.py
@@ -1,3 +1,3 @@
-old line
+new line
"""

SAMPLE_DIFF_B = """\
--- a/file.py
+++ b/file.py
@@ -1,3 +1,5 @@
-old line
+new line
+extra line
"""

SAMPLE_CONTEXT = {"repo": "test/repo", "description": "Fix a bug", "language": "python"}


# ---------------------------------------------------------------------------
# _extract_json
# ---------------------------------------------------------------------------

class TestExtractJson:
    def test_plain_json(self):
        data = {"winner": "a"}
        result = _extract_json(json.dumps(data))
        assert result == data

    def test_fenced_json(self):
        text = 'Some preamble\n```json\n{"winner": "b"}\n```\nSome postamble'
        assert _extract_json(text) == {"winner": "b"}

    def test_fenced_no_lang(self):
        text = '```\n{"winner": "tie"}\n```'
        assert _extract_json(text) == {"winner": "tie"}

    def test_invalid_json_raises(self):
        with pytest.raises(LLMJudgeError, match="Failed to parse"):
            _extract_json("not json at all")

    def test_nested_json(self):
        data = {"dimensions": {"consistency": {"a": 3, "b": 4}}}
        result = _extract_json(json.dumps(data))
        assert result["dimensions"]["consistency"]["a"] == 3


# ---------------------------------------------------------------------------
# _validate_judgement
# ---------------------------------------------------------------------------

class TestValidateJudgement:
    def test_valid_judgement(self):
        raw = _make_judgement()
        result = _validate_judgement(raw)
        assert result["overall_winner"] == "a"
        for dim in DIMENSIONS:
            assert dim in result["dimensions"]
            assert "a" in result["dimensions"][dim]
            assert "b" in result["dimensions"][dim]

    def test_missing_dimensions_key(self):
        with pytest.raises(LLMJudgeError, match="missing 'dimensions'"):
            _validate_judgement({"overall_winner": "a"})

    def test_missing_overall_winner(self):
        with pytest.raises(LLMJudgeError, match="missing 'overall_winner'"):
            _validate_judgement({"dimensions": {}})

    def test_missing_dimension(self):
        raw = _make_judgement()
        del raw["dimensions"]["consistency"]
        with pytest.raises(LLMJudgeError, match="missing dimension 'consistency'"):
            _validate_judgement(raw)

    def test_missing_score_label(self):
        raw = _make_judgement()
        del raw["dimensions"]["consistency"]["a"]
        with pytest.raises(LLMJudgeError, match="missing score for 'a'"):
            _validate_judgement(raw)

    def test_score_out_of_range_low(self):
        raw = _make_judgement()
        raw["dimensions"]["consistency"]["a"] = 0
        with pytest.raises(LLMJudgeError, match="must be 1-5"):
            _validate_judgement(raw)

    def test_score_out_of_range_high(self):
        raw = _make_judgement()
        raw["dimensions"]["completeness"]["b"] = 6
        with pytest.raises(LLMJudgeError, match="must be 1-5"):
            _validate_judgement(raw)

    def test_invalid_winner(self):
        raw = _make_judgement(winner="c")
        with pytest.raises(LLMJudgeError, match="must be 'a', 'b', or 'tie'"):
            _validate_judgement(raw)

    def test_winner_normalised_lowercase(self):
        raw = _make_judgement(winner="TIE")
        result = _validate_judgement(raw)
        assert result["overall_winner"] == "tie"

    def test_winner_stripped_whitespace(self):
        raw = _make_judgement(winner=" a ")
        result = _validate_judgement(raw)
        assert result["overall_winner"] == "a"

    def test_float_scores_accepted(self):
        raw = _make_judgement()
        raw["dimensions"]["consistency"]["a"] = 3.5
        result = _validate_judgement(raw)
        assert result["dimensions"]["consistency"]["a"] == 3.5


# ---------------------------------------------------------------------------
# _render_prompt
# ---------------------------------------------------------------------------

class TestRenderPrompt:
    def test_renders_diffs(self):
        prompt = _render_prompt("diff A content", "diff B content")
        assert "diff A content" in prompt
        assert "diff B content" in prompt

    def test_renders_context(self):
        prompt = _render_prompt(
            "diff A", "diff B",
            repo="my/repo",
            description="Fix issue",
            language="rust",
        )
        assert "my/repo" in prompt
        assert "Fix issue" in prompt
        assert "rust" in prompt

    def test_renders_dimensions(self):
        prompt = _render_prompt("a", "b")
        assert "Consistency" in prompt
        assert "Completeness" in prompt
        assert "Minimality" in prompt


# ---------------------------------------------------------------------------
# _flip_result
# ---------------------------------------------------------------------------

class TestFlipResult:
    def test_scores_swapped(self):
        original = _make_judgement(winner="a", a_scores=(5, 4, 3), b_scores=(2, 3, 4))
        validated = _validate_judgement(original)
        flipped = _flip_result(validated)

        for dim in DIMENSIONS:
            assert flipped["dimensions"][dim]["a"] == validated["dimensions"][dim]["b"]
            assert flipped["dimensions"][dim]["b"] == validated["dimensions"][dim]["a"]

    def test_winner_a_becomes_b(self):
        result = _validate_judgement(_make_judgement(winner="a"))
        assert _flip_result(result)["overall_winner"] == "b"

    def test_winner_b_becomes_a(self):
        result = _validate_judgement(_make_judgement(winner="b"))
        assert _flip_result(result)["overall_winner"] == "a"

    def test_tie_stays_tie(self):
        result = _validate_judgement(_make_judgement(winner="tie"))
        assert _flip_result(result)["overall_winner"] == "tie"


# ---------------------------------------------------------------------------
# _merge_results
# ---------------------------------------------------------------------------

class TestMergeResults:
    def test_agree_on_winner(self):
        forward = _validate_judgement(_make_judgement(winner="a", a_scores=(4, 4, 4), b_scores=(2, 2, 2)))
        flipped = _validate_judgement(_make_judgement(winner="a", a_scores=(5, 5, 5), b_scores=(3, 3, 3)))
        merged = _merge_results(forward, flipped)

        assert merged["overall_winner"] == "a"
        assert merged["bias_detected"] is False

    def test_disagree_becomes_tie(self):
        forward = _validate_judgement(_make_judgement(winner="a"))
        flipped = _validate_judgement(_make_judgement(winner="b"))
        merged = _merge_results(forward, flipped)

        assert merged["overall_winner"] == "tie"
        assert merged["bias_detected"] is True

    def test_scores_averaged(self):
        forward = _validate_judgement(
            _make_judgement(winner="a", a_scores=(4, 4, 4), b_scores=(2, 2, 2))
        )
        flipped = _validate_judgement(
            _make_judgement(winner="a", a_scores=(2, 2, 2), b_scores=(4, 4, 4))
        )
        merged = _merge_results(forward, flipped)

        for dim in DIMENSIONS:
            assert merged["dimensions"][dim]["a"] == 3.0
            assert merged["dimensions"][dim]["b"] == 3.0

    def test_both_tie_stays_tie(self):
        forward = _validate_judgement(_make_judgement(winner="tie"))
        flipped = _validate_judgement(_make_judgement(winner="tie"))
        merged = _merge_results(forward, flipped)

        assert merged["overall_winner"] == "tie"
        assert merged["bias_detected"] is False


# ---------------------------------------------------------------------------
# judge_pairwise (integration with mocked API)
# ---------------------------------------------------------------------------

def _mock_api_response(judgement_dict: dict) -> MagicMock:
    """Create a mock Anthropic message response."""
    content_block = MagicMock()
    content_block.text = json.dumps(judgement_dict)
    message = MagicMock()
    message.content = [content_block]
    return message


class TestJudgePairwise:
    @patch("scorer.llm_judge._call_llm")
    def test_consistent_winner(self, mock_llm):
        """Both orderings agree → winner preserved."""
        judgement_a_first = _make_judgement(winner="a", a_scores=(5, 5, 5), b_scores=(2, 2, 2))
        # When B goes first, the judge should still prefer the "better" solution.
        # Since B is now in position A, the raw response says "b" (meaning the original A).
        judgement_b_first = _make_judgement(winner="b", a_scores=(2, 2, 2), b_scores=(5, 5, 5))

        mock_llm.side_effect = [
            json.dumps(judgement_a_first),
            json.dumps(judgement_b_first),
        ]

        result = judge_pairwise(SAMPLE_DIFF_A, SAMPLE_DIFF_B, SAMPLE_CONTEXT)

        assert result["overall_winner"] == "a"
        assert result["bias_detected"] is False
        assert mock_llm.call_count == 2

    @patch("scorer.llm_judge._call_llm")
    def test_position_bias_detected(self, mock_llm):
        """Position bias: first solution always wins → tie."""
        # Forward: A wins (A is in position 1)
        judgement_forward = _make_judgement(winner="a", a_scores=(5, 5, 5), b_scores=(2, 2, 2))
        # Flipped: A still "wins" but now A is in position 1 again (was B originally)
        # This means the judge just picks whatever is first → position bias
        judgement_flipped = _make_judgement(winner="a", a_scores=(5, 5, 5), b_scores=(2, 2, 2))

        mock_llm.side_effect = [
            json.dumps(judgement_forward),
            json.dumps(judgement_flipped),
        ]

        result = judge_pairwise(SAMPLE_DIFF_A, SAMPLE_DIFF_B, SAMPLE_CONTEXT)

        assert result["overall_winner"] == "tie"
        assert result["bias_detected"] is True

    @patch("scorer.llm_judge._call_llm")
    def test_empty_diff_a_raises(self, mock_llm):
        with pytest.raises(LLMJudgeError, match="diff_a is empty"):
            judge_pairwise("", SAMPLE_DIFF_B, SAMPLE_CONTEXT)
        mock_llm.assert_not_called()

    @patch("scorer.llm_judge._call_llm")
    def test_empty_diff_b_raises(self, mock_llm):
        with pytest.raises(LLMJudgeError, match="diff_b is empty"):
            judge_pairwise(SAMPLE_DIFF_A, "   ", SAMPLE_CONTEXT)
        mock_llm.assert_not_called()

    @patch("scorer.llm_judge._call_llm")
    def test_json_in_markdown_fence(self, mock_llm):
        """Judge returns JSON wrapped in markdown code fence."""
        judgement = _make_judgement(winner="b")
        fenced = f"Here is my analysis:\n```json\n{json.dumps(judgement)}\n```"
        mock_llm.side_effect = [fenced, fenced]

        result = judge_pairwise(SAMPLE_DIFF_A, SAMPLE_DIFF_B, SAMPLE_CONTEXT)
        # Both orderings say "b" → flipped says "a" → disagree → tie
        assert result["overall_winner"] == "tie"

    @patch("scorer.llm_judge._call_llm")
    def test_result_has_all_dimensions(self, mock_llm):
        judgement = _make_judgement(winner="tie")
        mock_llm.return_value = json.dumps(judgement)

        result = judge_pairwise(SAMPLE_DIFF_A, SAMPLE_DIFF_B, SAMPLE_CONTEXT)

        for dim in DIMENSIONS:
            assert dim in result["dimensions"]
            assert "a" in result["dimensions"][dim]
            assert "b" in result["dimensions"][dim]

    @patch("scorer.llm_judge._call_llm")
    def test_api_error_propagated(self, mock_llm):
        mock_llm.side_effect = LLMJudgeError("API error")

        with pytest.raises(LLMJudgeError, match="API error"):
            judge_pairwise(SAMPLE_DIFF_A, SAMPLE_DIFF_B, SAMPLE_CONTEXT)

    @patch("scorer.llm_judge._call_llm")
    def test_invalid_json_from_llm(self, mock_llm):
        mock_llm.return_value = "I cannot evaluate these diffs."

        with pytest.raises(LLMJudgeError, match="Failed to parse"):
            judge_pairwise(SAMPLE_DIFF_A, SAMPLE_DIFF_B, SAMPLE_CONTEXT)

    @patch("scorer.llm_judge._call_llm")
    def test_model_parameter_forwarded(self, mock_llm):
        judgement = _make_judgement(winner="tie")
        mock_llm.return_value = json.dumps(judgement)

        judge_pairwise(
            SAMPLE_DIFF_A, SAMPLE_DIFF_B, SAMPLE_CONTEXT,
            model="claude-haiku-4-5-20251001",
        )

        # Both calls should use the specified model
        for call in mock_llm.call_args_list:
            assert call.kwargs.get("model") == "claude-haiku-4-5-20251001"

    @patch("scorer.llm_judge._call_llm")
    def test_context_defaults_to_empty(self, mock_llm):
        """Empty context dict should not cause errors."""
        judgement = _make_judgement(winner="a")
        # Forward: A wins. Flipped raw: also says A wins → after flip-back = B → disagree → tie
        mock_llm.return_value = json.dumps(judgement)

        result = judge_pairwise(SAMPLE_DIFF_A, SAMPLE_DIFF_B, {})

        assert result["overall_winner"] in ("a", "b", "tie")

    @patch("scorer.llm_judge._call_llm")
    def test_b_wins_consistently(self, mock_llm):
        """Both orderings agree B is better."""
        # Forward: B wins
        forward = _make_judgement(winner="b", a_scores=(2, 2, 2), b_scores=(5, 5, 5))
        # Flipped: original-B is now in position A, judge says "a" (meaning original B)
        flipped = _make_judgement(winner="a", a_scores=(5, 5, 5), b_scores=(2, 2, 2))

        mock_llm.side_effect = [json.dumps(forward), json.dumps(flipped)]

        result = judge_pairwise(SAMPLE_DIFF_A, SAMPLE_DIFF_B, SAMPLE_CONTEXT)

        assert result["overall_winner"] == "b"
        assert result["bias_detected"] is False
