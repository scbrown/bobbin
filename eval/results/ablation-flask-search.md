# Flask Search-Level Ablation Report

Generated: 2026-02-25T18:11:13.640940+00:00

Baseline config: sw=0.9, dd=0.3, k=60.0, depth=1

## Summary (sorted by F1)

| Variant | Description | Tasks | Precision | Recall | F1 | Latency |
|---------|-------------|-------|-----------|--------|----|---------|
| no_semantic | Disable semantic search (pure keyword/BM25) | 3 | 0.600 | 0.517 | **0.552** | 323ms |
| no_keyword | Disable keyword search (pure semantic/embedding) | 3 | 0.357 | 0.517 | **0.422** | 314ms |
| baseline | Full pipeline: sw=0.90, dd=0.30, k=60, depth=1 | 3 | 0.333 | 0.450 | **0.382** | 331ms |
| no_coupling | Disable temporal coupling expansion | 3 | 0.333 | 0.450 | **0.382** | 313ms |
| no_doc_demotion | Treat docs same as source (doc_demotion=1.0) | 3 | 0.333 | 0.450 | **0.382** | 325ms |
| no_recency | Disable recency/freshness signal | 3 | 0.333 | 0.450 | **0.382** | 306ms |

## Delta from Baseline

| Variant | F1 | Delta | Impact |
|---------|-----|-------|--------|
| no_semantic | 0.552 | +0.171 | Removing this helps (helps) |
| no_keyword | 0.422 | +0.040 | Removing this helps (helps) |
| baseline | 0.382 | BASELINE | — |
| no_coupling | 0.382 | 0.000 | Removing this has no effect (neutral) |
| no_doc_demotion | 0.382 | 0.000 | Removing this has no effect (neutral) |
| no_recency | 0.382 | 0.000 | Removing this has no effect (neutral) |

## Per-Task Results


### flask-002

Ground truth: setup.cfg, src/flask/cli.py, tests/test_cli.py

| Variant | Precision | Recall | F1 | Returned Files |
|---------|-----------|--------|----|----------------|
| baseline | 0.000 | 0.000 | 0.000 |  |
| no_semantic | 0.000 | 0.000 | 0.000 |  |
| no_keyword | 0.000 | 0.000 | 0.000 |  |
| no_coupling | 0.000 | 0.000 | 0.000 |  |
| no_doc_demotion | 0.000 | 0.000 | 0.000 |  |
| no_recency | 0.000 | 0.000 | 0.000 |  |

### flask-003

Ground truth: CHANGES.rst, src/flask/__init__.py, src/flask/app.py, src/flask/helpers.py, tests/test_helpers.py

| Variant | Precision | Recall | F1 | Returned Files |
|---------|-----------|--------|----|----------------|
| no_semantic | 0.200 | 0.200 | 0.200 | src/flask/helpers.py, src/flask/cli.py, src/flask/ctx.py, src/flask/blueprints.py, src/flask/sessions.py |
| baseline | 0.000 | 0.000 | 0.000 | tests/test_user_error_handler.py, src/flask/templating.py, src/flask/cli.py, tests/test_basic.py, src/flask/ctx.py (+1 more) |
| no_keyword | 0.000 | 0.000 | 0.000 | tests/test_user_error_handler.py, src/flask/templating.py, tests/test_basic.py, src/flask/ctx.py, src/flask/cli.py (+1 more) |
| no_coupling | 0.000 | 0.000 | 0.000 | tests/test_user_error_handler.py, src/flask/templating.py, src/flask/cli.py, tests/test_basic.py, src/flask/ctx.py (+1 more) |
| no_doc_demotion | 0.000 | 0.000 | 0.000 | tests/test_user_error_handler.py, src/flask/templating.py, src/flask/cli.py, tests/test_basic.py, src/flask/ctx.py (+1 more) |
| no_recency | 0.000 | 0.000 | 0.000 | tests/test_user_error_handler.py, src/flask/templating.py, src/flask/cli.py, tests/test_basic.py, src/flask/ctx.py (+1 more) |

### flask-004

Ground truth: CHANGES.rst, src/flask/__init__.py, src/flask/app.py, src/flask/helpers.py, tests/test_helpers.py

| Variant | Precision | Recall | F1 | Returned Files |
|---------|-----------|--------|----|----------------|
| no_keyword | 0.571 | 0.800 | 0.667 | src/flask/cli.py, tests/test_helpers.py, src/flask/app.py, git:542cf30, src/flask/__init__.py (+2 more) |
| no_semantic | 0.600 | 0.600 | 0.600 | src/flask/cli.py, tests/test_helpers.py, src/flask/helpers.py, git:fdab801, src/flask/__init__.py |
| baseline | 0.500 | 0.600 | 0.545 | src/flask/cli.py, src/flask/app.py, git:542cf30, tests/test_helpers.py, src/flask/__init__.py (+1 more) |
| no_coupling | 0.500 | 0.600 | 0.545 | src/flask/cli.py, src/flask/app.py, git:542cf30, tests/test_helpers.py, src/flask/__init__.py (+1 more) |
| no_doc_demotion | 0.500 | 0.600 | 0.545 | src/flask/cli.py, src/flask/app.py, git:542cf30, tests/test_helpers.py, src/flask/__init__.py (+1 more) |
| no_recency | 0.500 | 0.600 | 0.545 | src/flask/cli.py, src/flask/app.py, git:542cf30, tests/test_helpers.py, src/flask/__init__.py (+1 more) |

### flask-005

Ground truth: CHANGES.rst, src/flask/scaffold.py, tests/test_basic.py, tests/test_user_error_handler.py

| Variant | Precision | Recall | F1 | Returned Files |
|---------|-----------|--------|----|----------------|
| no_semantic | 1.000 | 0.750 | 0.857 | src/flask/scaffold.py, tests/test_basic.py, tests/test_user_error_handler.py |
| baseline | 0.500 | 0.750 | 0.600 | src/flask/scaffold.py, src/flask/app.py, tests/test_user_error_handler.py, tests/test_basic.py, src/flask/cli.py (+1 more) |
| no_keyword | 0.500 | 0.750 | 0.600 | src/flask/scaffold.py, src/flask/app.py, tests/test_user_error_handler.py, tests/test_basic.py, src/flask/cli.py (+1 more) |
| no_coupling | 0.500 | 0.750 | 0.600 | src/flask/scaffold.py, src/flask/app.py, tests/test_user_error_handler.py, tests/test_basic.py, src/flask/cli.py (+1 more) |
| no_doc_demotion | 0.500 | 0.750 | 0.600 | src/flask/scaffold.py, src/flask/app.py, tests/test_user_error_handler.py, tests/test_basic.py, src/flask/cli.py (+1 more) |
| no_recency | 0.500 | 0.750 | 0.600 | src/flask/scaffold.py, src/flask/app.py, tests/test_user_error_handler.py, tests/test_basic.py, src/flask/cli.py (+1 more) |