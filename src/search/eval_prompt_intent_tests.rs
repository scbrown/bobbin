//! Intent classification over the *actual* eval-harness prompts.
//!
//! Why this file exists (bobbin-53). The context-injection paper's ablation
//! numbers were measured before `bobbin-lpp` fixed an inverted doc-demotion
//! factor on the local hook path — the path `bobbin hook inject-context`
//! takes, and therefore the path every eval run took. Whether that fix
//! invalidates the measurements depends entirely on one question: what
//! `doc_demotion_factor` did the eval prompts actually select?
//!
//! At `factor == 1.0` the buggy and fixed forms are arithmetically identical
//! (`base * 1.0 == 1.0 - (1.0 - base) * 1.0`), so the fix is a no-op and the
//! numbers stand. At any other factor they diverge and the arm is void.
//!
//! The fixtures below are not paraphrases. Each is the exact string
//! `classify_intent` receives for that task: `_build_prompt` in
//! `eval/runner/cli.py` assembled with `approach="with-bobbin"`, then reduced
//! by the hook's own last-500-chars truncation. Every eval prompt exceeds 500
//! characters, so every one is truncated, and the 416-character bobbin
//! instruction block is shared verbatim by all of them — which is *why* the
//! answer comes out uniform.
//!
//! Regenerate with the script in `docs/plans/paper-measurement-validity.md`.

use super::*;

/// The exact intent-classification window for each of the paper's 13 tasks.
fn eval_prompt_windows() -> Vec<(&'static str, &'static str)> {
    vec![
        (
            "ruff-001",
            r####"test suite with `cargo test -p ruff_python_formatter -- except_handler` to verify.

This project has bobbin installed (a semantic code search engine). Before exploring manually, use bobbin to find relevant code:
- `bobbin search "<key terms from the task>"` — semantic + keyword search
- `bobbin related <file>` — find test files and co-changing dependencies
- `bobbin refs <SymbolName>` — trace definitions and usages
Start with bobbin search to orient yourself, then read the files it identifies."####,
        ),
        (
            "ruff-002",
            r####"the test suite with `cargo test -p ruff_linter -- flake8_boolean_trap` to verify.

This project has bobbin installed (a semantic code search engine). Before exploring manually, use bobbin to find relevant code:
- `bobbin search "<key terms from the task>"` — semantic + keyword search
- `bobbin related <file>` — find test files and co-changing dependencies
- `bobbin refs <SymbolName>` — trace definitions and usages
Start with bobbin search to orient yourself, then read the files it identifies."####,
        ),
        (
            "ruff-003",
            r####"the fix. Run the test suite with `cargo test -p ruff_linter -- PLC2701` to verify.

This project has bobbin installed (a semantic code search engine). Before exploring manually, use bobbin to find relevant code:
- `bobbin search "<key terms from the task>"` — semantic + keyword search
- `bobbin related <file>` — find test files and co-changing dependencies
- `bobbin refs <SymbolName>` — trace definitions and usages
Start with bobbin search to orient yourself, then read the files it identifies."####,
        ),
        (
            "ruff-004",
            r####"the fix. Run the test suite with `cargo test -p ruff_linter -- PYI034` to verify.

This project has bobbin installed (a semantic code search engine). Before exploring manually, use bobbin to find relevant code:
- `bobbin search "<key terms from the task>"` — semantic + keyword search
- `bobbin related <file>` — find test files and co-changing dependencies
- `bobbin refs <SymbolName>` — trace definitions and usages
Start with bobbin search to orient yourself, then read the files it identifies."####,
        ),
        (
            "ruff-005",
            r####"the fix. Run the test suite with `cargo test -p ruff -- format` to verify.

This project has bobbin installed (a semantic code search engine). Before exploring manually, use bobbin to find relevant code:
- `bobbin search "<key terms from the task>"` — semantic + keyword search
- `bobbin related <file>` — find test files and co-changing dependencies
- `bobbin refs <SymbolName>` — trace definitions and usages
Start with bobbin search to orient yourself, then read the files it identifies."####,
        ),
        (
            "flask-001",
            r####"the test suite with `.venv/bin/pytest tests/test_basic.py -x -k session` to verify.

This project has bobbin installed (a semantic code search engine). Before exploring manually, use bobbin to find relevant code:
- `bobbin search "<key terms from the task>"` — semantic + keyword search
- `bobbin related <file>` — find test files and co-changing dependencies
- `bobbin refs <SymbolName>` — trace definitions and usages
Start with bobbin search to orient yourself, then read the files it identifies."####,
        ),
        (
            "flask-002",
            r####"the fix. Run the test suite with `.venv/bin/pytest tests/test_cli.py -x` to verify.

This project has bobbin installed (a semantic code search engine). Before exploring manually, use bobbin to find relevant code:
- `bobbin search "<key terms from the task>"` — semantic + keyword search
- `bobbin related <file>` — find test files and co-changing dependencies
- `bobbin refs <SymbolName>` — trace definitions and usages
Start with bobbin search to orient yourself, then read the files it identifies."####,
        ),
        (
            "flask-003",
            r####"test suite with `.venv/bin/pytest tests/test_helpers.py -x -k redirect` to verify.

This project has bobbin installed (a semantic code search engine). Before exploring manually, use bobbin to find relevant code:
- `bobbin search "<key terms from the task>"` — semantic + keyword search
- `bobbin related <file>` — find test files and co-changing dependencies
- `bobbin refs <SymbolName>` — trace definitions and usages
Start with bobbin search to orient yourself, then read the files it identifies."####,
        ),
        (
            "flask-004",
            r####"the test suite with `.venv/bin/pytest tests/test_helpers.py -x -k abort` to verify.

This project has bobbin installed (a semantic code search engine). Before exploring manually, use bobbin to find relevant code:
- `bobbin search "<key terms from the task>"` — semantic + keyword search
- `bobbin related <file>` — find test files and co-changing dependencies
- `bobbin refs <SymbolName>` — trace definitions and usages
Start with bobbin search to orient yourself, then read the files it identifies."####,
        ),
        (
            "flask-005",
            r####"tests/test_basic.py tests/test_user_error_handler.py -x` to verify.

This project has bobbin installed (a semantic code search engine). Before exploring manually, use bobbin to find relevant code:
- `bobbin search "<key terms from the task>"` — semantic + keyword search
- `bobbin related <file>` — find test files and co-changing dependencies
- `bobbin refs <SymbolName>` — trace definitions and usages
Start with bobbin search to orient yourself, then read the files it identifies."####,
        ),
        (
            "cargo-001",
            r####"testsuite -- check::check_build_should_not_uplift_proc_macro_dylib_deps` to verify.

This project has bobbin installed (a semantic code search engine). Before exploring manually, use bobbin to find relevant code:
- `bobbin search "<key terms from the task>"` — semantic + keyword search
- `bobbin related <file>` — find test files and co-changing dependencies
- `bobbin refs <SymbolName>` — trace definitions and usages
Start with bobbin search to orient yourself, then read the files it identifies."####,
        ),
        (
            "polars-004",
            r####"-x -k test_concat_str_sortedness_26466` to verify.

This project has bobbin installed (a semantic code search engine). Before exploring manually, use bobbin to find relevant code:
- `bobbin search "<key terms from the task>"` — semantic + keyword search
- `bobbin related <file>` — find test files and co-changing dependencies
- `bobbin refs <SymbolName>` — trace definitions and usages
Start with bobbin search to orient yourself, then read the files it identifies."####,
        ),
        (
            "polars-005",
            r####"py-polars/tests/unit/expr/test_literal.py -x -k test_floordiv_mod` to verify.

This project has bobbin installed (a semantic code search engine). Before exploring manually, use bobbin to find relevant code:
- `bobbin search "<key terms from the task>"` — semantic + keyword search
- `bobbin related <file>` — find test files and co-changing dependencies
- `bobbin refs <SymbolName>` — trace definitions and usages
Start with bobbin search to orient yourself, then read the files it identifies."####,
        ),
    ]
}

/// What today's classifier makes of each eval prompt.
///
/// This is a *pin*, not an aspiration. It exists so that a change to
/// `classify_intent` which would silently alter what the eval harness measures
/// shows up as a failing test rather than as a shifted number in a re-run.
///
/// The split is itself the finding: 10 of 13 land on Navigation because the
/// shared 416-character bobbin instruction block dominates the 500-character
/// window with navigation signals (`find`, `files`, `definitions`, `read the`).
/// The three Operational cases are the ones whose ~84 characters of surviving
/// task text happen to add `check`/`status`-family signals on top.
const EXPECTED_INTENTS: &[(&str, QueryIntent)] = &[
    ("ruff-001", QueryIntent::Navigation),
    ("ruff-002", QueryIntent::Navigation),
    ("ruff-003", QueryIntent::Operational),
    ("ruff-004", QueryIntent::Operational),
    ("ruff-005", QueryIntent::Operational),
    ("flask-001", QueryIntent::Navigation),
    ("flask-002", QueryIntent::Navigation),
    ("flask-003", QueryIntent::Navigation),
    ("flask-004", QueryIntent::Navigation),
    ("flask-005", QueryIntent::Navigation),
    ("cargo-001", QueryIntent::Navigation),
    ("polars-004", QueryIntent::Navigation),
    ("polars-005", QueryIntent::Navigation),
];

#[test]
fn test_eval_prompt_intents_are_pinned() {
    let windows = eval_prompt_windows();
    assert_eq!(windows.len(), EXPECTED_INTENTS.len());
    for ((task, window), (expected_task, expected)) in windows.iter().zip(EXPECTED_INTENTS) {
        assert_eq!(task, expected_task, "fixture order drifted");
        assert_eq!(
            classify_intent(window),
            *expected,
            "{task}'s classified intent changed — the eval harness now measures \
             something different from what the paper reports. Re-check \
             docs/plans/paper-measurement-validity.md before updating this pin.",
        );
    }
}

/// The classifier is decided by the boilerplate, not by the task.
///
/// This is the mechanism behind the pin above, asserted directly: the shared
/// instruction block alone classifies the same way as the majority of full
/// task windows. An eval harness whose intent signal comes from its own
/// prompt template is not exercising intent classification at all.
#[test]
fn test_the_shared_instruction_block_alone_drives_the_classification() {
    const BLOCK: &str = "This project has bobbin installed (a semantic code search engine). \
Before exploring manually, use bobbin to find relevant code:\n\
- `bobbin search \"<key terms from the task>\"` — semantic + keyword search\n\
- `bobbin related <file>` — find test files and co-changing dependencies\n\
- `bobbin refs <SymbolName>` — trace definitions and usages\n\
Start with bobbin search to orient yourself, then read the files it identifies.";

    assert_eq!(
        classify_intent(BLOCK),
        QueryIntent::Navigation,
        "the boilerplate no longer dominates; the analysis in \
         docs/plans/paper-measurement-validity.md needs revisiting",
    );

    let majority = eval_prompt_windows()
        .iter()
        .filter(|(_, w)| classify_intent(w) == QueryIntent::Navigation)
        .count();
    assert!(
        majority * 2 > EXPECTED_INTENTS.len(),
        "expected the boilerplate's classification to dominate, got {majority}/13",
    );
}

#[test]
fn test_the_lpp_fix_is_arithmetically_a_no_op_at_factor_one() {
    // The claim the paper's validity rests on, stated as arithmetic rather
    // than as prose: at factor 1.0 the pre-fix raw multiply and the post-fix
    // effect-space form agree, so no measurement taken at factor 1.0 moved.
    for base in [0.0_f32, 0.1, 0.3, 0.5, 0.7, 1.0] {
        let buggy = (base * 1.0_f32).clamp(0.01, 1.0);
        let fixed = apply_doc_demotion_factor(base, 1.0, 0.01);
        assert!(
            (buggy - fixed).abs() < f32::EPSILON,
            "at base {base} the two forms disagree: {buggy} vs {fixed}",
        );
    }
}

#[test]
fn test_the_lpp_fix_does_diverge_at_every_non_neutral_shipped_factor() {
    // The guard on the test above: it would pass vacuously if the two forms
    // agreed everywhere. They do not — which is what made bobbin-lpp a P1.
    let base = 0.3_f32; // SearchConfig::default().doc_demotion
    for intent in [
        QueryIntent::BugFix,
        QueryIntent::Architecture,
        QueryIntent::Implementation,
        QueryIntent::Configuration,
        QueryIntent::Operational,
    ] {
        let factor = intent_adjustments(intent).doc_demotion_factor;
        let buggy = (base * factor).clamp(0.01, 1.0);
        let fixed = apply_doc_demotion_factor(base, factor, 0.01);
        assert!(
            (buggy - fixed).abs() > 0.01,
            "{intent:?} (factor {factor}) should diverge but did not: \
             {buggy} vs {fixed}",
        );
    }
}
