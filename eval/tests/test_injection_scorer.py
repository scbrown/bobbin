"""Tests for eval.scorer.injection_scorer module."""

from __future__ import annotations

import pytest

from scorer.injection_scorer import score_injection_usage


class TestScoreInjectionUsage:
    def test_perfect_overlap(self):
        """All injected files were touched and all touched files were injected."""
        result = score_injection_usage(
            injected_files=["src/a.py", "src/b.py"],
            files_touched=["src/a.py", "src/b.py"],
        )
        assert result["injection_precision"] == 1.0
        assert result["injection_recall"] == 1.0
        assert result["injection_f1"] == 1.0
        assert sorted(result["injected_and_touched"]) == ["src/a.py", "src/b.py"]
        assert result["injected_not_touched"] == []
        assert result["touched_not_injected"] == []

    def test_partial_overlap(self):
        """Some injected files were touched, agent also touched extras."""
        result = score_injection_usage(
            injected_files=["src/a.py", "src/b.py", "src/c.py"],
            files_touched=["src/a.py", "src/d.py"],
        )
        # Precision: 1/3 (only a.py from 3 injected)
        assert result["injection_precision"] == pytest.approx(0.3333, abs=0.001)
        # Recall: 1/2 (a.py from 2 touched)
        assert result["injection_recall"] == 0.5
        assert result["injected_and_touched"] == ["src/a.py"]
        assert sorted(result["injected_not_touched"]) == ["src/b.py", "src/c.py"]
        assert result["touched_not_injected"] == ["src/d.py"]

    def test_no_overlap(self):
        """Agent touched none of the injected files."""
        result = score_injection_usage(
            injected_files=["src/a.py", "src/b.py"],
            files_touched=["src/c.py", "src/d.py"],
        )
        assert result["injection_precision"] == 0.0
        assert result["injection_recall"] == 0.0
        assert result["injection_f1"] == 0.0
        assert result["injected_and_touched"] == []

    def test_empty_injected(self):
        """No files were injected."""
        result = score_injection_usage(
            injected_files=[],
            files_touched=["src/a.py"],
        )
        assert result["injection_precision"] == 0.0
        assert result["injection_recall"] == 0.0
        assert result["injection_f1"] == 0.0

    def test_empty_touched(self):
        """Agent touched no files."""
        result = score_injection_usage(
            injected_files=["src/a.py", "src/b.py"],
            files_touched=[],
        )
        assert result["injection_precision"] == 0.0
        assert result["injection_recall"] == 0.0
        assert result["injection_f1"] == 0.0

    def test_both_empty(self):
        """No files injected, no files touched."""
        result = score_injection_usage(
            injected_files=[],
            files_touched=[],
        )
        assert result["injection_precision"] == 0.0
        assert result["injection_recall"] == 0.0
        assert result["injection_f1"] == 0.0

    def test_f1_harmonic_mean(self):
        """Verify F1 is the harmonic mean of precision and recall."""
        result = score_injection_usage(
            injected_files=["a.py"],
            files_touched=["a.py", "b.py"],
        )
        # Precision: 1/1 = 1.0, Recall: 1/2 = 0.5
        # F1 = 2 * 1.0 * 0.5 / (1.0 + 0.5) = 0.6667
        assert result["injection_precision"] == 1.0
        assert result["injection_recall"] == 0.5
        assert result["injection_f1"] == pytest.approx(0.6667, abs=0.001)

    def test_duplicates_in_input(self):
        """Duplicate paths in inputs should be deduplicated."""
        result = score_injection_usage(
            injected_files=["src/a.py", "src/a.py", "src/b.py"],
            files_touched=["src/a.py", "src/a.py"],
        )
        # After dedup: injected={a,b}, touched={a}
        # Precision: 1/2, Recall: 1/1
        assert result["injection_precision"] == 0.5
        assert result["injection_recall"] == 1.0
