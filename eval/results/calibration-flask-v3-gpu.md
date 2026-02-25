# Bobbin Search Weight Calibration Report

## Summary (sorted by F1)

| Config | Semantic | DocDem | RRF k | Tasks | Precision | Recall | F1 | Top Score | Latency |
|--------|----------|--------|-------|-------|-----------|--------|----|-----------|---------|
| sw=0.90_dd=0.30_k=60.0 | 0.90 | 0.30 | 60 | 4 | 0.408 | 0.554 | **0.461** | 0.524 | 310ms |
| sw=0.90_dd=0.50_k=60.0 | 0.90 | 0.50 | 60 | 4 | 0.408 | 0.554 | **0.461** | 0.524 | 306ms |
| sw=0.50_dd=0.30_k=60.0 | 0.50 | 0.30 | 60 | 4 | 0.427 | 0.442 | **0.397** | 0.524 | 318ms |
| sw=0.50_dd=0.50_k=60.0 | 0.50 | 0.50 | 60 | 4 | 0.427 | 0.442 | **0.397** | 0.524 | 306ms |
| sw=0.70_dd=0.30_k=60.0 | 0.70 | 0.30 | 60 | 4 | 0.363 | 0.454 | **0.375** | 0.524 | 321ms |
| sw=0.70_dd=0.50_k=60.0 | 0.70 | 0.50 | 60 | 4 | 0.363 | 0.454 | **0.375** | 0.524 | 311ms |

## Best Config: sw=0.90_dd=0.30_k=60.0
- F1: 0.4611
- Precision: 0.4083
- Recall: 0.5542

## Per-Task Results


### flask-002

Ground truth files: setup.cfg, src/flask/cli.py, tests/test_cli.py

| Config | Precision | Recall | F1 | Returned Files |
|--------|-----------|--------|----|----------------|
| sw=0.90_dd=0.30_k=60.0 | 0.333 | 0.667 | 0.444 | src/flask/cli.py, tests/test_helpers.py, tests/test_cli.py, tests/conftest.py, src/flask/helpers.py (+1 more) |
| sw=0.90_dd=0.50_k=60.0 | 0.333 | 0.667 | 0.444 | src/flask/cli.py, tests/test_helpers.py, tests/test_cli.py, tests/conftest.py, src/flask/helpers.py (+1 more) |
| sw=0.70_dd=0.30_k=60.0 | 0.286 | 0.667 | 0.400 | tests/test_helpers.py, src/flask/cli.py, tests/test_cli.py, src/flask/scaffold.py, src/flask/helpers.py (+2 more) |
| sw=0.70_dd=0.50_k=60.0 | 0.286 | 0.667 | 0.400 | tests/test_helpers.py, src/flask/cli.py, tests/test_cli.py, src/flask/scaffold.py, src/flask/helpers.py (+2 more) |
| sw=0.50_dd=0.30_k=60.0 | 0.250 | 0.667 | 0.364 | tests/test_helpers.py, src/flask/cli.py, tests/test_cli.py, src/flask/scaffold.py, src/flask/helpers.py (+3 more) |
| sw=0.50_dd=0.50_k=60.0 | 0.250 | 0.667 | 0.364 | tests/test_helpers.py, src/flask/cli.py, tests/test_cli.py, src/flask/scaffold.py, src/flask/helpers.py (+3 more) |

### flask-003

Ground truth files: CHANGES.rst, src/flask/__init__.py, src/flask/app.py, src/flask/helpers.py, tests/test_helpers.py

| Config | Precision | Recall | F1 | Returned Files |
|--------|-----------|--------|----|----------------|
| sw=0.50_dd=0.30_k=60.0 | 0.125 | 0.200 | 0.154 | src/flask/cli.py, src/flask/helpers.py, tests/test_user_error_handler.py, src/flask/templating.py, src/flask/ctx.py (+3 more) |
| sw=0.50_dd=0.50_k=60.0 | 0.125 | 0.200 | 0.154 | src/flask/cli.py, src/flask/helpers.py, tests/test_user_error_handler.py, src/flask/templating.py, src/flask/ctx.py (+3 more) |
| sw=0.70_dd=0.30_k=60.0 | 0.000 | 0.000 | 0.000 | src/flask/cli.py, tests/test_user_error_handler.py, src/flask/templating.py, tests/test_basic.py, src/flask/ctx.py (+1 more) |
| sw=0.70_dd=0.50_k=60.0 | 0.000 | 0.000 | 0.000 | src/flask/cli.py, tests/test_user_error_handler.py, src/flask/templating.py, tests/test_basic.py, src/flask/ctx.py (+1 more) |
| sw=0.90_dd=0.30_k=60.0 | 0.000 | 0.000 | 0.000 | tests/test_user_error_handler.py, src/flask/templating.py, src/flask/cli.py, tests/test_basic.py, src/flask/ctx.py (+1 more) |
| sw=0.90_dd=0.50_k=60.0 | 0.000 | 0.000 | 0.000 | tests/test_user_error_handler.py, src/flask/templating.py, src/flask/cli.py, tests/test_basic.py, src/flask/ctx.py (+1 more) |

### flask-004

Ground truth files: CHANGES.rst, src/flask/__init__.py, src/flask/app.py, src/flask/helpers.py, tests/test_helpers.py

| Config | Precision | Recall | F1 | Returned Files |
|--------|-----------|--------|----|----------------|
| sw=0.90_dd=0.30_k=60.0 | 0.800 | 0.800 | 0.800 | src/flask/cli.py, src/flask/app.py, src/flask/__init__.py, tests/test_helpers.py, src/flask/helpers.py |
| sw=0.90_dd=0.50_k=60.0 | 0.800 | 0.800 | 0.800 | src/flask/cli.py, src/flask/app.py, src/flask/__init__.py, tests/test_helpers.py, src/flask/helpers.py |
| sw=0.50_dd=0.30_k=60.0 | 0.667 | 0.400 | 0.500 | src/flask/app.py, src/flask/cli.py, src/flask/__init__.py |
| sw=0.50_dd=0.50_k=60.0 | 0.667 | 0.400 | 0.500 | src/flask/app.py, src/flask/cli.py, src/flask/__init__.py |
| sw=0.70_dd=0.30_k=60.0 | 0.667 | 0.400 | 0.500 | src/flask/cli.py, src/flask/app.py, tests/test_helpers.py |
| sw=0.70_dd=0.50_k=60.0 | 0.667 | 0.400 | 0.500 | src/flask/cli.py, src/flask/app.py, tests/test_helpers.py |

### flask-005

Ground truth files: CHANGES.rst, src/flask/scaffold.py, tests/test_basic.py, tests/test_user_error_handler.py

| Config | Precision | Recall | F1 | Returned Files |
|--------|-----------|--------|----|----------------|
| sw=0.70_dd=0.30_k=60.0 | 0.500 | 0.750 | 0.600 | src/flask/scaffold.py, src/flask/app.py, tests/test_user_error_handler.py, tests/test_basic.py, src/flask/cli.py (+1 more) |
| sw=0.70_dd=0.50_k=60.0 | 0.500 | 0.750 | 0.600 | src/flask/scaffold.py, src/flask/app.py, tests/test_user_error_handler.py, tests/test_basic.py, src/flask/cli.py (+1 more) |
| sw=0.90_dd=0.30_k=60.0 | 0.500 | 0.750 | 0.600 | src/flask/scaffold.py, src/flask/app.py, tests/test_user_error_handler.py, tests/test_basic.py, src/flask/cli.py (+1 more) |
| sw=0.90_dd=0.50_k=60.0 | 0.500 | 0.750 | 0.600 | src/flask/scaffold.py, src/flask/app.py, tests/test_user_error_handler.py, tests/test_basic.py, src/flask/cli.py (+1 more) |
| sw=0.50_dd=0.30_k=60.0 | 0.667 | 0.500 | 0.571 | src/flask/scaffold.py, src/flask/app.py, tests/test_basic.py |
| sw=0.50_dd=0.50_k=60.0 | 0.667 | 0.500 | 0.571 | src/flask/scaffold.py, src/flask/app.py, tests/test_basic.py |