# Decomposing Context Injection for AI Coding Agents

**Which retrieval methods carry the weight, and what it takes to measure them**

Draft v0.2 -- August 2026

---

## Abstract

AI coding agents operate with limited awareness of the codebases they modify, and a growing number of systems address this by injecting retrieved context into the agent's loop. Such systems are typically evaluated end-to-end, which establishes that injection helps without establishing *what is doing the work*. We argue the useful unit of evaluation is the **decomposition**: a context injection engine is a composition of separable retrieval, expansion, ranking and filtering methods, and each can be removed and measured. We present Bobbin, a local-first injection engine that composes six such methods, and a removal-based evaluation harness that runs headless coding agents against real open-source repositories and scores their file-level edits against ground truth.

We report a pilot decomposition on 85 runs. The three *retrieval-expansion* methods separate clearly from the three *filtering* methods: disabling semantic search costs 0.384 F1 (95% CI [-0.721, -0.047]), git blame bridging 0.303 ([-0.624, +0.018]) and temporal coupling expansion 0.247 ([-0.578, +0.084]), while recency boosting, quality gating and doc demotion have point estimates at or below 0.080 with intervals spanning most of the metric's range.

**These are directional results, not established magnitudes, and we state the limits up front rather than in the discussion.** With 3-4 runs per arm and a baseline standard deviation of 0.347 — driven by the run-to-run non-determinism of the agents themselves — no arm survives Holm-Bonferroni correction across the six comparisons, and the separately measured aggregate injection effect (F1 0.695 to 0.722 over 66 runs) is smaller than the noise.

We also report a confound that our own design cannot resolve and that we believe generalises to this class of evaluation. One ablation arm turned out to disable injection entirely while still leaving the agent a search tool it was prompted to use. Comparing it against the two baselines decomposes the total effect: +0.287 F1 is present as soon as the agent has a retrieval tool and is told to use it, and only +0.025 is added by automatic injection on top. **On these point estimates, roughly 92% of what looks like an injection effect is a tool-availability effect.** Both figures are far from significant, which is precisely the problem: the study cannot separate the mechanism it is about from the mechanism it accidentally controls for.

We therefore report the decomposition as a ranking with intervals, decline to claim the three filtering effects at all, and give the power analysis: 14, 22 and 32 runs per arm would establish the three retrieval-expansion effects at 80% power, while the filtering effects would need 300-3,000. Our contribution is the removal-based methodology, an open harness that implements it, a preliminary ranking honest about which of its rows are load-bearing, and a worked account of how an agent evaluation of apparently reasonable size fails to support its own conclusions.

---

## 1. Introduction

Large language model (LLM) agents used for code generation and modification face a fundamental context problem: they begin each task with little or no knowledge of the surrounding codebase. The agent must discover relevant files through exploration -- reading directory listings, searching for patterns, and following import chains. This exploration consumes tokens, time, and money, and frequently leads to incomplete understanding of the code being modified.

The standard mitigation is to provide static context files (e.g., `CLAUDE.md` project instructions) that describe conventions and architecture. However, static files cannot anticipate which specific code is relevant to an arbitrary task. The agent still must discover the concrete implementation files it needs.

We propose automated context injection: intercepting the agent's lifecycle to inject task-relevant code context before each turn. Bobbin implements this approach as a hook in Claude Code's `UserPromptSubmit` lifecycle event. When the user submits a prompt, Bobbin analyzes the prompt text, searches a pre-built index of the codebase, and injects relevant code snippets into the agent's context window alongside the original prompt.

### 1.1 Why decomposition rather than aggregate improvement

The obvious way to evaluate such a system is end-to-end: run agents with and without injection and compare. We did this, and it illustrates why it is the wrong primary question. Across 66 runs the aggregate file-level F1 moves from 0.695 to 0.722 — a +0.027 difference against a per-run standard deviation of 0.347. An aggregate of that shape can be reported, but it cannot be *defended*: the first question a reader should ask is whether it is distinguishable from noise, and the honest answer is no.

More importantly, the aggregate answers a question nobody building these systems actually has. "Injection helps" is already the premise of every system in this space. The open question is **which of the composed methods carries the weight** — because that is what determines where engineering effort goes, what a smaller implementation can safely omit, and which mechanisms deserve theoretical attention.

That question has a natural experimental form. A context injection engine is not a monolith; it is a pipeline of methods that can each be independently disabled. Removing one and re-running measures its contribution directly, at an effect size an order of magnitude larger than the aggregate. Bobbin's six methods produce removal effects up to 0.384 F1, against an aggregate effect of 0.027.

We therefore ask:

1. **Which methods carry the weight?** With each of six injection methods removed in turn, how much retrieval quality is lost?
2. **What does it take to measure that?** Given the run-to-run variance of LLM agents, how many runs per condition does a removal study actually need, and which of our own results clear that bar?
3. **What are the costs?** Does injection increase latency, token usage, or financial cost?

Question 2 is not throat-clearing. It is the finding we most want to transfer: agent-based evaluation is noisy enough that a 3-run-per-condition ablation — a design that looks reasonable, and that we ran — cannot support the claims it appears to support. We quantify this in §5.3 rather than conceding it in a limitations section.

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

| Repository | Language | Tasks | IDs | Status |
|------------|----------|:-----:|-----|--------|
| Ruff | Rust/Python | 5 | ruff-001 through ruff-005 | Reported |
| Cargo | Rust | 1 | cargo-001 | Reported |
| Polars | Rust/Python | 2 | polars-004, polars-005 | Reported |
| Flask | Python | 5 | flask-001 through flask-005 | **Withdrawn — see below** |

Each task specifies a commit, a natural-language prompt describing the change, and a set of ground-truth files that should be modified. Tasks were selected to represent a mix of bug fixes, feature additions, and refactoring operations across different codebase sizes.

Two additional tasks (django-001, pandas-001) were planned but produced no completed runs due to infrastructure issues.

**The five Flask tasks are withdrawn and are not used in any claim in this paper.** Across 47 runs they returned a 0% test pass rate on *both* the with-bobbin and no-bobbin arms. A 0% pass rate that is invariant to the treatment is a property of the harness, not of the system under test: the root cause was in the tasks' own `setup_command` and `test_command` definitions, so no agent could have passed regardless of what context it received. They were quarantined on 2026-02-15.

We report this at length because an earlier draft of this paper did not. That draft included the Flask tasks in its aggregate, built its configuration-calibration sweep entirely on them, and interpreted their flat F1 deltas as evidence that "Flask's well-organized codebase and clear naming conventions make agent exploration already effective" — an affirmative conclusion drawn from a broken fixture. The failure mode is worth naming for others building agent evaluations: **a task whose pass rate is invariant across arms should be treated as a suspected harness fault until proven otherwise**, because a genuinely null result and a broken fixture look identical in the aggregate and only the former is a finding. We now excluded Flask everywhere, which withdraws the calibration sweep entirely (§5.5).

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
- `gate_threshold=1.0` -- **intended** as "disable quality gating"; see below
- `blame_bridging=false` -- disable git blame bridging

**The `gate_threshold=1.0` condition does not do what its label says.** Per §3.7, a gate threshold of 1.0 means the relevance bar can never be cleared, so injection is skipped entirely rather than injected-without-gating. We discovered this from the run artifacts after the fact, and we report the condition under its true behaviour throughout: it is a third baseline, not an ablation. §5.2 treats it as such, and it turns out to be the most informative condition in the study. A genuine gating ablation would set the threshold to 0.0 and remains unrun.

---

## 5. Results

We report the decomposition first (§5.1-5.3), because it is the contribution; the aggregate comparison (§5.4) follows as supporting context. §5.3 is not a limitations section — it is the result that determines how much of §5.1 can be claimed, and it should be read alongside it.

### 5.1 Ablation: which methods carry the weight

The ablation study focused on ruff-001, the task with the largest injection benefit. Each ablation condition was run 3 times; the baseline conditions had 5 (no-bobbin) and 7 (with-bobbin) runs.

**Table 1: Removal effects on ruff-001, with confidence intervals**

| Method disabled | Ablated F1 | Δ vs baseline | 95% CI of Δ | N |
|-----------------|:----------:|:-------------:|-------------|:-:|
| Semantic search (`semantic_weight=0.0`) | 0.252 | **−0.384** | [−0.721, −0.047] | 4 |
| Blame bridging (`blame_bridging=false`) | 0.333 | **−0.303** | [−0.624, +0.018] | 3 |
| Coupling expansion (`coupling_depth=0`) | 0.389 | **−0.247** | [−0.578, +0.084] | 3 |
| Doc demotion (`doc_demotion=0.0`) | 0.556 | −0.080 | [−0.839, +0.679] | 3 |
| Recency signal (`recency_weight=0.0`) | 0.611 | −0.025 | [−0.700, +0.650] | 3 |
| Quality gate (`gate_threshold=1.0`) | 0.611 | −0.025 | [−0.700, +0.650] | 3 |

Baseline: with-bobbin, F1 0.636 ± 0.347, N=7. Welch's t-test throughout — the arm standard deviations range from 0.000 to 0.385, so a pooled-variance test is not applicable. The `gate_threshold=1.0` row is retained for completeness but does not mean what its label suggests; see §5.2.

**The structure of the table is the finding, not the individual magnitudes.** The three retrieval-expansion methods — semantic search, blame bridging, coupling expansion — separate as a group from the three filtering methods by roughly an order of magnitude (0.247-0.384 against 0.025-0.080). That grouping is what we would expect a practitioner to take away: **effort spent on what gets retrieved dominates effort spent on what gets filtered out afterwards**, and a smaller implementation can reasonably omit the filtering stages before it omits any retrieval stage.

Two observations survive as directional signals rather than measured quantities:

- **Disabling semantic search (0.252) scores below no injection at all (0.324).** Keyword-only injection appears to be actively harmful — it injects lexically similar but conceptually irrelevant code. This is the study's largest single effect and the only arm nominally significant before correction.
- **Blame bridging shows zero variance when disabled** (0.333 ± 0.000 across 3 runs). A plausible reading is that without bridging the agent converges on one consistent worse exploration path. With N=3 an alternative reading is coincidence, and we cannot distinguish them.

We do **not** claim the three filtering effects. Their confidence intervals span most of the achievable range of the metric; the study contains essentially no information about them. §5.3 gives the runs required to change that.

### 5.2 An arm that disabled injection entirely, and what it reveals

`gate_threshold=1.0` was intended as "disable quality gating". It does not do that. Per §3.7 a gate threshold of 1.0 means the relevance bar can never be cleared, so **injection is skipped entirely**. The run artifacts confirm it: all three runs of this arm record an injection invocation that returned no chunks, while every other arm records 17-20 chunks.

This is a design error, and it produced the most informative comparison in the study. The arm is not an ablation — it is a **third baseline**, in which the agent receives the prompt block telling it Bobbin is installed, has the `bobbin` CLI available, and uses it (3-5 `bobbin search` invocations per run, plus `related` and `refs`), but receives no automatic injection whatsoever.

That is exactly the control the study otherwise lacks, and it splits the headline effect in two:

**Table 2: Decomposing the injection effect**

| Contrast | What differs | Δ F1 | p | 95% CI |
|----------|--------------|-----:|---:|--------|
| no-bobbin → gate-disabled arm | Tool available and prompted; no injection | **+0.287** | 0.288 | [−0.572, +1.146] |
| gate-disabled arm → with-bobbin | Automatic injection added on top | **+0.025** | 0.922 | [−0.650, +0.700] |
| no-bobbin → with-bobbin | Both | +0.312 | 0.055 | [−0.009, +0.633] |

**Roughly 92% of the apparent injection effect is present before any automatic injection occurs.** Giving the agent a retrieval tool and telling it to use one accounts for +0.287 of the +0.312 total; the automatic injection mechanism this paper is about accounts for +0.025.

We want to be precise about the epistemic status of this. Neither component is statistically significant, the intervals are enormous, and N is 3, 5 and 7. **This is not a finding that automatic injection does not work.** It is a demonstration that our design — and, we suspect, the natural design for this problem, which is what makes it worth reporting — **confounds the mechanism under study with the availability of a tool the prompt advertises.** A with-bobbin arm differs from a no-bobbin arm in two ways at once, and attributing the difference to injection assumes the other way is negligible. Our one accidental data point suggests it is not.

The remedy is a fourth arm, run at the N given in §5.3: Bobbin installed and prompted, hook removed. We report this rather than quietly re-running because the confound is a methodological point for anyone evaluating agent context systems, not just a defect in our numbers.

### 5.3 Statistical power: what the study can and cannot support

**Table 3: Multiple-comparison correction across the six arms (Holm-Bonferroni, α = 0.05)**

| Arm | p | Holm threshold | Outcome |
|-----|---:|---:|---|
| `semantic_weight=0.0` | 0.030 | 0.0083 | retain null |
| `blame_bridging=false` | 0.060 | 0.0100 | retain null |
| `coupling_depth=0` | 0.123 | 0.0125 | retain null |
| `doc_demotion=0.0` | 0.774 | 0.0167 | retain null |
| `recency_weight=0.0` | 0.922 | 0.0250 | retain null |
| `gate_threshold=1.0` | 0.922 | 0.0500 | retain null |

**After correction, no arm is significant.** The single nominally significant result — semantic search at p = 0.030 — misses its corrected threshold of 0.0083 by nearly fourfold.

Correction is mandatory here rather than conservative. The paper's framing *is* a six-way comparison: asking which of six methods carries the weight and then reporting the winner of six uncorrected tests is the exact error the correction exists to prevent. Five of the six confidence intervals in Table 1 cross zero.

**Table 4: Runs per arm required for 80% power at the observed effect (α = 0.05)**

| Arm | Observed \|Δ\| | Cohen's *d* | N needed | N had |
|-----|---:|---:|---:|---:|
| `semantic_weight=0.0` | 0.384 | 1.11 | **14** | 4 |
| `blame_bridging=false` | 0.303 | 0.87 | **22** | 3 |
| `coupling_depth=0` | 0.247 | 0.71 | **32** | 3 |
| `doc_demotion=0.0` | 0.080 | 0.23 | 297 | 3 |
| `recency_weight=0.0` | 0.025 | 0.07 | 3026 | 3 |
| `gate_threshold=1.0` | 0.025 | 0.07 | 3026 | 3 |

Both halves of this table are actionable:

- The three retrieval-expansion effects are **within reach**. Roughly 70 runs total, at ~$1.50 and ~5 minutes each, is about $105 and a day of wall-clock. That is a fundable follow-up, not an aspiration, and it is the experiment we recommend.
- The three filtering effects are **not worth powering**. Detecting a 0.025 F1 effect needs ~3,000 runs per arm, on the order of $9,000. The correct response is not to run them but to stop claiming them.

These N are **lower bounds**. Powering a study on the effect size observed in an underpowered pilot is optimistic, because pilots that reach significance systematically overestimate effect size.

The dominant cost is variance, and the variance is intrinsic: with-bobbin has a standard deviation of 0.347 on a metric bounded in [0, 1], from identical configuration and identical prompts. Any evaluation of LLM agents on end-task metrics inherits this, and it is why 3-run conditions — a design that looks reasonable and that we ran — cannot support method-level conclusions.

### 5.4 Aggregate Comparison

The aggregate comparison is reported second and deliberately not as a headline. It is the weaker measurement, for the reasons §5.3 gives.

**Table 5: Aggregate comparison across all tasks (66 runs)**

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

Aggregate file-level F1 differs by +2.7 percentage points, with precision +4.4pp and recall +3.1pp. Average task duration is 43 seconds (17%) lower, consistent with agents spending less time exploring when context arrives upfront. Cost is 20% higher, reflecting the additional input tokens.

**None of these differences can be tested, and we do not claim any of them.** Table 5 reports means without standard deviations or per-run values — a reporting defect in the original study design, independent of sample size. Given that the *same* configuration produces a per-run F1 standard deviation of 0.347 at task level (§5.3), a +0.027 aggregate difference is well inside the noise, and we report it only to show its magnitude relative to the removal effects in Table 1, which are up to fourteen times larger.

The test pass rate moves the *opposite* way, 65.5% to 47.2%. Most of this is polars-004, whose with-bobbin run recorded F1 = 0.0 and duration = 0.0s — an infrastructure failure, not a regression. We flag rather than explain it: a study that cannot distinguish an infrastructure failure from a quality regression in its own aggregate should not be reporting that aggregate as a result, which is precisely why §5.1 leads.

**Table 6: Per-task baseline comparison**

| Task | no-bobbin F1 | with-bobbin F1 | Delta |
|------|:------------:|:--------------:|:-----:|
| ruff-001 | 0.321 | 0.667 | +0.346 |
| ruff-002 | 0.571 | 0.571 | 0.000 |
| ruff-003 | 0.900 | 0.867 | −0.033 |
| ruff-004 | 0.542 | 0.708 | +0.166 |
| ruff-005 | 1.000 | 1.000 | 0.000 |
| cargo-001 | 1.000 | — | — |
| polars-004 | 0.800 | 0.000 | −0.800 |
| polars-005 | 0.794 | — | — |

Flask tasks are excluded (§4.2). Cells marked "—" have no completed runs in that arm, and the per-task with-bobbin counts are N=1 for ruff-003, ruff-004 and ruff-005 — single runs of a process whose standard deviation is 0.347. **Individual rows of this table should not be interpreted.** In particular the +0.346 on ruff-001 and +0.166 on ruff-004 are single-run and 7-run figures respectively against the same noise floor, and the apparent ceiling effect on ruff-005 and cargo-001 is equally consistent with those tasks being too easy to discriminate between arms.

### 5.5 Underlying per-condition data

**Table 7: Per-condition breakdown with standard deviations**

| Task | Approach | N | F1 (mean ± sd) | Test Pass% | Avg Cost |
|------|----------|:-:|:--------------:|:----------:|:--------:|
| ruff-001 | no-bobbin | 5 | 0.324 ± 0.021 | 100% | $0.74 |
| ruff-001 | with-bobbin | 7 | 0.636 ± 0.347 | 100% | $1.08 |
| ruff-001 | semantic_weight=0.0 | 4 | 0.252 ± 0.134 | 100% | $1.45 |
| ruff-001 | coupling_depth=0 | 3 | 0.389 ± 0.096 | 100% | $1.45 |
| ruff-001 | recency_weight=0.0 | 3 | 0.611 ± 0.347 | 100% | $1.43 |
| ruff-001 | doc_demotion=0.0 | 3 | 0.556 ± 0.385 | 100% | $1.48 |
| ruff-001 | gate_threshold=1.0 † | 3 | 0.611 ± 0.347 | 100% | $1.39 |
| ruff-001 | blame_bridging=false | 3 | 0.333 ± 0.000 | 100% | $1.25 |
| cargo-001 | no-bobbin | 1 | 1.000 ± 0.000 | 100% | $1.04 |
| cargo-001 | with-bobbin | 1 | 1.000 ± 0.000 | 100% | $1.03 |

† Not an ablation — injection is disabled entirely in this arm. See §5.2.

The `with-bobbin` standard deviation of 0.347 is the number that governs everything else in this paper. It is produced by identical configuration, identical prompts and identical repository state, so it is not measurement error to be reduced by better instrumentation — it is the intrinsic run-to-run variance of the agent. Every power calculation in §5.3 follows from it.

Note also that the test pass rate is 100% in every ruff-001 condition, including the ones with the worst F1. The tasks' own test suites do not discriminate between these arms at all, which is why file-level F1 rather than test outcome is the primary metric here — and a limitation, since file-level overlap with ground truth is a proxy for correctness rather than correctness itself.

### 5.6 Injection precision and recall

**Table 8: How well injected files predict agent-touched files**

| Task | Approach | Injection Precision | Injection Recall | Injection F1 |
|------|----------|:-------------------:|:----------------:|:------------:|
| ruff-001 | with-bobbin | 0.029 | 0.067 | 0.040 |
| ruff-001 | semantic_weight=0.0 | 0.181 | 0.204 | 0.174 |
| ruff-001 | coupling_depth=0 | 0.000 | 0.000 | 0.000 |
| ruff-001 | recency_weight=0.0 | 0.000 | 0.000 | 0.000 |
| ruff-001 | doc_demotion=0.0 | 0.026 | 0.111 | 0.042 |
| ruff-001 | blame_bridging=false | 0.000 | 0.000 | 0.000 |
| cargo-001 | with-bobbin | 0.125 | 0.500 | 0.200 |

Injection precision is low across all conditions — Bobbin injects many more files than the agent modifies. Some of that is by design, since contextual files (related code, tests, documentation) inform decisions without being edit targets. But injection *recall* is also low (0.067 on ruff-001), and that is harder to excuse: it means the specific files the agent needed were frequently not injected.

The most interesting row is `semantic_weight=0.0`, which has the **highest** injection precision (0.181) and the **lowest** task F1 (0.252). If overlap between injected and edited files measured usefulness, that combination would be impossible. We read it as evidence that **injection-overlap metrics are a poor proxy for injection value**: semantic search appears to help the agent *reason its way to* the right files, and the files that support that reasoning are not the files it edits. This undercuts the obvious cheap evaluation for context systems — measuring what fraction of injected context gets used — and is an argument for end-task evaluation despite its cost and variance.

Given the N in Table 7, these are observations to be tested, not results.

### 5.7 Withdrawn: calibration sweep

The original study included a configuration calibration sweep over semantic weight and doc demotion, and concluded that a semantic weight of 0.90 outperformed the 0.70 default. **That sweep was run entirely on the Flask tasks and is withdrawn** — every configuration in it was scored on fixtures with a 0% pass rate on both arms (§4.2). We report its withdrawal rather than silently dropping it because the recommendation had already been drawn from it, and because "the tuning study ran on the broken subset" is a failure mode worth naming.

We make no configuration recommendation. Establishing one requires re-running the sweep on the unquarantined tasks at the N indicated in §5.3.

---

## 6. Discussion

### 6.1 What the decomposition suggests

**Retrieval beats filtering, and the gap is large.** The three retrieval-expansion methods (0.247-0.384) separate from the three filtering methods (0.025-0.080) by roughly an order of magnitude. This is the study's most robust structural claim — robust in the sense that it does not depend on any individual arm being significant, only on the grouping being real, and the grouping is what survives when the individual magnitudes do not. For a practitioner the implication is direct: **spend engineering effort on getting more of the right things into the candidate set, not on filtering the candidate set afterwards.**

**Structural signals appear to complement semantic ones.** Blame bridging (−0.303) and coupling expansion (−0.247) surface files that are related to the query through *repository history* rather than through text or embedding similarity. Nothing in the query resembles those files; they are reached because the commit record connects them to something that does. If this holds at proper N it is the more interesting half of the result, because it is the half a purely embedding-based system cannot replicate.

**Keyword-only injection may be worse than none.** With semantic search disabled, F1 (0.252) falls below no injection at all (0.324). The obvious reading is that lexical matching injects code that *looks* relevant and is not, and misleading context is worse than absent context. This is a claim about the *risk* profile of injection systems that we think deserves direct study — a badly-retrieving injector is not merely a weak one.

### 6.2 What we cannot claim, and why that matters

**Nothing here is statistically established.** After correction for six comparisons, no arm is significant (§5.3). We restate this in the discussion because it is easy for a reader to carry Table 1's bolded numbers forward and forget the intervals attached to them.

**The tool-availability confound is the deepest problem.** As §5.2 shows, our own data attribute ~92% of the measured benefit to the agent having and using a search tool, and ~8% to automatic injection. We do not believe this is specific to Bobbin. Any evaluation that compares "agent with context system" against "agent without" changes two things simultaneously — the injection mechanism *and* the agent's awareness that retrieval tooling exists — unless it deliberately holds the second fixed. We did not, and we suspect it is easy not to.

**Injection-overlap metrics do not measure injection value.** The condition with the best injected-vs-edited overlap (`semantic_weight=0.0`, precision 0.181) is the condition with the worst task performance (§5.6). Whatever useful injection does, it is not well captured by counting how much injected context ends up edited. This closes off the cheap evaluation and is part of why the expensive one is noisy.

**A study can be substantially larger than ours and still be underpowered.** The 108-run design the original protocol called for would not have been sufficient for the filtering arms, which need hundreds to thousands of runs each. Scaling a noisy design is not the same as fixing it; the effects have to be big enough to see.

### 6.3 Limitations

**Small sample sizes.** N=3-7 per condition against a per-run standard deviation of 0.347. Table 4 gives the required N per arm. Stated in the abstract as well as here, because a limitation discovered late in a paper reads as concealment.

**Single-task ablation.** All ablation data come from ruff-001. The method ranking may not transfer across repositories, languages, or task types, and we have no evidence that it does. Cargo-001 scores 1.000 in both arms and provides no signal.

**Withdrawn and missing tasks.** The five Flask tasks are withdrawn as broken fixtures (§4.2), which also withdraws the calibration sweep (§5.7). Django-001 and pandas-001 produced no completed runs; polars-005 has no with-bobbin runs. Cross-repository generalisation is correspondingly weak: the reported evidence rests on Ruff plus two isolated tasks.

**File-level F1 is a proxy for correctness.** We score overlap between the agent's edited file set and ground truth. An agent can touch the right files and write wrong code, or fix the problem in a different valid place. Test pass rate would be the better metric but does not discriminate here — it is 100% across every ruff-001 condition (§5.5).

**The measured system is not the current system.** The runs reported here were produced in February-March 2026. Bobbin has changed since, and in at least one material respect: query-intent classification, which alters retrieval parameters per prompt, did not exist when these measurements were taken. Present-tense claims about "default configuration" in §2-3 describe the system as it now stands, not the binary that produced §5. We verified this from run artifacts rather than assuming it.

**Prompt-template artifacts.** Bobbin's intent classifier reads the last 500 characters of the prompt. Our harness appends a 416-character instruction block to every with-bobbin prompt, so were the study re-run today the classifier would be reading mostly our own boilerplate rather than the task. Any future intent-sensitive experiment must fix the harness first. We note it because it is a general hazard: **the evaluation harness is part of the system under test**, and prompt scaffolding added for one purpose can silently drive a mechanism measured for another.

---

## 7. Related Work

**Retrieval-augmented generation.** Injecting retrieved passages into a language model's context is the standard remedy for knowledge the model lacks, established by Lewis et al. (2020) for open-domain QA and extended by REALM (Guu et al., 2020) and Atlas (Izacard et al., 2022). Our setting differs in the unit and the trigger: the retrieval corpus is a single repository rather than a general corpus, the query is an agent's task description rather than a question, and success is measured by downstream *edits* rather than answer accuracy. The decomposition question we ask — which retrieval component carries the effect — is comparatively rare in that literature, where systems are more often evaluated end-to-end.

**Hybrid lexical and dense retrieval, and rank fusion.** Bobbin's hybrid search follows the standard finding that sparse and dense retrieval are complementary (Karpukhin et al., 2020 for DPR; Formal et al., 2021 for SPLADE). We fuse with Reciprocal Rank Fusion (Cormack et al., 2009), which a reviewer will name as the obvious baseline and which is exactly what we use. Our semantic-search ablation is a direct measurement of what the dense half contributes in this setting, and its magnitude (−0.384, the largest we observe) is consistent with the general result that the dense half dominates on conceptual queries.

**Code search.** Neural code search has a substantial literature — CodeSearchNet (Husain et al., 2019) as the benchmark, CodeBERT (Feng et al., 2020) and GraphCodeBERT (Guo et al., 2021) as representative encoders. That work optimises retrieval quality against human relevance judgements. We measure something downstream and noisier: whether retrieval changes what an agent *does*. The two can diverge, and §5.6 is a concrete instance — our best-overlap condition is our worst-performing one.

**Agent context management and context-window budgeting.** Managing what occupies a limited context window is an active area, from summarisation and recursive memory schemes (Park et al., 2023; Packer et al., 2023 on MemGPT) to the retrieval-over-long-context comparisons in Xu et al. (2024). Bobbin's budget-and-demote assembly stage is a simple instance of this. Our finding that the *filtering* methods (gating, demotion, recency) show effects at or below 0.080 is a mild negative result for that direction in this setting, though our power to detect such effects is poor and we do not claim it.

**Repository history as supervision.** This is the paper's least conventional component and the one most in need of positioning. Using version-control history as a signal has precedent in defect prediction and change-impact analysis — Zimmermann et al. (2005) mined co-change to predict which files change together, and the evolutionary-coupling literature builds on it — but that work predicts *changes* for human developers rather than assembling *context* for an agent. Blame bridging, which uses commit co-occurrence to link documentation to the source it describes, we are not aware of as a retrieval mechanism elsewhere. Its measured effect (−0.303) is second only to semantic search, which makes it the result we would most like to see replicated at proper N.

**Injecting analysis into the decoding loop.** The nearest neighbour to our mechanism is Monitor-Guided Decoding (Agrawal et al., 2023), which consults static analysis during generation to constrain the model toward valid identifiers. The shared idea is that a language model benefits from facts a program analysis can supply and it cannot infer. The differences are the granularity and the timing: MGD intervenes per-token with a hard constraint from a type system, while Bobbin intervenes once per turn with a soft prior assembled from retrieval and repository history. MGD's guarantees are stronger; our signal is broader and, as this paper shows, correspondingly harder to measure.

**Evaluating coding agents.** SWE-bench (Jimenez et al., 2024) established repository-scale agent evaluation against real issues and their fix commits, and our harness follows its shape — real repositories, real commits, ground truth from the actual fix. Reported variance across repeated runs on such benchmarks motivates our emphasis in §5.3: our contribution there is not that agent evaluation is noisy, which is known, but a concrete accounting of what that noise costs in runs per condition for a *component-level* ablation, which is a stricter requirement than end-to-end comparison.

*Bibliography note: the quipu submission assembled overlapping references for RAG, hybrid retrieval and agent evaluation; reuse that BibTeX rather than rebuilding it. Citation keys and full entries are pending the LaTeX conversion (see `docs/plans/paper-arxiv-submission.md`).*

---

---

## 8. Future Work

### 8.1 Format Mode Experiments

Bobbin supports four output format modes for injected context: standard (default), minimal (clean with no metadata), verbose (standard with type annotations), and XML (structured tags). A format comparison study is planned to determine whether structured formatting improves agent utilization of injected context.

### 8.2 Production Feedback Loop

Bobbin includes an injection feedback system where agents and users can rate injections as useful, noise, or harmful. Each injection receives a ULID-based identifier, and feedback is stored in a SQLite database alongside the injection record. Accumulating production feedback data will enable:

- Per-chunk quality signals for reranking
- Automated detection of consistently noisy file patterns
- Adaptive threshold tuning based on historical usefulness

### 8.3 Adaptive Injection

Current injection uses fixed configuration parameters. Future work could adapt injection strategy based on:

- Task type detection (bug fix vs. feature vs. refactor)
- Repository characteristics (size, language mix, commit frequency)
- Agent behavior patterns (exploration-heavy agents may benefit from broader injection)

### 8.4 Larger-Scale Evaluation

The current study is limited to 13 tasks across 4 repositories. A more comprehensive evaluation would include:

- Additional languages (Go, TypeScript, Java)
- Larger repositories (monorepos, multi-package workspaces)
- More task types (security fixes, performance optimization, dependency upgrades)
- Sufficient runs per condition (N >= 10) for statistical significance testing

### 8.5 Temporal Decay Analysis

The measurement framework design calls for coupling depth sweeps (100, 500, 1000, 5000 commits) and recency weight sweeps to characterize how these signals decay with commit age. This data would inform automatic parameter selection.

---

## 9. Conclusion

We set out to measure which of six context-injection methods carries the weight in a working system, and we report two things: a preliminary decomposition, and an account of why a study of this size cannot settle it.

**The decomposition.** Removing a method and re-measuring produces effects up to fourteen times larger than the aggregate with-versus-without comparison, which is the case for making removal the primary experiment. The six methods separate into two groups: retrieval-expansion (semantic search −0.384, blame bridging −0.303, coupling expansion −0.247) and filtering (doc demotion −0.080, recency −0.025, gating −0.025). We regard the *grouping* as the transferable result and the individual magnitudes as provisional. The practical reading is that effort belongs in what enters the candidate set rather than in what is filtered from it, and that repository history is a retrieval signal distinct from — not a proxy for — semantic similarity.

**Why it is not settled.** After correcting for six comparisons, no arm is significant. Five of six confidence intervals cross zero. The aggregate effect the original study led with (+0.027 F1) is an order of magnitude below the per-run standard deviation of 0.347, and cannot be tested at all because the aggregate was reported without dispersion. Most seriously, one arm turned out to disable injection entirely while leaving the agent a search tool it was told to use, and comparing it against both baselines attributes roughly 92% of the apparent benefit to tool availability rather than to injection. That is a confound in the design, not a defect in the implementation, and we expect it to recur in any evaluation that compares an agent with a context system against an agent without one.

**What we would tell someone repeating this.** Three things, in order of how much they cost to get wrong. First, hold tool availability fixed — the control arm is "system installed and advertised, injection disabled", not "system absent". Second, power the study on the effects you intend to claim: 14, 22 and 32 runs per arm for the retrieval-expansion effects here, and simply do not claim effects that would need thousands. Third, treat the harness as part of the system under test — a task whose pass rate is invariant across arms is a suspected broken fixture, and prompt scaffolding added for one purpose can silently drive a mechanism measured for another. Each of these cost us a result.

Automated context injection remains a promising direction and we continue to build on it. But the evidence that it works, as distinct from the evidence that giving an agent a good search tool works, is thinner than the field's published aggregates suggest — including our own earlier draft's. The harness, the tasks, the run artifacts and the analysis scripts are open, and the follow-up experiment that would settle the three retrieval-expansion effects costs roughly $105 and a day.

---

## Appendix A: Raw Data Sources

- Baseline comparison: 66 runs across 13 tasks (`eval/results/fresh-report.md`)
- Ablation study: 85 runs, 8 conditions on ruff-001 (`eval/results/ablation-report-final.md`)
- Calibration sweep: **withdrawn** — 6 configurations across 4 Flask tasks (§5.7)
- Per-run artifacts: `eval/results/runs/<run-id>/`, including `*_metrics.jsonl`, which is what establishes injection actually occurred in a given arm (§5.2)
- Evaluation framework: headless Claude Code agents, `claude-sonnet-4-5-20250929`
- Statistics: `scripts/paper_stats.py` regenerates every figure in §5.1-5.3
- Measurement-validity audit: `docs/plans/paper-measurement-validity.md`

**Discrepancy to resolve before submission.** The run counts are not mutually consistent: the abstract of the original draft reported 85 ablation runs, the study script comments describe "4 tasks x 8 conditions x 3 attempts" (96), and the cost estimate below is quoted for 108. These are three numbers for one study and at most one is right. The per-run artifacts are authoritative and should be recounted directly.

## Appendix B: Reproduction

```bash
# Baseline comparison
python3 -m runner.cli run-task <task> --approach no-bobbin
python3 -m runner.cli run-task <task> --approach with-bobbin

# Ablation (example: disable semantic search)
python3 -m runner.cli run-task ruff-001 --approach with-bobbin -C semantic_weight=0.0

# Statistics over the reported means
python3 scripts/paper_stats.py
```

Estimated cost: ~$1-2 per run, ~5 minutes per run.

**Two caveats for anyone reproducing this.**

1. **The task suite has changed.** `eval/tasks/` now contains 40 tasks across cargo, django, go, nushell, pandas, polars, ruff and typst. The 13 tasks reported here are a subset, and the five Flask tasks are in `eval/tasks/_quarantined/`. Reproducing §5 requires selecting the reported subset explicitly.
2. **The code has changed.** These runs were produced in February-March 2026 by a binary that predates this repository's git history, and it differed materially from the current one — query-intent classification did not exist (§6.3). An exact reproduction is not currently possible; a re-run measures today's system. Given §5.2, a re-run should add the missing control arm (installed and advertised, hook disabled) rather than repeating the original design.
