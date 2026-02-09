# Bobbin Eval Framework

## Context

Bobbin injects relevant code context into Claude Code via hooks, but we have no empirical proof it helps. We need a reproducible evaluation framework that compares Claude Code **with** and **without** bobbin on real coding tasks, using real human commits as ground truth. This serves two purposes: (1) guide iterative improvement of bobbin, and (2) produce compelling results to attract users.

## Approach: Commit-Revert Three-Way Comparison

Inspired by GitGoodBench and Cline-Bench, we use real commits from open-source repos as ground truth:

1. Pick a meaningful commit from a well-tested repo
2. Check out the parent commit (pre-fix state)
3. Give Claude Code the task described by the commit message
4. Compare three solutions: **human** (original commit), **AI** (no bobbin), **AI+bobbin** (hook active)

### Why This Works for Bobbin

- Cross-file commits require the agent to *discover* which files matter — exactly what bobbin pre-answers
- Real repos with history give bobbin's coupling analysis actual signal
- Test suites provide objective correctness validation
- The human commit is a natural quality ceiling

## Architecture

```
eval/
  pyproject.toml               # Python: click, pyyaml, anthropic, pandas
  tasks/                       # Curated task YAML files
    ruff-001.yaml
    flask-001.yaml
  runner/
    cli.py                     # `python -m eval.runner.cli run-task ruff-001`
    workspace.py               # Clone, checkout parent, snapshot
    bobbin_setup.py            # bobbin init + index on workspace
    agent_runner.py            # Claude Code headless invocation
  scorer/
    test_scorer.py             # Run repo tests, parse pass/fail
    diff_scorer.py             # Compare diffs (files touched, precision/recall)
    llm_judge.py               # Pairwise LLM-as-judge
    aggregator.py              # Combine all scores
  prompts/
    pairwise_judge.md.j2       # Judge prompt template (Jinja2)
  results/                     # JSON results per run (gitignored)
  analysis/
    report.py                  # Generate markdown summary
```

### Task Definition Format

```yaml
id: ruff-001
repo: astral-sh/ruff
commit: abc123
description: |
  The f-string linter rule (F541) incorrectly flags nested f-strings.
  Fix the parser to track nesting depth.
test_command: "cargo test -p ruff_linter -- f_string"
language: rust
difficulty: medium
tags: [cross-file, bug-fix]
```

### Runner Flow

For each task, for each approach (no-bobbin / with-bobbin), for each attempt (3x):

1. **Workspace setup**: Clone repo, checkout `commit^`, verify tests pass
2. **Bobbin setup** (with-bobbin only): `bobbin init && bobbin index`
3. **Run Claude Code headless**:
   ```bash
   claude -p "$PROMPT" \
     --model claude-sonnet-4-5-20250929 \
     --output-format json \
     --max-budget-usd 2.00 \
     --no-session-persistence \
     --settings "$SETTINGS_FILE"    # with or without bobbin hooks
   ```
4. **Score**: Run tests, capture diff, collect token/tool-call metrics
5. **Store** results JSON

Key detail: use `--settings` to point to different settings files (one with bobbin hooks configured, one without) rather than installing/uninstalling hooks per run.

### Prompt Template

```
You are working on the {repo_name} project.

{task.description}

Implement the fix. Run the test suite with `{task.test_command}` to verify.
```

Identical prompt for both approaches. The only difference is whether bobbin's hook fires.

## Scoring

### Automated Metrics

| Metric | Source | What it measures |
|--------|--------|-----------------|
| **Test pass rate** | Run test_command | Correctness (primary signal) |
| **File precision/recall** | Diff vs ground truth | Did it touch the right files? |
| **Tool call count** | Session JSON | Exploration efficiency |
| **Token usage** | Session JSON | Cost efficiency |
| **Time to first edit** | Session JSON | Orientation speed |
| **Retrieval precision/recall** | Bobbin injection log vs ground truth files | Context quality (bobbin-only) |

### LLM-as-Judge (Pairwise)

For quality dimensions automated metrics can't capture:

- **Consistency**: Does the code follow existing codebase patterns?
- **Completeness**: Are edge cases handled?
- **Minimality**: Surgical diff or unnecessary sprawl?

Protocol:
- Pairwise comparison (not absolute scoring) — 85% human agreement
- **Flip-and-draw**: Present each pair in both orders to counter position bias
- Strip comments and normalize formatting before presenting to judge
- Use a different model as judge than as agent (e.g., Opus judges Sonnet's work)
- Three pairs per task: human-vs-AI, human-vs-AI+bobbin, AI-vs-AI+bobbin

## Task Selection Criteria

Good eval tasks have:
- **2-5 files changed** (cross-file = bobbin's sweet spot)
- **20-200 lines of real logic** (not bulk renames)
- **Clear commit message** usable as a prompt
- **Tests that cover the change**
- **Tests pass at parent commit** (clean starting state)

Bad tasks: dep bumps, generated code, commits needing API keys, broken parent state.

### Target Repos (MVP)

| Repo | Language | Why |
|------|----------|-----|
| `astral-sh/ruff` | Rust | Excellent tests, clear commits, cross-file linter rule fixes |
| `pallets/flask` | Python | Smaller, solid tests, approachable, good commit messages |

5 tasks per repo = 10 tasks total.

### Task Curation Script

Semi-automated: filter `git log` for commits with 2-8 files changed, 20-200 insertions, exclude noise patterns (`chore:`, `ci:`, `docs:`), then manual review.

## MVP Scope

**Build**: 10 curated tasks, runner, test scorer, diff scorer, LLM judge, markdown report generator.

**Run**: 10 tasks x 2 AI approaches x 3 attempts = 60 headless Claude runs.

**Cost estimate**: ~$50-130 (Sonnet runs + Opus judge calls).

**Output**: Markdown report with summary table:

```
| Metric              | Without Bobbin | With Bobbin | Delta |
|---------------------|:-:|:-:|:-:|
| Test Pass Rate      | 40% | 70% | +30% |
| Avg Tool Calls      | 18.3 | 12.1 | -34% |
| Avg Tokens          | 14,200 | 9,800 | -31% |
| File Precision      | 0.45 | 0.72 | +60% |
| LLM Judge Win Rate  | - | 73% | - |
```

## Implementation Sequence

1. Create `eval/` directory structure + pyproject.toml
2. Build workspace manager (clone, checkout, snapshot)
3. Build agent runner (Claude Code headless invocation with --settings toggle)
4. Build test scorer (run tests, parse results)
5. Curate 5 tasks from ruff, 5 from flask (manual, using curation script)
6. Run first end-to-end: 1 task, 1 attempt, both approaches — validate pipeline
7. Build diff scorer + retrieval quality scorer
8. Build LLM judge with pairwise prompts
9. Build aggregator + report generator
10. Full MVP run: all 10 tasks x 3 attempts x 2 approaches

## Stretch Goals (post-MVP)

- Docker containerization for reproducibility
- Parallel execution
- Charts (matplotlib bar/box plots)
- More repos + tasks (TypeScript, Go)
- CI integration (nightly runs, regression detection)
- Ablation studies (bobbin config sweeps: threshold, budget, content_mode)
- MCP-mode comparison (hook vs on-demand tool)

## Research References

- **SWE-bench**: Real GitHub issues + test validation, containerized. github.com/SWE-bench/SWE-bench
- **GitGoodBench**: Uses merge commits as ground truth (arxiv.org/html/2505.22583v1)
- **Cline-Bench**: Real git state snapshots (cline.bot/blog/cline-bench-initiative)
- **Aider benchmarks**: Exercism problems, pass rate + 2-attempt recovery (github.com/Aider-AI/aider)
- **CodeRAG-Bench**: RAG impact on code generation (code-rag-bench.github.io)
- **LLM-as-Judge**: Pairwise 85% human agreement, flip-and-draw for position bias
- **DeepEval/RAGAS**: Context relevance scoring frameworks

## Files to Reference

- `src/cli/hook.rs:537` — `inject_context_inner`: the hook injection path the eval must replicate
- `src/config.rs:233` — `HooksConfig`: threshold/budget/content_mode defaults to record per run
- `tests/common/mod.rs` — `TestProject` pattern for isolated workspaces
- `src/cli/benchmark.rs` — existing benchmark patterns for output formatting
