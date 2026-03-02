# Ablation Analysis — Context Injection Paper

Generated from 85 completed results across 13 tasks.
Study tasks: ruff-001, cargo-001, django-001, pandas-001
Additional tasks with data: flask-001, flask-002, flask-003, flask-004, flask-005, polars-004, polars-005, ruff-002, ruff-003, ruff-004, ruff-005

## Baseline Comparison: No-Injection vs With-Injection

| Task | No-Bobbin F1 | With-Bobbin F1 | Delta | Tests (NB) | Tests (WB) |
|------|:------------:|:--------------:|:-----:|:----------:|:----------:|
| ruff-001 | 0.324 | 0.636 | +0.312 | 100% | 100% |
| cargo-001 | 1.000 | 1.000 | +0.000 | 100% | 100% |
| django-001 | — | — | — | — | — |
| pandas-001 | — | — | — | — | — |
| **Average** | **0.662** | **0.818** | **+0.156** | | |

## Ablation Impact Summary

Effect of disabling each method (averaged across study tasks):

| Method Disabled | Baseline F1 | Ablated F1 | Delta | Impact | N |
|-----------------|:-----------:|:----------:|:-----:|:------:|:-:|
| Semantic search | 0.636 | 0.252 | -0.384 | hurts | 1 |
| Coupling expansion | 0.636 | 0.389 | -0.247 | hurts | 1 |
| Recency signal  | 0.636 | 0.611 | -0.025 | hurts | 1 |
| Doc demotion    | 0.636 | 0.556 | -0.081 | hurts | 1 |
| Quality gate    | 0.636 | 0.611 | -0.025 | hurts | 1 |
| Blame bridging  | 0.636 | 0.333 | -0.303 | hurts | 1 |

## Per-Task Ablation Breakdown

| Task | Approach | N | F1 (mean±std) | Pass% | Cost |
|------|----------|:-:|:-------------:|:-----:|:----:|
| ruff-001 | no-bobbin | 5 | 0.324±0.021 | 100% | $0.74 |
| ruff-001 | with-bobbin | 7 | 0.636±0.347 | 100% | $1.08 |
| ruff-001 | with-bobbin+semantic_weight=0.0 | 4 | 0.252±0.134 | 100% | $1.45 |
| ruff-001 | with-bobbin+coupling_depth=0 | 3 | 0.389±0.096 | 100% | $1.45 |
| ruff-001 | with-bobbin+recency_weight=0.0 | 3 | 0.611±0.347 | 100% | $1.43 |
| ruff-001 | with-bobbin+doc_demotion=0.0 | 3 | 0.556±0.385 | 100% | $1.48 |
| ruff-001 | with-bobbin+gate_threshold=1.0 | 3 | 0.611±0.347 | 100% | $1.39 |
| ruff-001 | with-bobbin+blame_bridging=false | 3 | 0.333±0.000 | 100% | $1.25 |
| | | | | | |
| cargo-001 | no-bobbin | 1 | 1.000±0.000 | 100% | $1.04 |
| cargo-001 | with-bobbin | 1 | 1.000±0.000 | 100% | $1.03 |
| cargo-001 | with-bobbin+semantic_weight=0.0 | 0 | — | — | — |
| cargo-001 | with-bobbin+coupling_depth=0 | 0 | — | — | — |
| cargo-001 | with-bobbin+recency_weight=0.0 | 0 | — | — | — |
| cargo-001 | with-bobbin+doc_demotion=0.0 | 0 | — | — | — |
| cargo-001 | with-bobbin+gate_threshold=1.0 | 0 | — | — | — |
| cargo-001 | with-bobbin+blame_bridging=false | 0 | — | — | — |
| | | | | | |
| django-001 | no-bobbin | 0 | — | — | — |
| django-001 | with-bobbin | 0 | — | — | — |
| django-001 | with-bobbin+semantic_weight=0.0 | 0 | — | — | — |
| django-001 | with-bobbin+coupling_depth=0 | 0 | — | — | — |
| django-001 | with-bobbin+recency_weight=0.0 | 0 | — | — | — |
| django-001 | with-bobbin+doc_demotion=0.0 | 0 | — | — | — |
| django-001 | with-bobbin+gate_threshold=1.0 | 0 | — | — | — |
| django-001 | with-bobbin+blame_bridging=false | 0 | — | — | — |
| | | | | | |
| pandas-001 | no-bobbin | 0 | — | — | — |
| pandas-001 | with-bobbin | 0 | — | — | — |
| pandas-001 | with-bobbin+semantic_weight=0.0 | 0 | — | — | — |
| pandas-001 | with-bobbin+coupling_depth=0 | 0 | — | — | — |
| pandas-001 | with-bobbin+recency_weight=0.0 | 0 | — | — | — |
| pandas-001 | with-bobbin+doc_demotion=0.0 | 0 | — | — | — |
| pandas-001 | with-bobbin+gate_threshold=1.0 | 0 | — | — | — |
| pandas-001 | with-bobbin+blame_bridging=false | 0 | — | — | — |
| | | | | | |
## Injection Usage Analysis

How well bobbin's injected files predicted what the agent actually touched:

| Task | Approach | Injection Precision | Injection Recall | Injection F1 |
|------|----------|:-------------------:|:----------------:|:------------:|
| ruff-001 | with-bobbin | 0.029 | 0.067 | 0.040 |
| ruff-001 | with-bobbin+semantic_weight=0.0 | 0.181 | 0.204 | 0.174 |
| ruff-001 | with-bobbin+coupling_depth=0 | 0.000 | 0.000 | 0.000 |
| ruff-001 | with-bobbin+recency_weight=0.0 | 0.000 | 0.000 | 0.000 |
| ruff-001 | with-bobbin+doc_demotion=0.0 | 0.026 | 0.111 | 0.042 |
| ruff-001 | with-bobbin+blame_bridging=false | 0.000 | 0.000 | 0.000 |
| cargo-001 | with-bobbin | 0.125 | 0.500 | 0.200 |
