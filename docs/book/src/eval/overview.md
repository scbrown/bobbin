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

Each task is run twice under controlled conditions:

| Approach | Description | Settings |
|----------|-------------|----------|
| **no-bobbin** | Agent works with only its built-in knowledge and the prompt | Empty hooks (isolated from user config) |
| **with-bobbin** | Agent receives semantic code context via bobbin's hook system | `bobbin hook inject-context` on each prompt |

The with-bobbin approach injects relevant code snippets automatically when the agent processes its prompt, giving it awareness of related files, function signatures, and code patterns.

### Isolation

Each run uses an independent, freshly cloned workspace in a temporary directory. The no-bobbin approach uses an explicit empty settings file (`settings-no-bobbin.json`) to prevent contamination from user-level Claude Code hooks. This ensures the control group never receives bobbin context.

## Scoring Dimensions

### Test Pass Rate

Does the agent's fix make the test suite pass? This is the primary success metric — a fix that doesn't pass tests is a failed attempt, regardless of how close the code looks.

### File-Level Precision

**Definition**: Of the files the agent modified, what fraction were also modified in the ground truth commit?

```text
Precision = |agent_files ∩ ground_truth_files| / |agent_files|
```

**What it measures**: Surgical accuracy. High precision (close to 1.0) means the agent only touched files that actually needed changing. Low precision means the agent made unnecessary modifications — touching files that weren't part of the real fix.

**Example**: Ground truth modifies files A, B, C. Agent modifies A, B, D, E.

- `Precision = |{A,B}| / |{A,B,D,E}| = 2/4 = 0.50`
- The agent found 2 correct files but also touched 2 unnecessary ones.

### File-Level Recall

**Definition**: Of the files modified in the ground truth commit, what fraction did the agent also modify?

```text
Recall = |agent_files ∩ ground_truth_files| / |ground_truth_files|
```

**What it measures**: Completeness. High recall (close to 1.0) means the agent found all files that needed changing. Low recall means the agent missed some required files.

**Example**: Ground truth modifies files A, B, C. Agent modifies A, B, D, E.

- `Recall = |{A,B}| / |{A,B,C}| = 2/3 = 0.67`
- The agent found 2 of the 3 required files but missed file C.

### F1 Score

**Definition**: The harmonic mean of precision and recall.

```text
F1 = 2 × (Precision × Recall) / (Precision + Recall)
```

**Why harmonic mean?** Unlike an arithmetic mean, the harmonic mean penalizes extreme imbalances. An agent that touches every file in the repo would have recall = 1.0 but precision ≈ 0.0, and F1 would correctly be near 0 rather than 0.5.

**Interpretation guide**:

| F1 Range | Meaning |
|----------|---------|
| 1.0 | Perfect — agent modified exactly the same files as the ground truth |
| 0.7-0.9 | Strong — agent found most files with minimal extras |
| 0.4-0.6 | Partial — agent found some files but missed others or added extras |
| 0.0-0.3 | Weak — agent's changes have little overlap with the ground truth |

**Why F1 matters for context engines**: Bobbin's value proposition is that semantic context helps agents find the *right* files to modify. Without context, agents often explore broadly (low precision) or miss related files (low recall). F1 captures both failure modes in a single number.

### Duration

Wall-clock time for the agent to complete its work. Includes thinking, tool calls, and compilation. Faster is better, all else equal, but correctness always trumps speed.

## GPU-Accelerated Indexing

The with-bobbin approach requires indexing the target codebase before the agent starts. Bobbin automatically detects NVIDIA CUDA GPUs and uses them for embedding inference:

| Project | Files | Chunks | CPU Index Time | GPU Index Time |
|---------|-------|--------|----------------|----------------|
| ruff | ~5,000 | ~57K | >30 min (timeout) | ~83s |

GPU acceleration makes large-codebase evaluation practical. Without it, indexing ruff's 57K chunks was the primary bottleneck — consistently timing out at the 30-minute mark. With GPU (RTX 4070 Super), embedding throughput jumps from ~100 chunks/s to ~2,400 chunks/s.

The GPU is only used during the indexing phase. Search queries are sub-100ms regardless.

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

Current task suites:

| Suite | Project | Language | Files | Tasks | Difficulty |
|-------|---------|----------|-------|-------|------------|
| flask | pallets/flask | Python | ~50 | 5 | easy-medium |
| polars | pola-rs/polars | Rust+Python | ~2,000 | 5 | easy-medium |
| ruff | astral-sh/ruff | Rust | ~5,000 | 5 | easy-medium |
