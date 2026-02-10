# Evaluation Methodology

Bobbin's evaluation framework measures how semantic code context affects AI agent performance on real bug fixes across open-source projects.

## Approach: Commit-Revert

Each evaluation task is based on a real bug fix from a well-tested open-source project:

1. **Select a commit** that fixes a bug and has a passing test suite
2. **Check out the parent** of that commit (the broken state)
3. **Give the agent** the bug description and test command
4. **Measure** whether the agent can reproduce the fix

This approach has several advantages:

- **Ground truth exists** — the actual commit shows exactly what needed to change
- **Tests are authoritative** — the project's own test suite validates correctness
- **Difficulty is natural** — real bugs have realistic complexity and cross-file dependencies

## Two Approaches

Each task is run twice:

| Approach | Description |
|----------|-------------|
| **no-bobbin** | Agent works with only its built-in knowledge and the prompt |
| **with-bobbin** | Agent receives semantic code context via bobbin's hook system |

The with-bobbin approach injects relevant code snippets automatically when the agent processes its prompt, giving it awareness of related files, function signatures, and code patterns.

## Scoring Dimensions

### Test Pass Rate

Does the agent's fix make the test suite pass? This is the primary success metric.

### File-Level Precision

Of the files the agent modified, what fraction were in the ground truth? High precision means the agent didn't touch unnecessary files.

### File-Level Recall

Of the files in the ground truth, what fraction did the agent modify? High recall means the agent found all the files that needed changing.

### F1 Score

Harmonic mean of precision and recall. Balances surgical accuracy with completeness.

### Duration

Wall-clock time for the agent to complete its work. Faster is better, all else equal.

## LLM Judge

Optionally, an LLM judge performs pairwise comparison of agent diffs across three dimensions:

- **Consistency**: Does the solution follow codebase conventions?
- **Completeness**: Are edge cases handled?
- **Minimality**: Is the diff surgical, or does it include unnecessary changes?

The judge uses a flip-and-draw protocol (running comparison in both orders) to detect and mitigate position bias.

## Task Selection Criteria

Tasks are curated to be:

- **Self-contained** — fixable without external documentation or API access
- **Well-tested** — the project's test suite reliably catches the bug
- **Cross-file** — the fix typically touches 2-5 files (not trivial single-line changes)
- **Diverse** — spanning multiple languages, frameworks, and bug categories
