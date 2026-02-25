# Bobbin Search Weight Calibration Report

## Summary (sorted by F1)

| Config | Semantic | DocDem | RRF k | Tasks | Precision | Recall | F1 | Top Score | Latency |
|--------|----------|--------|-------|-------|-----------|--------|----|-----------|---------|
| sw=0.50_dd=0.20_k=40.0 | 0.50 | 0.20 | 40 | 2 | 0.000 | 0.000 | **0.000** | 0.530 | 464ms |
| sw=0.50_dd=0.20_k=60.0 | 0.50 | 0.20 | 60 | 2 | 0.000 | 0.000 | **0.000** | 0.530 | 368ms |
| sw=0.50_dd=0.50_k=40.0 | 0.50 | 0.50 | 40 | 2 | 0.000 | 0.000 | **0.000** | 0.530 | 370ms |
| sw=0.50_dd=0.50_k=60.0 | 0.50 | 0.50 | 60 | 2 | 0.000 | 0.000 | **0.000** | 0.530 | 366ms |
| sw=0.50_dd=1.00_k=40.0 | 0.50 | 1.00 | 40 | 2 | 0.000 | 0.000 | **0.000** | 0.530 | 369ms |
| sw=0.50_dd=1.00_k=60.0 | 0.50 | 1.00 | 60 | 2 | 0.000 | 0.000 | **0.000** | 0.530 | 392ms |
| sw=0.70_dd=0.20_k=40.0 | 0.70 | 0.20 | 40 | 2 | 0.000 | 0.000 | **0.000** | 0.530 | 400ms |
| sw=0.70_dd=0.20_k=60.0 | 0.70 | 0.20 | 60 | 2 | 0.000 | 0.000 | **0.000** | 0.530 | 368ms |
| sw=0.70_dd=0.50_k=40.0 | 0.70 | 0.50 | 40 | 2 | 0.000 | 0.000 | **0.000** | 0.530 | 364ms |
| sw=0.70_dd=0.50_k=60.0 | 0.70 | 0.50 | 60 | 2 | 0.000 | 0.000 | **0.000** | 0.530 | 369ms |
| sw=0.70_dd=1.00_k=40.0 | 0.70 | 1.00 | 40 | 2 | 0.000 | 0.000 | **0.000** | 0.530 | 370ms |
| sw=0.70_dd=1.00_k=60.0 | 0.70 | 1.00 | 60 | 2 | 0.000 | 0.000 | **0.000** | 0.530 | 368ms |
| sw=0.90_dd=0.20_k=40.0 | 0.90 | 0.20 | 40 | 2 | 0.000 | 0.000 | **0.000** | 0.530 | 389ms |
| sw=0.90_dd=0.20_k=60.0 | 0.90 | 0.20 | 60 | 2 | 0.000 | 0.000 | **0.000** | 0.530 | 367ms |
| sw=0.90_dd=0.50_k=40.0 | 0.90 | 0.50 | 40 | 2 | 0.000 | 0.000 | **0.000** | 0.530 | 344ms |
| sw=0.90_dd=0.50_k=60.0 | 0.90 | 0.50 | 60 | 2 | 0.000 | 0.000 | **0.000** | 0.530 | 374ms |
| sw=0.90_dd=1.00_k=40.0 | 0.90 | 1.00 | 40 | 2 | 0.000 | 0.000 | **0.000** | 0.530 | 376ms |
| sw=0.90_dd=1.00_k=60.0 | 0.90 | 1.00 | 60 | 2 | 0.000 | 0.000 | **0.000** | 0.530 | 399ms |

## Best Config: sw=0.50_dd=0.20_k=40.0
- F1: 0.0000
- Precision: 0.0000
- Recall: 0.0000

## Per-Task Results


### flask-002

Ground truth files: setup.cfg, src/flask/cli.py, tests/test_cli.py

| Config | Precision | Recall | F1 | Returned Files |
|--------|-----------|--------|----|----------------|
| sw=0.50_dd=0.20_k=40.0 | 0.000 | 0.000 | 0.000 | /tmp/bobbin-cal-xm0029o7/pallets--flask/tests/test_helpers.py, /tmp/bobbin-cal-xm0029o7/pallets--flask/src/flask/cli.py, /tmp/bobbin-cal-xm0029o7/pallets--flask/tests/test_cli.py, /tmp/bobbin-cal-xm0029o7/pallets--flask/src/flask/scaffold.py, /tmp/bobbin-cal-xm0029o7/pallets--flask/src/flask/helpers.py (+3 more) |
| sw=0.50_dd=0.20_k=60.0 | 0.000 | 0.000 | 0.000 | /tmp/bobbin-cal-xm0029o7/pallets--flask/tests/test_helpers.py, /tmp/bobbin-cal-xm0029o7/pallets--flask/src/flask/cli.py, /tmp/bobbin-cal-xm0029o7/pallets--flask/tests/test_cli.py, /tmp/bobbin-cal-xm0029o7/pallets--flask/src/flask/scaffold.py, /tmp/bobbin-cal-xm0029o7/pallets--flask/src/flask/helpers.py (+3 more) |
| sw=0.50_dd=0.50_k=40.0 | 0.000 | 0.000 | 0.000 | /tmp/bobbin-cal-xm0029o7/pallets--flask/tests/test_helpers.py, /tmp/bobbin-cal-xm0029o7/pallets--flask/src/flask/cli.py, /tmp/bobbin-cal-xm0029o7/pallets--flask/tests/test_cli.py, /tmp/bobbin-cal-xm0029o7/pallets--flask/src/flask/scaffold.py, /tmp/bobbin-cal-xm0029o7/pallets--flask/src/flask/helpers.py (+3 more) |
| sw=0.50_dd=0.50_k=60.0 | 0.000 | 0.000 | 0.000 | /tmp/bobbin-cal-xm0029o7/pallets--flask/tests/test_helpers.py, /tmp/bobbin-cal-xm0029o7/pallets--flask/src/flask/cli.py, /tmp/bobbin-cal-xm0029o7/pallets--flask/tests/test_cli.py, /tmp/bobbin-cal-xm0029o7/pallets--flask/src/flask/scaffold.py, /tmp/bobbin-cal-xm0029o7/pallets--flask/src/flask/helpers.py (+3 more) |
| sw=0.50_dd=1.00_k=40.0 | 0.000 | 0.000 | 0.000 | /tmp/bobbin-cal-xm0029o7/pallets--flask/tests/test_helpers.py, /tmp/bobbin-cal-xm0029o7/pallets--flask/src/flask/cli.py, /tmp/bobbin-cal-xm0029o7/pallets--flask/tests/test_cli.py, /tmp/bobbin-cal-xm0029o7/pallets--flask/setup.py, /tmp/bobbin-cal-xm0029o7/pallets--flask/src/flask/scaffold.py (+3 more) |
| sw=0.50_dd=1.00_k=60.0 | 0.000 | 0.000 | 0.000 | /tmp/bobbin-cal-xm0029o7/pallets--flask/tests/test_helpers.py, /tmp/bobbin-cal-xm0029o7/pallets--flask/src/flask/cli.py, /tmp/bobbin-cal-xm0029o7/pallets--flask/tests/test_cli.py, /tmp/bobbin-cal-xm0029o7/pallets--flask/setup.py, /tmp/bobbin-cal-xm0029o7/pallets--flask/src/flask/scaffold.py (+3 more) |
| sw=0.70_dd=0.20_k=40.0 | 0.000 | 0.000 | 0.000 | /tmp/bobbin-cal-xm0029o7/pallets--flask/tests/test_helpers.py, /tmp/bobbin-cal-xm0029o7/pallets--flask/src/flask/cli.py, /tmp/bobbin-cal-xm0029o7/pallets--flask/tests/test_cli.py, /tmp/bobbin-cal-xm0029o7/pallets--flask/src/flask/scaffold.py, /tmp/bobbin-cal-xm0029o7/pallets--flask/src/flask/helpers.py (+2 more) |
| sw=0.70_dd=0.20_k=60.0 | 0.000 | 0.000 | 0.000 | /tmp/bobbin-cal-xm0029o7/pallets--flask/tests/test_helpers.py, /tmp/bobbin-cal-xm0029o7/pallets--flask/src/flask/cli.py, /tmp/bobbin-cal-xm0029o7/pallets--flask/tests/test_cli.py, /tmp/bobbin-cal-xm0029o7/pallets--flask/src/flask/scaffold.py, /tmp/bobbin-cal-xm0029o7/pallets--flask/src/flask/helpers.py (+2 more) |
| sw=0.70_dd=0.50_k=40.0 | 0.000 | 0.000 | 0.000 | /tmp/bobbin-cal-xm0029o7/pallets--flask/tests/test_helpers.py, /tmp/bobbin-cal-xm0029o7/pallets--flask/src/flask/cli.py, /tmp/bobbin-cal-xm0029o7/pallets--flask/tests/test_cli.py, /tmp/bobbin-cal-xm0029o7/pallets--flask/src/flask/scaffold.py, /tmp/bobbin-cal-xm0029o7/pallets--flask/src/flask/helpers.py (+2 more) |
| sw=0.70_dd=0.50_k=60.0 | 0.000 | 0.000 | 0.000 | /tmp/bobbin-cal-xm0029o7/pallets--flask/tests/test_helpers.py, /tmp/bobbin-cal-xm0029o7/pallets--flask/src/flask/cli.py, /tmp/bobbin-cal-xm0029o7/pallets--flask/tests/test_cli.py, /tmp/bobbin-cal-xm0029o7/pallets--flask/src/flask/scaffold.py, /tmp/bobbin-cal-xm0029o7/pallets--flask/src/flask/helpers.py (+2 more) |
| sw=0.70_dd=1.00_k=40.0 | 0.000 | 0.000 | 0.000 | /tmp/bobbin-cal-xm0029o7/pallets--flask/tests/test_helpers.py, /tmp/bobbin-cal-xm0029o7/pallets--flask/src/flask/cli.py, /tmp/bobbin-cal-xm0029o7/pallets--flask/tests/test_cli.py, /tmp/bobbin-cal-xm0029o7/pallets--flask/setup.py, /tmp/bobbin-cal-xm0029o7/pallets--flask/src/flask/scaffold.py (+3 more) |
| sw=0.70_dd=1.00_k=60.0 | 0.000 | 0.000 | 0.000 | /tmp/bobbin-cal-xm0029o7/pallets--flask/tests/test_helpers.py, /tmp/bobbin-cal-xm0029o7/pallets--flask/src/flask/cli.py, /tmp/bobbin-cal-xm0029o7/pallets--flask/tests/test_cli.py, /tmp/bobbin-cal-xm0029o7/pallets--flask/setup.py, /tmp/bobbin-cal-xm0029o7/pallets--flask/src/flask/scaffold.py (+3 more) |
| sw=0.90_dd=0.20_k=40.0 | 0.000 | 0.000 | 0.000 | /tmp/bobbin-cal-xm0029o7/pallets--flask/src/flask/cli.py, /tmp/bobbin-cal-xm0029o7/pallets--flask/tests/test_helpers.py, /tmp/bobbin-cal-xm0029o7/pallets--flask/tests/test_cli.py, /tmp/bobbin-cal-xm0029o7/pallets--flask/tests/conftest.py, /tmp/bobbin-cal-xm0029o7/pallets--flask/src/flask/helpers.py (+1 more) |
| sw=0.90_dd=0.20_k=60.0 | 0.000 | 0.000 | 0.000 | /tmp/bobbin-cal-xm0029o7/pallets--flask/src/flask/cli.py, /tmp/bobbin-cal-xm0029o7/pallets--flask/tests/test_helpers.py, /tmp/bobbin-cal-xm0029o7/pallets--flask/tests/test_cli.py, /tmp/bobbin-cal-xm0029o7/pallets--flask/tests/conftest.py, /tmp/bobbin-cal-xm0029o7/pallets--flask/src/flask/helpers.py (+1 more) |
| sw=0.90_dd=0.50_k=40.0 | 0.000 | 0.000 | 0.000 | /tmp/bobbin-cal-xm0029o7/pallets--flask/src/flask/cli.py, /tmp/bobbin-cal-xm0029o7/pallets--flask/tests/test_helpers.py, /tmp/bobbin-cal-xm0029o7/pallets--flask/tests/test_cli.py, /tmp/bobbin-cal-xm0029o7/pallets--flask/tests/conftest.py, /tmp/bobbin-cal-xm0029o7/pallets--flask/src/flask/helpers.py (+1 more) |
| sw=0.90_dd=0.50_k=60.0 | 0.000 | 0.000 | 0.000 | /tmp/bobbin-cal-xm0029o7/pallets--flask/src/flask/cli.py, /tmp/bobbin-cal-xm0029o7/pallets--flask/tests/test_helpers.py, /tmp/bobbin-cal-xm0029o7/pallets--flask/tests/test_cli.py, /tmp/bobbin-cal-xm0029o7/pallets--flask/tests/conftest.py, /tmp/bobbin-cal-xm0029o7/pallets--flask/src/flask/helpers.py (+1 more) |
| sw=0.90_dd=1.00_k=40.0 | 0.000 | 0.000 | 0.000 | /tmp/bobbin-cal-xm0029o7/pallets--flask/src/flask/cli.py, /tmp/bobbin-cal-xm0029o7/pallets--flask/tests/test_helpers.py, /tmp/bobbin-cal-xm0029o7/pallets--flask/tests/test_cli.py, /tmp/bobbin-cal-xm0029o7/pallets--flask/tests/conftest.py, /tmp/bobbin-cal-xm0029o7/pallets--flask/src/flask/helpers.py (+3 more) |
| sw=0.90_dd=1.00_k=60.0 | 0.000 | 0.000 | 0.000 | /tmp/bobbin-cal-xm0029o7/pallets--flask/src/flask/cli.py, /tmp/bobbin-cal-xm0029o7/pallets--flask/tests/test_helpers.py, /tmp/bobbin-cal-xm0029o7/pallets--flask/tests/test_cli.py, /tmp/bobbin-cal-xm0029o7/pallets--flask/tests/conftest.py, /tmp/bobbin-cal-xm0029o7/pallets--flask/src/flask/helpers.py (+3 more) |

### flask-003

Ground truth files: CHANGES.rst, src/flask/__init__.py, src/flask/app.py, src/flask/helpers.py, tests/test_helpers.py

| Config | Precision | Recall | F1 | Returned Files |
|--------|-----------|--------|----|----------------|
| sw=0.50_dd=0.20_k=40.0 | 0.000 | 0.000 | 0.000 | /tmp/bobbin-cal-xm0029o7/pallets--flask/src/flask/cli.py, /tmp/bobbin-cal-xm0029o7/pallets--flask/src/flask/helpers.py, /tmp/bobbin-cal-xm0029o7/pallets--flask/tests/test_user_error_handler.py, /tmp/bobbin-cal-xm0029o7/pallets--flask/src/flask/templating.py, /tmp/bobbin-cal-xm0029o7/pallets--flask/src/flask/ctx.py (+3 more) |
| sw=0.50_dd=0.20_k=60.0 | 0.000 | 0.000 | 0.000 | /tmp/bobbin-cal-xm0029o7/pallets--flask/src/flask/cli.py, /tmp/bobbin-cal-xm0029o7/pallets--flask/src/flask/helpers.py, /tmp/bobbin-cal-xm0029o7/pallets--flask/tests/test_user_error_handler.py, /tmp/bobbin-cal-xm0029o7/pallets--flask/src/flask/templating.py, /tmp/bobbin-cal-xm0029o7/pallets--flask/src/flask/ctx.py (+3 more) |
| sw=0.50_dd=0.50_k=40.0 | 0.000 | 0.000 | 0.000 | /tmp/bobbin-cal-xm0029o7/pallets--flask/src/flask/cli.py, /tmp/bobbin-cal-xm0029o7/pallets--flask/tests/test_user_error_handler.py, /tmp/bobbin-cal-xm0029o7/pallets--flask/src/flask/helpers.py, /tmp/bobbin-cal-xm0029o7/pallets--flask/src/flask/templating.py, /tmp/bobbin-cal-xm0029o7/pallets--flask/src/flask/ctx.py (+3 more) |
| sw=0.50_dd=0.50_k=60.0 | 0.000 | 0.000 | 0.000 | /tmp/bobbin-cal-xm0029o7/pallets--flask/src/flask/cli.py, /tmp/bobbin-cal-xm0029o7/pallets--flask/src/flask/helpers.py, /tmp/bobbin-cal-xm0029o7/pallets--flask/tests/test_user_error_handler.py, /tmp/bobbin-cal-xm0029o7/pallets--flask/src/flask/templating.py, /tmp/bobbin-cal-xm0029o7/pallets--flask/src/flask/ctx.py (+3 more) |
| sw=0.50_dd=1.00_k=40.0 | 0.000 | 0.000 | 0.000 | /tmp/bobbin-cal-xm0029o7/pallets--flask/src/flask/cli.py, /tmp/bobbin-cal-xm0029o7/pallets--flask/src/flask/helpers.py, /tmp/bobbin-cal-xm0029o7/pallets--flask/tests/test_user_error_handler.py, /tmp/bobbin-cal-xm0029o7/pallets--flask/src/flask/templating.py, /tmp/bobbin-cal-xm0029o7/pallets--flask/src/flask/ctx.py (+3 more) |
| sw=0.50_dd=1.00_k=60.0 | 0.000 | 0.000 | 0.000 | /tmp/bobbin-cal-xm0029o7/pallets--flask/src/flask/cli.py, /tmp/bobbin-cal-xm0029o7/pallets--flask/src/flask/helpers.py, /tmp/bobbin-cal-xm0029o7/pallets--flask/tests/test_user_error_handler.py, /tmp/bobbin-cal-xm0029o7/pallets--flask/src/flask/templating.py, /tmp/bobbin-cal-xm0029o7/pallets--flask/src/flask/ctx.py (+3 more) |
| sw=0.70_dd=0.20_k=40.0 | 0.000 | 0.000 | 0.000 | /tmp/bobbin-cal-xm0029o7/pallets--flask/src/flask/cli.py, /tmp/bobbin-cal-xm0029o7/pallets--flask/tests/test_user_error_handler.py, /tmp/bobbin-cal-xm0029o7/pallets--flask/src/flask/templating.py, /tmp/bobbin-cal-xm0029o7/pallets--flask/tests/test_basic.py, /tmp/bobbin-cal-xm0029o7/pallets--flask/src/flask/ctx.py (+1 more) |
| sw=0.70_dd=0.20_k=60.0 | 0.000 | 0.000 | 0.000 | /tmp/bobbin-cal-xm0029o7/pallets--flask/src/flask/cli.py, /tmp/bobbin-cal-xm0029o7/pallets--flask/tests/test_user_error_handler.py, /tmp/bobbin-cal-xm0029o7/pallets--flask/src/flask/templating.py, /tmp/bobbin-cal-xm0029o7/pallets--flask/tests/test_basic.py, /tmp/bobbin-cal-xm0029o7/pallets--flask/src/flask/ctx.py (+1 more) |
| sw=0.70_dd=0.50_k=40.0 | 0.000 | 0.000 | 0.000 | /tmp/bobbin-cal-xm0029o7/pallets--flask/src/flask/cli.py, /tmp/bobbin-cal-xm0029o7/pallets--flask/tests/test_user_error_handler.py, /tmp/bobbin-cal-xm0029o7/pallets--flask/src/flask/templating.py, /tmp/bobbin-cal-xm0029o7/pallets--flask/tests/test_basic.py, /tmp/bobbin-cal-xm0029o7/pallets--flask/src/flask/ctx.py (+1 more) |
| sw=0.70_dd=0.50_k=60.0 | 0.000 | 0.000 | 0.000 | /tmp/bobbin-cal-xm0029o7/pallets--flask/src/flask/cli.py, /tmp/bobbin-cal-xm0029o7/pallets--flask/tests/test_user_error_handler.py, /tmp/bobbin-cal-xm0029o7/pallets--flask/src/flask/templating.py, /tmp/bobbin-cal-xm0029o7/pallets--flask/tests/test_basic.py, /tmp/bobbin-cal-xm0029o7/pallets--flask/src/flask/ctx.py (+1 more) |
| sw=0.70_dd=1.00_k=40.0 | 0.000 | 0.000 | 0.000 | /tmp/bobbin-cal-xm0029o7/pallets--flask/src/flask/cli.py, /tmp/bobbin-cal-xm0029o7/pallets--flask/tests/test_user_error_handler.py, /tmp/bobbin-cal-xm0029o7/pallets--flask/src/flask/templating.py, /tmp/bobbin-cal-xm0029o7/pallets--flask/tests/test_basic.py, /tmp/bobbin-cal-xm0029o7/pallets--flask/src/flask/ctx.py (+1 more) |
| sw=0.70_dd=1.00_k=60.0 | 0.000 | 0.000 | 0.000 | /tmp/bobbin-cal-xm0029o7/pallets--flask/src/flask/cli.py, /tmp/bobbin-cal-xm0029o7/pallets--flask/tests/test_user_error_handler.py, /tmp/bobbin-cal-xm0029o7/pallets--flask/src/flask/templating.py, /tmp/bobbin-cal-xm0029o7/pallets--flask/tests/test_basic.py, /tmp/bobbin-cal-xm0029o7/pallets--flask/src/flask/ctx.py (+1 more) |
| sw=0.90_dd=0.20_k=40.0 | 0.000 | 0.000 | 0.000 | /tmp/bobbin-cal-xm0029o7/pallets--flask/tests/test_user_error_handler.py, /tmp/bobbin-cal-xm0029o7/pallets--flask/src/flask/templating.py, /tmp/bobbin-cal-xm0029o7/pallets--flask/tests/test_basic.py, /tmp/bobbin-cal-xm0029o7/pallets--flask/src/flask/cli.py, /tmp/bobbin-cal-xm0029o7/pallets--flask/src/flask/ctx.py (+1 more) |
| sw=0.90_dd=0.20_k=60.0 | 0.000 | 0.000 | 0.000 | /tmp/bobbin-cal-xm0029o7/pallets--flask/tests/test_user_error_handler.py, /tmp/bobbin-cal-xm0029o7/pallets--flask/src/flask/templating.py, /tmp/bobbin-cal-xm0029o7/pallets--flask/src/flask/cli.py, /tmp/bobbin-cal-xm0029o7/pallets--flask/tests/test_basic.py, /tmp/bobbin-cal-xm0029o7/pallets--flask/src/flask/ctx.py (+1 more) |
| sw=0.90_dd=0.50_k=40.0 | 0.000 | 0.000 | 0.000 | /tmp/bobbin-cal-xm0029o7/pallets--flask/tests/test_user_error_handler.py, /tmp/bobbin-cal-xm0029o7/pallets--flask/src/flask/templating.py, /tmp/bobbin-cal-xm0029o7/pallets--flask/tests/test_basic.py, /tmp/bobbin-cal-xm0029o7/pallets--flask/src/flask/cli.py, /tmp/bobbin-cal-xm0029o7/pallets--flask/src/flask/ctx.py (+1 more) |
| sw=0.90_dd=0.50_k=60.0 | 0.000 | 0.000 | 0.000 | /tmp/bobbin-cal-xm0029o7/pallets--flask/tests/test_user_error_handler.py, /tmp/bobbin-cal-xm0029o7/pallets--flask/src/flask/templating.py, /tmp/bobbin-cal-xm0029o7/pallets--flask/src/flask/cli.py, /tmp/bobbin-cal-xm0029o7/pallets--flask/tests/test_basic.py, /tmp/bobbin-cal-xm0029o7/pallets--flask/src/flask/ctx.py (+1 more) |
| sw=0.90_dd=1.00_k=40.0 | 0.000 | 0.000 | 0.000 | /tmp/bobbin-cal-xm0029o7/pallets--flask/tests/test_user_error_handler.py, /tmp/bobbin-cal-xm0029o7/pallets--flask/src/flask/templating.py, /tmp/bobbin-cal-xm0029o7/pallets--flask/tests/test_basic.py, /tmp/bobbin-cal-xm0029o7/pallets--flask/src/flask/cli.py, /tmp/bobbin-cal-xm0029o7/pallets--flask/src/flask/ctx.py (+1 more) |
| sw=0.90_dd=1.00_k=60.0 | 0.000 | 0.000 | 0.000 | /tmp/bobbin-cal-xm0029o7/pallets--flask/tests/test_user_error_handler.py, /tmp/bobbin-cal-xm0029o7/pallets--flask/src/flask/templating.py, /tmp/bobbin-cal-xm0029o7/pallets--flask/src/flask/cli.py, /tmp/bobbin-cal-xm0029o7/pallets--flask/tests/test_basic.py, /tmp/bobbin-cal-xm0029o7/pallets--flask/src/flask/ctx.py (+1 more) |