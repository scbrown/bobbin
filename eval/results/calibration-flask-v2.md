# Bobbin Search Weight Calibration Report

## Summary (sorted by F1)

| Config | Semantic | DocDem | RRF k | Tasks | Precision | Recall | F1 | Top Score | Latency |
|--------|----------|--------|-------|-------|-----------|--------|----|-----------|---------|
| sw=0.50_dd=0.20_k=40.0 | 0.50 | 0.20 | 40 | 2 | 0.188 | 0.433 | **0.259** | 0.530 | 320ms |
| sw=0.50_dd=0.20_k=60.0 | 0.50 | 0.20 | 60 | 2 | 0.188 | 0.433 | **0.259** | 0.530 | 318ms |
| sw=0.50_dd=0.50_k=40.0 | 0.50 | 0.50 | 40 | 2 | 0.188 | 0.433 | **0.259** | 0.530 | 321ms |
| sw=0.50_dd=0.50_k=60.0 | 0.50 | 0.50 | 60 | 2 | 0.188 | 0.433 | **0.259** | 0.530 | 312ms |
| sw=0.50_dd=1.00_k=40.0 | 0.50 | 1.00 | 40 | 2 | 0.188 | 0.433 | **0.259** | 0.530 | 311ms |
| sw=0.50_dd=1.00_k=60.0 | 0.50 | 1.00 | 60 | 2 | 0.188 | 0.433 | **0.259** | 0.530 | 322ms |
| sw=0.90_dd=0.20_k=40.0 | 0.90 | 0.20 | 40 | 2 | 0.167 | 0.333 | **0.222** | 0.530 | 322ms |
| sw=0.90_dd=0.20_k=60.0 | 0.90 | 0.20 | 60 | 2 | 0.167 | 0.333 | **0.222** | 0.530 | 321ms |
| sw=0.90_dd=0.50_k=40.0 | 0.90 | 0.50 | 40 | 2 | 0.167 | 0.333 | **0.222** | 0.530 | 320ms |
| sw=0.90_dd=0.50_k=60.0 | 0.90 | 0.50 | 60 | 2 | 0.167 | 0.333 | **0.222** | 0.530 | 336ms |
| sw=0.70_dd=0.20_k=40.0 | 0.70 | 0.20 | 40 | 2 | 0.143 | 0.333 | **0.200** | 0.530 | 317ms |
| sw=0.70_dd=0.20_k=60.0 | 0.70 | 0.20 | 60 | 2 | 0.143 | 0.333 | **0.200** | 0.530 | 331ms |
| sw=0.70_dd=0.50_k=40.0 | 0.70 | 0.50 | 40 | 2 | 0.143 | 0.333 | **0.200** | 0.530 | 319ms |
| sw=0.70_dd=0.50_k=60.0 | 0.70 | 0.50 | 60 | 2 | 0.143 | 0.333 | **0.200** | 0.530 | 313ms |
| sw=0.70_dd=1.00_k=40.0 | 0.70 | 1.00 | 40 | 2 | 0.125 | 0.333 | **0.182** | 0.530 | 313ms |
| sw=0.70_dd=1.00_k=60.0 | 0.70 | 1.00 | 60 | 2 | 0.125 | 0.333 | **0.182** | 0.530 | 331ms |
| sw=0.90_dd=1.00_k=40.0 | 0.90 | 1.00 | 40 | 2 | 0.125 | 0.333 | **0.182** | 0.530 | 308ms |
| sw=0.90_dd=1.00_k=60.0 | 0.90 | 1.00 | 60 | 2 | 0.125 | 0.333 | **0.182** | 0.530 | 305ms |

## Best Config: sw=0.50_dd=0.20_k=40.0
- F1: 0.2587
- Precision: 0.1875
- Recall: 0.4333

## Per-Task Results


### flask-002

Ground truth files: setup.cfg, src/flask/cli.py, tests/test_cli.py

| Config | Precision | Recall | F1 | Returned Files |
|--------|-----------|--------|----|----------------|
| sw=0.90_dd=0.20_k=40.0 | 0.333 | 0.667 | 0.444 | src/flask/cli.py, tests/test_helpers.py, tests/test_cli.py, tests/conftest.py, src/flask/helpers.py (+1 more) |
| sw=0.90_dd=0.20_k=60.0 | 0.333 | 0.667 | 0.444 | src/flask/cli.py, tests/test_helpers.py, tests/test_cli.py, tests/conftest.py, src/flask/helpers.py (+1 more) |
| sw=0.90_dd=0.50_k=40.0 | 0.333 | 0.667 | 0.444 | src/flask/cli.py, tests/test_helpers.py, tests/test_cli.py, tests/conftest.py, src/flask/helpers.py (+1 more) |
| sw=0.90_dd=0.50_k=60.0 | 0.333 | 0.667 | 0.444 | src/flask/cli.py, tests/test_helpers.py, tests/test_cli.py, tests/conftest.py, src/flask/helpers.py (+1 more) |
| sw=0.70_dd=0.20_k=40.0 | 0.286 | 0.667 | 0.400 | tests/test_helpers.py, src/flask/cli.py, tests/test_cli.py, src/flask/scaffold.py, src/flask/helpers.py (+2 more) |
| sw=0.70_dd=0.20_k=60.0 | 0.286 | 0.667 | 0.400 | tests/test_helpers.py, src/flask/cli.py, tests/test_cli.py, src/flask/scaffold.py, src/flask/helpers.py (+2 more) |
| sw=0.70_dd=0.50_k=40.0 | 0.286 | 0.667 | 0.400 | tests/test_helpers.py, src/flask/cli.py, tests/test_cli.py, src/flask/scaffold.py, src/flask/helpers.py (+2 more) |
| sw=0.70_dd=0.50_k=60.0 | 0.286 | 0.667 | 0.400 | tests/test_helpers.py, src/flask/cli.py, tests/test_cli.py, src/flask/scaffold.py, src/flask/helpers.py (+2 more) |
| sw=0.50_dd=0.20_k=40.0 | 0.250 | 0.667 | 0.364 | tests/test_helpers.py, src/flask/cli.py, tests/test_cli.py, src/flask/scaffold.py, src/flask/helpers.py (+3 more) |
| sw=0.50_dd=0.20_k=60.0 | 0.250 | 0.667 | 0.364 | tests/test_helpers.py, src/flask/cli.py, tests/test_cli.py, src/flask/scaffold.py, src/flask/helpers.py (+3 more) |
| sw=0.50_dd=0.50_k=40.0 | 0.250 | 0.667 | 0.364 | tests/test_helpers.py, src/flask/cli.py, tests/test_cli.py, src/flask/scaffold.py, src/flask/helpers.py (+3 more) |
| sw=0.50_dd=0.50_k=60.0 | 0.250 | 0.667 | 0.364 | tests/test_helpers.py, src/flask/cli.py, tests/test_cli.py, src/flask/scaffold.py, src/flask/helpers.py (+3 more) |
| sw=0.50_dd=1.00_k=40.0 | 0.250 | 0.667 | 0.364 | tests/test_helpers.py, src/flask/cli.py, tests/test_cli.py, setup.py, src/flask/scaffold.py (+3 more) |
| sw=0.50_dd=1.00_k=60.0 | 0.250 | 0.667 | 0.364 | tests/test_helpers.py, src/flask/cli.py, tests/test_cli.py, setup.py, src/flask/scaffold.py (+3 more) |
| sw=0.70_dd=1.00_k=40.0 | 0.250 | 0.667 | 0.364 | tests/test_helpers.py, src/flask/cli.py, tests/test_cli.py, setup.py, src/flask/scaffold.py (+3 more) |
| sw=0.70_dd=1.00_k=60.0 | 0.250 | 0.667 | 0.364 | tests/test_helpers.py, src/flask/cli.py, tests/test_cli.py, setup.py, src/flask/scaffold.py (+3 more) |
| sw=0.90_dd=1.00_k=40.0 | 0.250 | 0.667 | 0.364 | src/flask/cli.py, tests/test_helpers.py, tests/test_cli.py, tests/conftest.py, src/flask/helpers.py (+3 more) |
| sw=0.90_dd=1.00_k=60.0 | 0.250 | 0.667 | 0.364 | src/flask/cli.py, tests/test_helpers.py, tests/test_cli.py, tests/conftest.py, src/flask/helpers.py (+3 more) |

### flask-003

Ground truth files: CHANGES.rst, src/flask/__init__.py, src/flask/app.py, src/flask/helpers.py, tests/test_helpers.py

| Config | Precision | Recall | F1 | Returned Files |
|--------|-----------|--------|----|----------------|
| sw=0.50_dd=0.20_k=40.0 | 0.125 | 0.200 | 0.154 | src/flask/cli.py, tests/test_user_error_handler.py, src/flask/helpers.py, src/flask/templating.py, src/flask/ctx.py (+3 more) |
| sw=0.50_dd=0.20_k=60.0 | 0.125 | 0.200 | 0.154 | src/flask/cli.py, src/flask/helpers.py, tests/test_user_error_handler.py, src/flask/templating.py, src/flask/ctx.py (+3 more) |
| sw=0.50_dd=0.50_k=40.0 | 0.125 | 0.200 | 0.154 | src/flask/cli.py, src/flask/helpers.py, tests/test_user_error_handler.py, src/flask/templating.py, src/flask/ctx.py (+3 more) |
| sw=0.50_dd=0.50_k=60.0 | 0.125 | 0.200 | 0.154 | src/flask/cli.py, src/flask/helpers.py, tests/test_user_error_handler.py, src/flask/templating.py, src/flask/ctx.py (+3 more) |
| sw=0.50_dd=1.00_k=40.0 | 0.125 | 0.200 | 0.154 | src/flask/cli.py, src/flask/helpers.py, tests/test_user_error_handler.py, src/flask/templating.py, src/flask/ctx.py (+3 more) |
| sw=0.50_dd=1.00_k=60.0 | 0.125 | 0.200 | 0.154 | src/flask/cli.py, tests/test_user_error_handler.py, src/flask/helpers.py, src/flask/templating.py, src/flask/ctx.py (+3 more) |
| sw=0.70_dd=0.20_k=40.0 | 0.000 | 0.000 | 0.000 | src/flask/cli.py, tests/test_user_error_handler.py, src/flask/templating.py, tests/test_basic.py, src/flask/ctx.py (+1 more) |
| sw=0.70_dd=0.20_k=60.0 | 0.000 | 0.000 | 0.000 | src/flask/cli.py, tests/test_user_error_handler.py, src/flask/templating.py, tests/test_basic.py, src/flask/ctx.py (+1 more) |
| sw=0.70_dd=0.50_k=40.0 | 0.000 | 0.000 | 0.000 | src/flask/cli.py, tests/test_user_error_handler.py, src/flask/templating.py, tests/test_basic.py, src/flask/ctx.py (+1 more) |
| sw=0.70_dd=0.50_k=60.0 | 0.000 | 0.000 | 0.000 | src/flask/cli.py, tests/test_user_error_handler.py, src/flask/templating.py, tests/test_basic.py, src/flask/ctx.py (+1 more) |
| sw=0.70_dd=1.00_k=40.0 | 0.000 | 0.000 | 0.000 | src/flask/cli.py, tests/test_user_error_handler.py, src/flask/templating.py, tests/test_basic.py, src/flask/ctx.py (+1 more) |
| sw=0.70_dd=1.00_k=60.0 | 0.000 | 0.000 | 0.000 | src/flask/cli.py, tests/test_user_error_handler.py, src/flask/templating.py, tests/test_basic.py, src/flask/ctx.py (+1 more) |
| sw=0.90_dd=0.20_k=40.0 | 0.000 | 0.000 | 0.000 | tests/test_user_error_handler.py, src/flask/templating.py, tests/test_basic.py, src/flask/cli.py, src/flask/ctx.py (+1 more) |
| sw=0.90_dd=0.20_k=60.0 | 0.000 | 0.000 | 0.000 | tests/test_user_error_handler.py, src/flask/templating.py, src/flask/cli.py, tests/test_basic.py, src/flask/ctx.py (+1 more) |
| sw=0.90_dd=0.50_k=40.0 | 0.000 | 0.000 | 0.000 | tests/test_user_error_handler.py, src/flask/templating.py, tests/test_basic.py, src/flask/cli.py, src/flask/ctx.py (+1 more) |
| sw=0.90_dd=0.50_k=60.0 | 0.000 | 0.000 | 0.000 | tests/test_user_error_handler.py, src/flask/templating.py, src/flask/cli.py, tests/test_basic.py, src/flask/ctx.py (+1 more) |
| sw=0.90_dd=1.00_k=40.0 | 0.000 | 0.000 | 0.000 | tests/test_user_error_handler.py, src/flask/templating.py, tests/test_basic.py, src/flask/cli.py, src/flask/ctx.py (+1 more) |
| sw=0.90_dd=1.00_k=60.0 | 0.000 | 0.000 | 0.000 | tests/test_user_error_handler.py, src/flask/templating.py, src/flask/cli.py, tests/test_basic.py, src/flask/ctx.py (+1 more) |