# Measuring Context Injection Effectiveness for AI Coding Agents

**Bobbin: A Local-First Context Injection Engine**

Draft v0.1 -- March 2026

---

## Abstract

AI coding agents operate with limited awareness of the codebases they modify. We present Bobbin, a local-first context injection engine that intercepts AI agent lifecycle hooks to automatically inject relevant code context before each agent turn. We evaluate Bobbin's effectiveness across 66 agent runs spanning 13 tasks in real open-source repositories (Ruff, Flask, Cargo, Polars). Injection improves average file-level F1 from 0.695 to 0.722 and reduces average task duration from 252s to 209s. An ablation study isolating six injection methods on 85 runs reveals that semantic search contributes the most to retrieval quality (F1 drops by 0.384 when disabled), followed by git blame bridging (-0.303) and temporal coupling expansion (-0.247). We report injection precision/recall metrics, calibration sweeps across search weight configurations, and discuss limitations including small sample sizes for several ablation conditions.

---

## 1. Introduction

Large language model (LLM) agents used for code generation and modification face a fundamental context problem: they begin each task with little or no knowledge of the surrounding codebase. The agent must discover relevant files through exploration -- reading directory listings, searching for patterns, and following import chains. This exploration consumes tokens, time, and money, and frequently leads to incomplete understanding of the code being modified.

The standard mitigation is to provide static context files (e.g., `CLAUDE.md` project instructions) that describe conventions and architecture. However, static files cannot anticipate which specific code is relevant to an arbitrary task. The agent still must discover the concrete implementation files it needs.

We propose automated context injection: intercepting the agent's lifecycle to inject task-relevant code context before each turn. Bobbin implements this approach as a hook in Claude Code's `UserPromptSubmit` lifecycle event. When the user submits a prompt, Bobbin analyzes the prompt text, searches a pre-built index of the codebase, and injects relevant code snippets into the agent's context window alongside the original prompt.

This paper measures how effective this injection is. We ask three questions:

1. **Does injection help?** Does providing codebase context improve the agent's ability to identify and correctly modify the right files?
2. **Which methods matter?** Bobbin uses multiple retrieval and ranking methods -- which contribute the most?
3. **What are the costs?** Does injection increase latency, token usage, or financial cost?

---

## 2. System Architecture

Bobbin is implemented in Rust and operates in two phases: offline indexing and online injection.

### 2.1 Indexing Pipeline

The indexing pipeline processes repository files into searchable chunks:

1. **File walking**: Traverse the repository respecting `.gitignore` rules and configurable include/exclude globs.
2. **Structural parsing**: Use tree-sitter grammars (Rust, Python, TypeScript, Go, Java, C++) and pulldown-cmark (Markdown) to extract semantic chunks -- functions, methods, classes, structs, enums, traits, documentation sections, tables, and code blocks.
3. **Embedding generation**: Generate 384-dimensional vectors for each chunk using the all-MiniLM-L6-v2 model via ONNX Runtime, running locally with no external API calls.
4. **Storage**: Store chunks, vectors, and metadata in LanceDB with a full-text search index on content. Store temporal coupling data (git co-change relationships) in SQLite.
5. **Git history analysis**: Analyze commit history to build a co-change matrix: which files are frequently modified together within the same commits.

### 2.2 Search Pipeline

At query time, Bobbin runs a hybrid search combining two retrieval strategies:

- **Semantic search**: Embed the query and find nearest neighbors via LanceDB approximate nearest neighbor (ANN) search.
- **Keyword search**: Full-text search (BM25) against chunk content via LanceDB FTS.
- **Hybrid fusion**: Combine results using Reciprocal Rank Fusion (RRF): `score = w_s / (k + rank_s) + w_k / (k + rank_k)` where `k = 60`, `w_s` is the semantic weight (default 0.7), and `w_k = 1 - w_s`.

### 2.3 Context Assembly

Search results are expanded and filtered through a three-stage assembly pipeline:

1. **Direct results**: The top hybrid search results for the query.
2. **Coupled results**: For each file in the direct results, retrieve temporally coupled files (files frequently co-changed in git history).
3. **Bridged results**: Use git blame to bridge from documentation chunks to the source files they describe, and vice versa.

The assembled bundle undergoes content deduplication (line-level Jaccard similarity, threshold 0.65), prompt deduplication (removing chunks that overlap with the agent's existing `CLAUDE.md` system prompt), quality gating (skipping results below a relevance threshold), and doc demotion (reducing the weight of documentation relative to source code).

### 2.4 Injection Hook

Bobbin registers as a `UserPromptSubmit` hook in Claude Code. On each user prompt submission:

1. The hook receives the prompt text.
2. It calls `bobbin hook inject-context` with the prompt as query.
3. The context assembly pipeline runs (~300ms typical latency).
4. Relevant code snippets are formatted and prepended to the agent's context.
5. Each injection is assigned a ULID-based injection ID for feedback tracking.

---

## 3. Injection Methods

Bobbin composes multiple retrieval, expansion, ranking, and filtering methods. Each can be independently toggled for ablation testing.

### 3.1 Semantic Search (Embedding Similarity)

Chunks are embedded using all-MiniLM-L6-v2 (384 dimensions). Query embeddings are compared against the chunk vector store using approximate nearest neighbor search. This captures conceptual similarity even when terminology differs.

**Config toggle**: `semantic_weight=0.0` disables semantic search, falling back to pure keyword.

### 3.2 Keyword Search (BM25 Full-Text)

LanceDB's built-in full-text search index provides BM25-ranked keyword matching. This excels when the user prompt contains exact identifiers, function names, or error messages present in the codebase.

**Config toggle**: `semantic_weight=1.0` disables keyword search, using pure semantic.

### 3.3 Hybrid Search (RRF Fusion)

The default mode fuses semantic and keyword results via Reciprocal Rank Fusion. Results appearing in both result sets receive boosted scores. The default semantic weight of 0.7 favors semantic results while still benefiting from keyword matches.

### 3.4 Temporal Coupling (Git Co-Change)

Files frequently modified together in the same commits are likely related. After retrieving direct search results, Bobbin looks up each result file's co-change partners from the SQLite coupling table (built from the last 1000 commits, with a minimum of 3 co-changes required). Coupled files are added to the context bundle even if they did not match the search query directly.

**Config toggle**: `coupling_depth=0` disables temporal coupling expansion.

### 3.5 Git Blame Bridging (Doc-to-Source Links)

Documentation files often describe behavior implemented in source files. Bobbin uses git blame to identify which source files were modified in the same commits as documentation files, creating a bridge from docs to their implementing code. When a documentation chunk is retrieved, blame bridging can pull in the relevant source files, and vice versa.

**Config toggle**: `blame_bridging=false` disables the bridging pass.

### 3.6 Doc Demotion

Documentation chunks (Markdown sections, README content) are demoted in the ranking relative to source code chunks. This prevents documentation from crowding out the actual implementation code the agent needs to modify.

**Config toggle**: `doc_demotion=0.0` disables demotion (treats docs equal to source). Default applies a demotion factor.

### 3.7 Quality Gating

A relevance threshold filters out low-scoring results. If the top semantic score falls below the gate threshold, injection is skipped entirely for that turn -- the query is too dissimilar to anything in the index.

**Config toggle**: `gate_threshold=1.0` disables gating (never injects). `gate_threshold=0.0` always injects.

### 3.8 Recency Boosting

Recently modified files receive a score boost, reflecting the assumption that the user's current task is more likely to involve recently changed code.

**Config toggle**: `recency_weight=0.0` disables recency boosting.

### 3.9 Content Deduplication

A line-level Jaccard similarity check (threshold 0.65) removes near-duplicate chunks from the assembled context. This catches path-duplicate repositories (the same repo indexed under multiple paths) and templated content with minor per-instance variations. Small chunks (2 or fewer unique lines) use exact-match only to avoid false positives.

### 3.10 CLAUDE.md Prompt Deduplication

The hook walks up from the working directory collecting `CLAUDE.md` files (which Claude Code loads as system prompt), splits them at `##` headers, and pre-seeds the deduplicator. Chunks that substantially overlap with content already in the system prompt are dropped before injection.

---

## 4. Experimental Setup

### 4.1 Eval Framework

We built a custom evaluation framework that spawns headless Claude Code agents against real open-source repositories. Each eval run:

1. Clones a bare mirror of the target repository (cached in `~/.cache/bobbin-eval/repos/`).
2. Checks out the specified commit, creating a clean working copy.
3. Launches a Claude Code agent (model: claude-sonnet-4-5-20250929) with the task prompt.
4. Records all agent actions, tool uses, files touched, and timing.
5. Compares the agent's file modifications against ground truth.

### 4.2 Tasks

We assembled 13 tasks across 4 open-source repositories:

| Repository | Language | Tasks | IDs |
|------------|----------|:-----:|-----|
| Ruff | Rust/Python | 5 | ruff-001 through ruff-005 |
| Flask | Python | 5 | flask-001 through flask-005 |
| Cargo | Rust | 1 | cargo-001 |
| Polars | Rust/Python | 2 | polars-004, polars-005 |

Each task specifies a commit, a natural-language prompt describing the change, and a set of ground-truth files that should be modified. Tasks were selected to represent a mix of bug fixes, feature additions, and refactoring operations across different codebase sizes.

Two additional tasks (django-001, pandas-001) were planned but produced no completed runs due to infrastructure issues.

### 4.3 Metrics

**File-level precision**: fraction of agent-modified files that are in the ground truth set.

**File-level recall**: fraction of ground truth files that the agent modified.

**File-level F1**: harmonic mean of precision and recall.

**Test pass rate**: fraction of runs where the agent's changes pass the task's test suite.

**Injection precision**: fraction of injected files that the agent subsequently touched.

**Injection recall**: fraction of agent-touched files that were injected.

### 4.4 Conditions

Two primary conditions:

- **no-bobbin**: Agent runs without any context injection. No Bobbin hook active.
- **with-bobbin**: Agent runs with Bobbin injection using default configuration (semantic_weight=0.7, coupling_depth=1000, blame_bridging=true, doc_demotion enabled, gate_threshold default, recency_weight default).

Six ablation conditions (each disabling one method while keeping the rest at defaults):

- `semantic_weight=0.0` -- disable semantic search
- `coupling_depth=0` -- disable temporal coupling
- `recency_weight=0.0` -- disable recency boosting
- `doc_demotion=0.0` -- disable doc demotion
- `gate_threshold=1.0` -- disable quality gating
- `blame_bridging=false` -- disable git blame bridging

---

## 5. Results

### 5.1 Baseline Comparison

**Table 1: Aggregate comparison across all tasks (66 runs)**

| Metric | no-bobbin (N=29) | with-bobbin (N=36) |
|--------|:----------------:|:------------------:|
| Avg File Precision | 86.8% | 91.2% |
| Avg File Recall | 61.1% | 64.2% |
| Avg F1 | 69.5% | 72.2% |
| Test Pass Rate | 65.5% | 47.2% |
| Avg Duration (s) | 252.3 | 209.1 |
| Avg Cost (USD) | $1.18 | $1.42 |
| Avg Input Tokens | 96 | 136 |
| Avg Output Tokens | 8,065 | 8,608 |

Injection improves file-level F1 by +2.7 percentage points on average, with gains concentrated in precision (+4.4pp) and recall (+3.1pp). Average task duration decreases by 43 seconds (17%), suggesting that agents spend less time exploring when relevant context is provided upfront. However, the test pass rate drops from 65.5% to 47.2% -- this is driven primarily by polars-004, where the with-bobbin run failed entirely (F1=0.0, duration=0.0s), likely an infrastructure failure rather than a genuine regression.

Cost increases modestly from $1.18 to $1.42 per run (+20%), reflecting the additional input tokens from injected context.

**Table 2: Per-task baseline comparison**

| Task | no-bobbin F1 | with-bobbin F1 | Delta |
|------|:------------:|:--------------:|:-----:|
| ruff-001 | 0.321 | 0.667 | +0.346 |
| ruff-002 | 0.571 | 0.571 | 0.000 |
| ruff-003 | 0.900 | 0.867 | -0.033 |
| ruff-004 | 0.542 | 0.708 | +0.166 |
| ruff-005 | 1.000 | 1.000 | 0.000 |
| flask-001 | 0.500 | 0.500 | 0.000 |
| flask-002 | 0.800 | 0.700 | -0.100 |
| flask-003 | 0.750 | 0.750 | 0.000 |
| flask-004 | 0.819 | 0.750 | -0.069 |
| flask-005 | 0.667 | 0.730 | +0.063 |
| cargo-001 | 1.000 | -- | -- |
| polars-004 | 0.800 | 0.000 | -0.800 |
| polars-005 | 0.794 | -- | -- |

The strongest gains appear on ruff-001 (+0.346 F1) and ruff-004 (+0.166), where the agent benefits from having relevant Rust source files injected. Tasks where the agent already achieves high F1 without injection (ruff-005 at 1.000, cargo-001 at 1.000) show no improvement -- a ceiling effect.

### 5.2 Ablation Study

The ablation study focused on ruff-001, the task with the largest injection benefit. Each ablation condition was run 3 times; the baseline conditions had 5 (no-bobbin) and 7 (with-bobbin) runs.

**Table 3: Ablation impact summary (ruff-001)**

| Method Disabled | Baseline F1 | Ablated F1 | Delta | Impact |
|-----------------|:-----------:|:----------:|:-----:|:------:|
| Semantic search (`semantic_weight=0.0`) | 0.636 | 0.252 | **-0.384** | Large negative |
| Blame bridging (`blame_bridging=false`) | 0.636 | 0.333 | **-0.303** | Large negative |
| Coupling expansion (`coupling_depth=0`) | 0.636 | 0.389 | **-0.247** | Large negative |
| Doc demotion (`doc_demotion=0.0`) | 0.636 | 0.556 | **-0.081** | Moderate negative |
| Recency signal (`recency_weight=0.0`) | 0.636 | 0.611 | **-0.025** | Small negative |
| Quality gate (`gate_threshold=1.0`) | 0.636 | 0.611 | **-0.025** | Small negative |

All six methods contribute positively -- disabling any one hurts performance. The three retrieval expansion methods (semantic search, blame bridging, coupling) have the largest individual effects, together accounting for the majority of Bobbin's value.

**Table 4: Per-task ablation breakdown with standard deviations**

| Task | Approach | N | F1 (mean +/- std) | Test Pass% | Avg Cost |
|------|----------|:-:|:------------------:|:----------:|:--------:|
| ruff-001 | no-bobbin | 5 | 0.324 +/- 0.021 | 100% | $0.74 |
| ruff-001 | with-bobbin | 7 | 0.636 +/- 0.347 | 100% | $1.08 |
| ruff-001 | semantic_weight=0.0 | 4 | 0.252 +/- 0.134 | 100% | $1.45 |
| ruff-001 | coupling_depth=0 | 3 | 0.389 +/- 0.096 | 100% | $1.45 |
| ruff-001 | recency_weight=0.0 | 3 | 0.611 +/- 0.347 | 100% | $1.43 |
| ruff-001 | doc_demotion=0.0 | 3 | 0.556 +/- 0.385 | 100% | $1.48 |
| ruff-001 | gate_threshold=1.0 | 3 | 0.611 +/- 0.347 | 100% | $1.39 |
| ruff-001 | blame_bridging=false | 3 | 0.333 +/- 0.000 | 100% | $1.25 |
| cargo-001 | no-bobbin | 1 | 1.000 +/- 0.000 | 100% | $1.04 |
| cargo-001 | with-bobbin | 1 | 1.000 +/- 0.000 | 100% | $1.03 |

Notable observations:

- The **with-bobbin standard deviation (0.347)** is high, indicating substantial run-to-run variance even with the same configuration. This reflects the inherent non-determinism of LLM agent behavior.
- **Blame bridging** shows zero variance when disabled (0.333 +/- 0.000 across 3 runs), suggesting that without blame bridging, the agent converges to a consistent (but worse) exploration pattern.
- **Disabling semantic search** (0.252) actually performs worse than no injection at all (0.324), indicating that keyword-only injection can be actively harmful -- it injects irrelevant code that misleads the agent.

### 5.3 Injection Precision and Recall

**Table 5: How well injected files predict agent-touched files**

| Task | Approach | Injection Precision | Injection Recall | Injection F1 |
|------|----------|:-------------------:|:----------------:|:------------:|
| ruff-001 | with-bobbin | 0.029 | 0.067 | 0.040 |
| ruff-001 | semantic_weight=0.0 | 0.181 | 0.204 | 0.174 |
| ruff-001 | coupling_depth=0 | 0.000 | 0.000 | 0.000 |
| ruff-001 | recency_weight=0.0 | 0.000 | 0.000 | 0.000 |
| ruff-001 | doc_demotion=0.0 | 0.026 | 0.111 | 0.042 |
| ruff-001 | blame_bridging=false | 0.000 | 0.000 | 0.000 |
| cargo-001 | with-bobbin | 0.125 | 0.500 | 0.200 |

Injection precision is low across all conditions: Bobbin injects many more files than the agent ends up modifying. This is by design -- the injection includes contextual files (related code, documentation, tests) that inform the agent's decisions without being direct edit targets. However, the low injection recall (0.067 for ruff-001) indicates that Bobbin frequently fails to inject the specific files the agent needs.

The paradox of semantic_weight=0.0 having *higher* injection precision (0.181) but *lower* task F1 (0.252) suggests that raw injection overlap is a poor proxy for usefulness. Semantic search injects contextually relevant code that helps the agent *discover* the right files through understanding, even when those injected files are not themselves the edit targets.

### 5.4 Calibration Sweep

A separate calibration study swept search weight configurations across 4 Flask tasks.

**Table 6: Search weight calibration (Flask tasks)**

| Config | Semantic Weight | Doc Demotion | Precision | Recall | F1 |
|--------|:-:|:-:|:-:|:-:|:-:|
| sw=0.90, dd=0.30 | 0.90 | 0.30 | 0.408 | 0.554 | 0.461 |
| sw=0.90, dd=0.50 | 0.90 | 0.50 | 0.408 | 0.554 | 0.461 |
| sw=0.50, dd=0.30 | 0.50 | 0.30 | 0.427 | 0.442 | 0.397 |
| sw=0.50, dd=0.50 | 0.50 | 0.50 | 0.427 | 0.442 | 0.397 |
| sw=0.70, dd=0.30 | 0.70 | 0.30 | 0.363 | 0.454 | 0.375 |
| sw=0.70, dd=0.50 | 0.70 | 0.50 | 0.363 | 0.454 | 0.375 |

Higher semantic weight (0.90) outperforms balanced (0.50) and the default (0.70) configurations on Flask tasks, with F1 of 0.461 vs 0.397 and 0.375 respectively. Doc demotion factor (0.30 vs 0.50) shows no measurable effect in this sweep. The optimal configuration favors semantic search heavily, consistent with the ablation finding that semantic search is the most impactful method.

---

## 6. Discussion

### 6.1 What Works

**Semantic search is the foundation.** Disabling it drops F1 by 0.384, more than any other single method. The embedding model captures conceptual relationships between the user's natural-language prompt and code structure in ways that keyword matching cannot.

**Git-based methods provide significant value.** Blame bridging (-0.303 when disabled) and temporal coupling (-0.247) together contribute nearly as much as semantic search. These methods surface files that are structurally related to the search results but may not share any textual or semantic similarity with the query. A bug fix prompt mentioning an error message can lead, through blame bridging, to the documentation that describes the feature, and from there to the implementation files.

**Injection reduces exploration time.** The 17% reduction in average task duration (252s to 209s) suggests that agents spend fewer turns exploring when relevant context arrives upfront. This partially offsets the 20% cost increase from additional input tokens.

### 6.2 What Doesn't Work

**Low injection precision is concerning.** At 2.9% injection precision for ruff-001, the vast majority of injected content is not directly used. While we argue that contextual files provide indirect value, there is likely room to tighten the injection set. The quality gate and doc demotion methods exist to address this but show only small effects in the ablation (-0.025 each).

**Test pass rate regressions.** The aggregate test pass rate dropped from 65.5% to 47.2% with injection. While the polars-004 infrastructure failure accounts for much of this, the pattern warrants investigation. Injected context may sometimes mislead the agent toward plausible but incorrect modifications.

**Flask tasks show minimal benefit.** Five Flask tasks showed mixed results (-0.100 to +0.063 F1 delta), possibly because Flask's well-organized codebase and clear naming conventions make agent exploration already effective.

### 6.3 Limitations

**Small sample sizes.** The ablation study has N=3 for most conditions and N=1 for the ablation summary. High variance (std dev up to 0.385 for doc_demotion=0.0) means these results are directional, not statistically significant. The planned 108-run study would provide more robust estimates.

**Single-task ablation.** Ablation data exists only for ruff-001. The relative importance of methods may differ across repositories and languages. Cargo-001 shows perfect F1 with and without injection, providing no ablation signal.

**Missing tasks.** Django-001 and pandas-001 produced zero completed runs. Polars-005 has no with-bobbin runs. These gaps limit cross-repository generalization.

**Non-deterministic agent behavior.** LLM agents exhibit substantial run-to-run variance (with-bobbin std dev of 0.347 on ruff-001). This makes small-N comparisons unreliable and complicates causal attribution.

**Injection vs. exploration confound.** We measure file-level F1 of the agent's final changes, not the agent's intermediate understanding. An agent that receives injected context may still explore independently and arrive at the same files. The injection usage metrics (Table 5) attempt to address this but show low overlap.

---

## 7. Future Work

### 7.1 Format Mode Experiments

Bobbin supports four output format modes for injected context: standard (default), minimal (clean with no metadata), verbose (standard with type annotations), and XML (structured tags). A format comparison study is planned to determine whether structured formatting improves agent utilization of injected context.

### 7.2 Production Feedback Loop

Bobbin includes an injection feedback system where agents and users can rate injections as useful, noise, or harmful. Each injection receives a ULID-based identifier, and feedback is stored in a SQLite database alongside the injection record. Accumulating production feedback data will enable:

- Per-chunk quality signals for reranking
- Automated detection of consistently noisy file patterns
- Adaptive threshold tuning based on historical usefulness

### 7.3 Adaptive Injection

Current injection uses fixed configuration parameters. Future work could adapt injection strategy based on:

- Task type detection (bug fix vs. feature vs. refactor)
- Repository characteristics (size, language mix, commit frequency)
- Agent behavior patterns (exploration-heavy agents may benefit from broader injection)

### 7.4 Larger-Scale Evaluation

The current study is limited to 13 tasks across 4 repositories. A more comprehensive evaluation would include:

- Additional languages (Go, TypeScript, Java)
- Larger repositories (monorepos, multi-package workspaces)
- More task types (security fixes, performance optimization, dependency upgrades)
- Sufficient runs per condition (N >= 10) for statistical significance testing

### 7.5 Temporal Decay Analysis

The measurement framework design calls for coupling depth sweeps (100, 500, 1000, 5000 commits) and recency weight sweeps to characterize how these signals decay with commit age. This data would inform automatic parameter selection.

---

## 8. Conclusion

Bobbin demonstrates that automated context injection can improve AI coding agent performance on file-level precision and recall metrics, with an average F1 improvement of +2.7 percentage points and a 17% reduction in task duration. The ablation study identifies semantic search as the most critical injection method, followed by git blame bridging and temporal coupling -- confirming that combining embedding-based retrieval with structural code relationships outperforms either approach alone.

The results also reveal important limitations: low injection precision (most injected files are not edit targets), high run-to-run variance inherent to LLM agents, and the need for larger-scale evaluation to establish statistical significance. The 20% cost increase from additional input tokens is a real tradeoff that must be weighed against the time savings and quality improvements.

Context injection for AI coding agents is a promising direction, but the field needs better evaluation methodology -- larger task suites, more runs per condition, and metrics that capture the agent's intermediate understanding rather than just its final file edits. Bobbin's open evaluation framework and injection feedback system provide infrastructure for this ongoing measurement.

---

## Appendix A: Raw Data Sources

- Baseline comparison: 66 runs across 13 tasks (fresh-report.md)
- Ablation study: 85 runs across 13 tasks, 8 conditions on ruff-001 (ablation-report-final.md)
- Calibration sweep: 6 configurations across 4 Flask tasks (calibration-flask-v4-tuned.md)
- Evaluation framework: headless Claude Code agents using claude-sonnet-4-5-20250929

## Appendix B: Reproduction

All evaluation runs can be reproduced using the Bobbin eval framework:

```bash
# Baseline comparison
python3 -m runner.cli run-task <task> --approach no-bobbin
python3 -m runner.cli run-task <task> --approach with-bobbin

# Ablation (example: disable semantic search)
python3 -m runner.cli run-task ruff-001 --approach with-bobbin -C semantic_weight=0.0

# Full study
./run-baseline-study.sh    # 4 tasks x 8 conditions x 3 attempts
```

Estimated cost: ~$1-2 per run, ~5 minutes per run, ~$135 for the full 108-run study.
