# Bobbin Search Weight Calibration Report

## Summary (sorted by F1)

| Config | Semantic | DocDem | RRF k | Tasks | Precision | Recall | F1 | Top Score | Latency |
|--------|----------|--------|-------|-------|-----------|--------|----|-----------|---------|
| sw=0.50_dd=0.30_k=20.0 | 0.50 | 0.30 | 20 | 1 | 0.000 | 0.000 | **0.000** | 0.555 | 3204ms |
| sw=0.50_dd=0.30_k=40.0 | 0.50 | 0.30 | 40 | 1 | 0.000 | 0.000 | **0.000** | 0.555 | 3068ms |
| sw=0.50_dd=0.30_k=60.0 | 0.50 | 0.30 | 60 | 1 | 0.000 | 0.000 | **0.000** | 0.555 | 2777ms |
| sw=0.50_dd=0.30_k=80.0 | 0.50 | 0.30 | 80 | 1 | 0.000 | 0.000 | **0.000** | 0.555 | 3053ms |
| sw=0.50_dd=0.50_k=20.0 | 0.50 | 0.50 | 20 | 1 | 0.000 | 0.000 | **0.000** | 0.555 | 3441ms |
| sw=0.50_dd=0.50_k=40.0 | 0.50 | 0.50 | 40 | 1 | 0.000 | 0.000 | **0.000** | 0.555 | 3144ms |
| sw=0.50_dd=0.50_k=60.0 | 0.50 | 0.50 | 60 | 1 | 0.000 | 0.000 | **0.000** | 0.555 | 3168ms |
| sw=0.50_dd=0.50_k=80.0 | 0.50 | 0.50 | 80 | 1 | 0.000 | 0.000 | **0.000** | 0.555 | 2833ms |
| sw=0.50_dd=0.70_k=20.0 | 0.50 | 0.70 | 20 | 1 | 0.000 | 0.000 | **0.000** | 0.555 | 2870ms |
| sw=0.50_dd=0.70_k=40.0 | 0.50 | 0.70 | 40 | 1 | 0.000 | 0.000 | **0.000** | 0.555 | 3256ms |
| sw=0.50_dd=0.70_k=60.0 | 0.50 | 0.70 | 60 | 1 | 0.000 | 0.000 | **0.000** | 0.555 | 2590ms |
| sw=0.50_dd=0.70_k=80.0 | 0.50 | 0.70 | 80 | 1 | 0.000 | 0.000 | **0.000** | 0.555 | 2913ms |
| sw=0.50_dd=1.00_k=20.0 | 0.50 | 1.00 | 20 | 1 | 0.000 | 0.000 | **0.000** | 0.555 | 2756ms |
| sw=0.50_dd=1.00_k=40.0 | 0.50 | 1.00 | 40 | 1 | 0.000 | 0.000 | **0.000** | 0.555 | 3029ms |
| sw=0.50_dd=1.00_k=60.0 | 0.50 | 1.00 | 60 | 1 | 0.000 | 0.000 | **0.000** | 0.555 | 3128ms |
| sw=0.50_dd=1.00_k=80.0 | 0.50 | 1.00 | 80 | 1 | 0.000 | 0.000 | **0.000** | 0.555 | 2826ms |
| sw=0.60_dd=0.30_k=20.0 | 0.60 | 0.30 | 20 | 1 | 0.000 | 0.000 | **0.000** | 0.555 | 2962ms |
| sw=0.60_dd=0.30_k=40.0 | 0.60 | 0.30 | 40 | 1 | 0.000 | 0.000 | **0.000** | 0.555 | 2913ms |
| sw=0.60_dd=0.30_k=60.0 | 0.60 | 0.30 | 60 | 1 | 0.000 | 0.000 | **0.000** | 0.555 | 3091ms |
| sw=0.60_dd=0.30_k=80.0 | 0.60 | 0.30 | 80 | 1 | 0.000 | 0.000 | **0.000** | 0.555 | 2824ms |
| sw=0.60_dd=0.50_k=20.0 | 0.60 | 0.50 | 20 | 1 | 0.000 | 0.000 | **0.000** | 0.555 | 2645ms |
| sw=0.60_dd=0.50_k=40.0 | 0.60 | 0.50 | 40 | 1 | 0.000 | 0.000 | **0.000** | 0.555 | 2787ms |
| sw=0.60_dd=0.50_k=60.0 | 0.60 | 0.50 | 60 | 1 | 0.000 | 0.000 | **0.000** | 0.555 | 2812ms |
| sw=0.60_dd=0.50_k=80.0 | 0.60 | 0.50 | 80 | 1 | 0.000 | 0.000 | **0.000** | 0.555 | 3287ms |
| sw=0.60_dd=0.70_k=20.0 | 0.60 | 0.70 | 20 | 1 | 0.000 | 0.000 | **0.000** | 0.555 | 2645ms |
| sw=0.60_dd=0.70_k=40.0 | 0.60 | 0.70 | 40 | 1 | 0.000 | 0.000 | **0.000** | 0.555 | 2640ms |
| sw=0.60_dd=0.70_k=60.0 | 0.60 | 0.70 | 60 | 1 | 0.000 | 0.000 | **0.000** | 0.555 | 2730ms |
| sw=0.60_dd=0.70_k=80.0 | 0.60 | 0.70 | 80 | 1 | 0.000 | 0.000 | **0.000** | 0.555 | 2695ms |
| sw=0.60_dd=1.00_k=20.0 | 0.60 | 1.00 | 20 | 1 | 0.000 | 0.000 | **0.000** | 0.555 | 2966ms |
| sw=0.60_dd=1.00_k=40.0 | 0.60 | 1.00 | 40 | 1 | 0.000 | 0.000 | **0.000** | 0.555 | 3149ms |
| sw=0.60_dd=1.00_k=60.0 | 0.60 | 1.00 | 60 | 1 | 0.000 | 0.000 | **0.000** | 0.555 | 2649ms |
| sw=0.60_dd=1.00_k=80.0 | 0.60 | 1.00 | 80 | 1 | 0.000 | 0.000 | **0.000** | 0.555 | 2600ms |
| sw=0.70_dd=0.30_k=20.0 | 0.70 | 0.30 | 20 | 1 | 0.000 | 0.000 | **0.000** | 0.555 | 2763ms |
| sw=0.70_dd=0.30_k=40.0 | 0.70 | 0.30 | 40 | 1 | 0.000 | 0.000 | **0.000** | 0.555 | 3217ms |
| sw=0.70_dd=0.30_k=60.0 | 0.70 | 0.30 | 60 | 1 | 0.000 | 0.000 | **0.000** | 0.555 | 3067ms |
| sw=0.70_dd=0.30_k=80.0 | 0.70 | 0.30 | 80 | 1 | 0.000 | 0.000 | **0.000** | 0.555 | 2791ms |
| sw=0.70_dd=0.50_k=20.0 | 0.70 | 0.50 | 20 | 1 | 0.000 | 0.000 | **0.000** | 0.555 | 2679ms |
| sw=0.70_dd=0.50_k=40.0 | 0.70 | 0.50 | 40 | 1 | 0.000 | 0.000 | **0.000** | 0.555 | 3026ms |
| sw=0.70_dd=0.50_k=60.0 | 0.70 | 0.50 | 60 | 1 | 0.000 | 0.000 | **0.000** | 0.555 | 3346ms |
| sw=0.70_dd=0.50_k=80.0 | 0.70 | 0.50 | 80 | 1 | 0.000 | 0.000 | **0.000** | 0.555 | 3120ms |
| sw=0.70_dd=0.70_k=20.0 | 0.70 | 0.70 | 20 | 1 | 0.000 | 0.000 | **0.000** | 0.555 | 3013ms |
| sw=0.70_dd=0.70_k=40.0 | 0.70 | 0.70 | 40 | 1 | 0.000 | 0.000 | **0.000** | 0.555 | 2764ms |
| sw=0.70_dd=0.70_k=60.0 | 0.70 | 0.70 | 60 | 1 | 0.000 | 0.000 | **0.000** | 0.555 | 2960ms |
| sw=0.70_dd=0.70_k=80.0 | 0.70 | 0.70 | 80 | 1 | 0.000 | 0.000 | **0.000** | 0.555 | 3017ms |
| sw=0.70_dd=1.00_k=20.0 | 0.70 | 1.00 | 20 | 1 | 0.000 | 0.000 | **0.000** | 0.555 | 2745ms |
| sw=0.70_dd=1.00_k=40.0 | 0.70 | 1.00 | 40 | 1 | 0.000 | 0.000 | **0.000** | 0.555 | 2749ms |
| sw=0.70_dd=1.00_k=60.0 | 0.70 | 1.00 | 60 | 1 | 0.000 | 0.000 | **0.000** | 0.555 | 2816ms |
| sw=0.70_dd=1.00_k=80.0 | 0.70 | 1.00 | 80 | 1 | 0.000 | 0.000 | **0.000** | 0.555 | 2812ms |
| sw=0.80_dd=0.30_k=20.0 | 0.80 | 0.30 | 20 | 1 | 0.000 | 0.000 | **0.000** | 0.555 | 2884ms |
| sw=0.80_dd=0.30_k=40.0 | 0.80 | 0.30 | 40 | 1 | 0.000 | 0.000 | **0.000** | 0.555 | 2961ms |
| sw=0.80_dd=0.30_k=60.0 | 0.80 | 0.30 | 60 | 1 | 0.000 | 0.000 | **0.000** | 0.555 | 2842ms |
| sw=0.80_dd=0.30_k=80.0 | 0.80 | 0.30 | 80 | 1 | 0.000 | 0.000 | **0.000** | 0.555 | 2590ms |
| sw=0.80_dd=0.50_k=20.0 | 0.80 | 0.50 | 20 | 1 | 0.000 | 0.000 | **0.000** | 0.555 | 2602ms |
| sw=0.80_dd=0.50_k=40.0 | 0.80 | 0.50 | 40 | 1 | 0.000 | 0.000 | **0.000** | 0.555 | 2840ms |
| sw=0.80_dd=0.50_k=60.0 | 0.80 | 0.50 | 60 | 1 | 0.000 | 0.000 | **0.000** | 0.555 | 2964ms |
| sw=0.80_dd=0.50_k=80.0 | 0.80 | 0.50 | 80 | 1 | 0.000 | 0.000 | **0.000** | 0.555 | 2712ms |
| sw=0.80_dd=0.70_k=20.0 | 0.80 | 0.70 | 20 | 1 | 0.000 | 0.000 | **0.000** | 0.555 | 2865ms |
| sw=0.80_dd=0.70_k=40.0 | 0.80 | 0.70 | 40 | 1 | 0.000 | 0.000 | **0.000** | 0.555 | 2635ms |
| sw=0.80_dd=0.70_k=60.0 | 0.80 | 0.70 | 60 | 1 | 0.000 | 0.000 | **0.000** | 0.555 | 2722ms |
| sw=0.80_dd=0.70_k=80.0 | 0.80 | 0.70 | 80 | 1 | 0.000 | 0.000 | **0.000** | 0.555 | 2717ms |
| sw=0.80_dd=1.00_k=20.0 | 0.80 | 1.00 | 20 | 1 | 0.000 | 0.000 | **0.000** | 0.555 | 2686ms |
| sw=0.80_dd=1.00_k=40.0 | 0.80 | 1.00 | 40 | 1 | 0.000 | 0.000 | **0.000** | 0.555 | 2743ms |
| sw=0.80_dd=1.00_k=60.0 | 0.80 | 1.00 | 60 | 1 | 0.000 | 0.000 | **0.000** | 0.555 | 2631ms |
| sw=0.80_dd=1.00_k=80.0 | 0.80 | 1.00 | 80 | 1 | 0.000 | 0.000 | **0.000** | 0.555 | 2665ms |
| sw=0.90_dd=0.30_k=20.0 | 0.90 | 0.30 | 20 | 1 | 0.000 | 0.000 | **0.000** | 0.555 | 2863ms |
| sw=0.90_dd=0.30_k=40.0 | 0.90 | 0.30 | 40 | 1 | 0.000 | 0.000 | **0.000** | 0.555 | 2785ms |
| sw=0.90_dd=0.30_k=60.0 | 0.90 | 0.30 | 60 | 1 | 0.000 | 0.000 | **0.000** | 0.555 | 2554ms |
| sw=0.90_dd=0.30_k=80.0 | 0.90 | 0.30 | 80 | 1 | 0.000 | 0.000 | **0.000** | 0.555 | 3094ms |
| sw=0.90_dd=0.50_k=20.0 | 0.90 | 0.50 | 20 | 1 | 0.000 | 0.000 | **0.000** | 0.555 | 3123ms |
| sw=0.90_dd=0.50_k=40.0 | 0.90 | 0.50 | 40 | 1 | 0.000 | 0.000 | **0.000** | 0.555 | 2592ms |
| sw=0.90_dd=0.50_k=60.0 | 0.90 | 0.50 | 60 | 1 | 0.000 | 0.000 | **0.000** | 0.555 | 2673ms |
| sw=0.90_dd=0.50_k=80.0 | 0.90 | 0.50 | 80 | 1 | 0.000 | 0.000 | **0.000** | 0.555 | 2784ms |
| sw=0.90_dd=0.70_k=20.0 | 0.90 | 0.70 | 20 | 1 | 0.000 | 0.000 | **0.000** | 0.555 | 2670ms |
| sw=0.90_dd=0.70_k=40.0 | 0.90 | 0.70 | 40 | 1 | 0.000 | 0.000 | **0.000** | 0.555 | 2774ms |
| sw=0.90_dd=0.70_k=60.0 | 0.90 | 0.70 | 60 | 1 | 0.000 | 0.000 | **0.000** | 0.555 | 2724ms |
| sw=0.90_dd=0.70_k=80.0 | 0.90 | 0.70 | 80 | 1 | 0.000 | 0.000 | **0.000** | 0.555 | 2752ms |
| sw=0.90_dd=1.00_k=20.0 | 0.90 | 1.00 | 20 | 1 | 0.000 | 0.000 | **0.000** | 0.555 | 3000ms |
| sw=0.90_dd=1.00_k=40.0 | 0.90 | 1.00 | 40 | 1 | 0.000 | 0.000 | **0.000** | 0.555 | 2750ms |
| sw=0.90_dd=1.00_k=60.0 | 0.90 | 1.00 | 60 | 1 | 0.000 | 0.000 | **0.000** | 0.555 | 3052ms |
| sw=0.90_dd=1.00_k=80.0 | 0.90 | 1.00 | 80 | 1 | 0.000 | 0.000 | **0.000** | 0.555 | 2905ms |

## Best Config: sw=0.50_dd=0.30_k=20.0
- F1: 0.0000
- Precision: 0.0000
- Recall: 0.0000

## Per-Task Results


### ruff-002

Ground truth files: crates/ruff_linter/resources/test/fixtures/flake8_boolean_trap/FBT.py, crates/ruff_linter/src/rules/flake8_boolean_trap/helpers.rs, crates/ruff_linter/src/rules/flake8_boolean_trap/snapshots/ruff_linter__rules__flake8_boolean_trap__tests__FBT001_FBT.py.snap, crates/ruff_linter/src/rules/flake8_boolean_trap/snapshots/ruff_linter__rules__flake8_boolean_trap__tests__FBT003_FBT.py.snap, crates/ruff_linter/src/rules/flake8_boolean_trap/snapshots/ruff_linter__rules__flake8_boolean_trap__tests__extend_allowed_callable.snap

| Config | Precision | Recall | F1 | Returned Files |
|--------|-----------|--------|----|----------------|
| sw=0.50_dd=0.30_k=20.0 | 0.000 | 0.000 | 0.000 | crates/ruff_linter/src/rules/flake8_boolean_trap/mod.rs, crates/ruff_linter/src/rules/mod.rs, crates/ruff_workspace/src/options.rs, crates/ruff_linter/src/rules/flake8_boolean_trap/settings.rs, crates/ruff_linter/src/rules/flake8_boolean_trap/rules/boolean_positional_value_in_call.rs (+2 more) |
| sw=0.50_dd=0.30_k=40.0 | 0.000 | 0.000 | 0.000 | crates/ruff_linter/src/rules/flake8_boolean_trap/mod.rs, crates/ruff_linter/src/rules/mod.rs, crates/ruff_workspace/src/options.rs, crates/ruff_linter/src/rules/flake8_boolean_trap/settings.rs, crates/ruff_linter/src/rules/flake8_boolean_trap/rules/boolean_positional_value_in_call.rs (+1 more) |
| sw=0.50_dd=0.30_k=60.0 | 0.000 | 0.000 | 0.000 | crates/ruff_linter/src/rules/flake8_boolean_trap/mod.rs, crates/ruff_linter/src/rules/mod.rs, crates/ruff_workspace/src/options.rs, crates/ruff_linter/src/rules/flake8_boolean_trap/settings.rs, crates/ruff_linter/src/rules/flake8_boolean_trap/rules/boolean_positional_value_in_call.rs (+1 more) |
| sw=0.50_dd=0.30_k=80.0 | 0.000 | 0.000 | 0.000 | crates/ruff_linter/src/rules/flake8_boolean_trap/mod.rs, crates/ruff_linter/src/rules/mod.rs, crates/ruff_workspace/src/options.rs, crates/ruff_linter/src/rules/flake8_boolean_trap/settings.rs, crates/ruff_linter/src/rules/flake8_boolean_trap/rules/boolean_positional_value_in_call.rs (+1 more) |
| sw=0.50_dd=0.50_k=20.0 | 0.000 | 0.000 | 0.000 | crates/ruff_linter/src/rules/flake8_boolean_trap/mod.rs, crates/ruff_linter/src/rules/mod.rs, crates/ruff_workspace/src/options.rs, crates/ruff_linter/src/rules/flake8_boolean_trap/settings.rs, changelogs/0.3.x.md (+2 more) |
| sw=0.50_dd=0.50_k=40.0 | 0.000 | 0.000 | 0.000 | crates/ruff_linter/src/rules/flake8_boolean_trap/mod.rs, crates/ruff_linter/src/rules/mod.rs, crates/ruff_workspace/src/options.rs, crates/ruff_linter/src/rules/flake8_boolean_trap/settings.rs, crates/ruff_linter/src/rules/flake8_boolean_trap/rules/boolean_positional_value_in_call.rs (+2 more) |
| sw=0.50_dd=0.50_k=60.0 | 0.000 | 0.000 | 0.000 | crates/ruff_linter/src/rules/flake8_boolean_trap/mod.rs, crates/ruff_linter/src/rules/mod.rs, crates/ruff_workspace/src/options.rs, crates/ruff_linter/src/rules/flake8_boolean_trap/settings.rs, crates/ruff_linter/src/rules/flake8_boolean_trap/rules/boolean_positional_value_in_call.rs (+2 more) |
| sw=0.50_dd=0.50_k=80.0 | 0.000 | 0.000 | 0.000 | crates/ruff_linter/src/rules/flake8_boolean_trap/mod.rs, crates/ruff_linter/src/rules/mod.rs, crates/ruff_workspace/src/options.rs, crates/ruff_linter/src/rules/flake8_boolean_trap/settings.rs, crates/ruff_linter/src/rules/flake8_boolean_trap/rules/boolean_positional_value_in_call.rs (+2 more) |
| sw=0.50_dd=0.70_k=20.0 | 0.000 | 0.000 | 0.000 | crates/ruff_linter/src/rules/flake8_boolean_trap/mod.rs, crates/ruff_linter/src/rules/mod.rs, crates/ruff_workspace/src/options.rs, changelogs/0.3.x.md, crates/ruff_linter/src/rules/flake8_boolean_trap/settings.rs (+3 more) |
| sw=0.50_dd=0.70_k=40.0 | 0.000 | 0.000 | 0.000 | crates/ruff_linter/src/rules/flake8_boolean_trap/mod.rs, crates/ruff_linter/src/rules/mod.rs, crates/ruff_workspace/src/options.rs, crates/ruff_linter/src/rules/flake8_boolean_trap/settings.rs, changelogs/0.3.x.md (+3 more) |
| sw=0.50_dd=0.70_k=60.0 | 0.000 | 0.000 | 0.000 | crates/ruff_linter/src/rules/flake8_boolean_trap/mod.rs, crates/ruff_linter/src/rules/mod.rs, crates/ruff_workspace/src/options.rs, crates/ruff_linter/src/rules/flake8_boolean_trap/settings.rs, crates/ruff_linter/src/rules/flake8_boolean_trap/rules/boolean_positional_value_in_call.rs (+5 more) |
| sw=0.50_dd=0.70_k=80.0 | 0.000 | 0.000 | 0.000 | crates/ruff_linter/src/rules/flake8_boolean_trap/mod.rs, crates/ruff_linter/src/rules/mod.rs, crates/ruff_workspace/src/options.rs, crates/ruff_linter/src/rules/flake8_boolean_trap/settings.rs, crates/ruff_linter/src/rules/flake8_boolean_trap/rules/boolean_positional_value_in_call.rs (+5 more) |
| sw=0.50_dd=1.00_k=20.0 | 0.000 | 0.000 | 0.000 | crates/ruff_linter/src/rules/flake8_boolean_trap/mod.rs, crates/ruff_linter/src/rules/mod.rs, changelogs/0.3.x.md, crates/ruff_workspace/src/options.rs, changelogs/0.4.x.md (+6 more) |
| sw=0.50_dd=1.00_k=40.0 | 0.000 | 0.000 | 0.000 | crates/ruff_linter/src/rules/flake8_boolean_trap/mod.rs, crates/ruff_linter/src/rules/mod.rs, changelogs/0.3.x.md, crates/ruff_workspace/src/options.rs, changelogs/0.4.x.md (+6 more) |
| sw=0.50_dd=1.00_k=60.0 | 0.000 | 0.000 | 0.000 | crates/ruff_linter/src/rules/flake8_boolean_trap/mod.rs, crates/ruff_linter/src/rules/mod.rs, changelogs/0.3.x.md, crates/ruff_workspace/src/options.rs, changelogs/0.4.x.md (+6 more) |
| sw=0.50_dd=1.00_k=80.0 | 0.000 | 0.000 | 0.000 | crates/ruff_linter/src/rules/flake8_boolean_trap/mod.rs, crates/ruff_linter/src/rules/mod.rs, changelogs/0.3.x.md, crates/ruff_workspace/src/options.rs, changelogs/0.4.x.md (+6 more) |
| sw=0.60_dd=0.30_k=20.0 | 0.000 | 0.000 | 0.000 | crates/ruff_linter/src/rules/flake8_boolean_trap/mod.rs, crates/ruff_linter/src/rules/mod.rs, crates/ruff_workspace/src/options.rs, crates/ruff_linter/src/rules/flake8_annotations/mod.rs, crates/ruff_linter/src/rules/flake8_boolean_trap/settings.rs (+1 more) |
| sw=0.60_dd=0.30_k=40.0 | 0.000 | 0.000 | 0.000 | crates/ruff_linter/src/rules/flake8_boolean_trap/mod.rs, crates/ruff_linter/src/rules/mod.rs, crates/ruff_workspace/src/options.rs, crates/ruff_linter/src/rules/flake8_boolean_trap/settings.rs, crates/ruff_linter/src/rules/flake8_annotations/mod.rs (+1 more) |
| sw=0.60_dd=0.30_k=60.0 | 0.000 | 0.000 | 0.000 | crates/ruff_linter/src/rules/flake8_boolean_trap/mod.rs, crates/ruff_linter/src/rules/mod.rs, crates/ruff_workspace/src/options.rs, crates/ruff_linter/src/rules/flake8_boolean_trap/settings.rs, crates/ruff_linter/src/rules/flake8_boolean_trap/rules/boolean_positional_value_in_call.rs (+1 more) |
| sw=0.60_dd=0.30_k=80.0 | 0.000 | 0.000 | 0.000 | crates/ruff_linter/src/rules/flake8_boolean_trap/mod.rs, crates/ruff_linter/src/rules/mod.rs, crates/ruff_workspace/src/options.rs, crates/ruff_linter/src/rules/flake8_boolean_trap/settings.rs, crates/ruff_linter/src/rules/flake8_boolean_trap/rules/boolean_positional_value_in_call.rs (+1 more) |
| sw=0.60_dd=0.50_k=20.0 | 0.000 | 0.000 | 0.000 | crates/ruff_linter/src/rules/flake8_boolean_trap/mod.rs, crates/ruff_linter/src/rules/mod.rs, crates/ruff_workspace/src/options.rs, crates/ruff_linter/src/rules/flake8_annotations/mod.rs, crates/ruff_linter/src/rules/flake8_boolean_trap/settings.rs (+2 more) |
| sw=0.60_dd=0.50_k=40.0 | 0.000 | 0.000 | 0.000 | crates/ruff_linter/src/rules/flake8_boolean_trap/mod.rs, crates/ruff_linter/src/rules/mod.rs, crates/ruff_workspace/src/options.rs, crates/ruff_linter/src/rules/flake8_boolean_trap/settings.rs, crates/ruff_linter/src/rules/flake8_annotations/mod.rs (+1 more) |
| sw=0.60_dd=0.50_k=60.0 | 0.000 | 0.000 | 0.000 | crates/ruff_linter/src/rules/flake8_boolean_trap/mod.rs, crates/ruff_linter/src/rules/mod.rs, crates/ruff_workspace/src/options.rs, crates/ruff_linter/src/rules/flake8_boolean_trap/settings.rs, crates/ruff_linter/src/rules/flake8_boolean_trap/rules/boolean_positional_value_in_call.rs (+1 more) |
| sw=0.60_dd=0.50_k=80.0 | 0.000 | 0.000 | 0.000 | crates/ruff_linter/src/rules/flake8_boolean_trap/mod.rs, crates/ruff_linter/src/rules/mod.rs, crates/ruff_workspace/src/options.rs, crates/ruff_linter/src/rules/flake8_boolean_trap/settings.rs, crates/ruff_linter/src/rules/flake8_boolean_trap/rules/boolean_positional_value_in_call.rs (+1 more) |
| sw=0.60_dd=0.70_k=20.0 | 0.000 | 0.000 | 0.000 | crates/ruff_linter/src/rules/flake8_boolean_trap/mod.rs, crates/ruff_linter/src/rules/mod.rs, crates/ruff_workspace/src/options.rs, changelogs/0.3.x.md, crates/ruff_linter/src/rules/flake8_annotations/mod.rs (+3 more) |
| sw=0.60_dd=0.70_k=40.0 | 0.000 | 0.000 | 0.000 | crates/ruff_linter/src/rules/flake8_boolean_trap/mod.rs, crates/ruff_linter/src/rules/mod.rs, crates/ruff_workspace/src/options.rs, crates/ruff_linter/src/rules/flake8_boolean_trap/settings.rs, changelogs/0.3.x.md (+2 more) |
| sw=0.60_dd=0.70_k=60.0 | 0.000 | 0.000 | 0.000 | crates/ruff_linter/src/rules/flake8_boolean_trap/mod.rs, crates/ruff_linter/src/rules/mod.rs, crates/ruff_workspace/src/options.rs, crates/ruff_linter/src/rules/flake8_boolean_trap/settings.rs, crates/ruff_linter/src/rules/flake8_boolean_trap/rules/boolean_positional_value_in_call.rs (+2 more) |
| sw=0.60_dd=0.70_k=80.0 | 0.000 | 0.000 | 0.000 | crates/ruff_linter/src/rules/flake8_boolean_trap/mod.rs, crates/ruff_linter/src/rules/mod.rs, crates/ruff_workspace/src/options.rs, crates/ruff_linter/src/rules/flake8_boolean_trap/settings.rs, crates/ruff_linter/src/rules/flake8_boolean_trap/rules/boolean_positional_value_in_call.rs (+2 more) |
| sw=0.60_dd=1.00_k=20.0 | 0.000 | 0.000 | 0.000 | crates/ruff_linter/src/rules/flake8_boolean_trap/mod.rs, crates/ruff_linter/src/rules/mod.rs, changelogs/0.3.x.md, crates/ruff_workspace/src/options.rs, changelogs/0.4.x.md (+3 more) |
| sw=0.60_dd=1.00_k=40.0 | 0.000 | 0.000 | 0.000 | crates/ruff_linter/src/rules/flake8_boolean_trap/mod.rs, crates/ruff_linter/src/rules/mod.rs, changelogs/0.3.x.md, crates/ruff_workspace/src/options.rs, changelogs/0.4.x.md (+4 more) |
| sw=0.60_dd=1.00_k=60.0 | 0.000 | 0.000 | 0.000 | crates/ruff_linter/src/rules/flake8_boolean_trap/mod.rs, crates/ruff_linter/src/rules/mod.rs, changelogs/0.3.x.md, crates/ruff_workspace/src/options.rs, changelogs/0.4.x.md (+3 more) |
| sw=0.60_dd=1.00_k=80.0 | 0.000 | 0.000 | 0.000 | crates/ruff_linter/src/rules/flake8_boolean_trap/mod.rs, crates/ruff_linter/src/rules/mod.rs, changelogs/0.3.x.md, crates/ruff_workspace/src/options.rs, changelogs/0.4.x.md (+6 more) |
| sw=0.70_dd=0.30_k=20.0 | 0.000 | 0.000 | 0.000 | crates/ruff_linter/src/rules/flake8_boolean_trap/mod.rs, crates/ruff_linter/src/rules/mod.rs, crates/ruff_linter/src/rules/flake8_annotations/mod.rs, crates/ruff_workspace/src/options.rs, crates/ty_ide/src/completion.rs |
| sw=0.70_dd=0.30_k=40.0 | 0.000 | 0.000 | 0.000 | crates/ruff_linter/src/rules/flake8_boolean_trap/mod.rs, crates/ruff_linter/src/rules/mod.rs, crates/ruff_linter/src/rules/flake8_annotations/mod.rs, crates/ruff_workspace/src/options.rs, crates/ty_ide/src/completion.rs |
| sw=0.70_dd=0.30_k=60.0 | 0.000 | 0.000 | 0.000 | crates/ruff_linter/src/rules/flake8_boolean_trap/mod.rs, crates/ruff_linter/src/rules/mod.rs, crates/ruff_linter/src/rules/flake8_annotations/mod.rs, crates/ruff_workspace/src/options.rs, crates/ty_ide/src/completion.rs |
| sw=0.70_dd=0.30_k=80.0 | 0.000 | 0.000 | 0.000 | crates/ruff_linter/src/rules/flake8_boolean_trap/mod.rs, crates/ruff_linter/src/rules/mod.rs, crates/ruff_linter/src/rules/flake8_annotations/mod.rs, crates/ruff_workspace/src/options.rs, crates/ty_ide/src/completion.rs |
| sw=0.70_dd=0.50_k=20.0 | 0.000 | 0.000 | 0.000 | crates/ruff_linter/src/rules/flake8_boolean_trap/mod.rs, crates/ruff_linter/src/rules/mod.rs, crates/ruff_linter/src/rules/flake8_annotations/mod.rs, crates/ruff_workspace/src/options.rs, crates/ty_ide/src/completion.rs |
| sw=0.70_dd=0.50_k=40.0 | 0.000 | 0.000 | 0.000 | crates/ruff_linter/src/rules/flake8_boolean_trap/mod.rs, crates/ruff_linter/src/rules/mod.rs, crates/ruff_linter/src/rules/flake8_annotations/mod.rs, crates/ruff_workspace/src/options.rs, crates/ty_ide/src/completion.rs |
| sw=0.70_dd=0.50_k=60.0 | 0.000 | 0.000 | 0.000 | crates/ruff_linter/src/rules/flake8_boolean_trap/mod.rs, crates/ruff_linter/src/rules/mod.rs, crates/ruff_linter/src/rules/flake8_annotations/mod.rs, crates/ruff_workspace/src/options.rs, crates/ty_ide/src/completion.rs |
| sw=0.70_dd=0.50_k=80.0 | 0.000 | 0.000 | 0.000 | crates/ruff_linter/src/rules/flake8_boolean_trap/mod.rs, crates/ruff_linter/src/rules/mod.rs, crates/ruff_linter/src/rules/flake8_annotations/mod.rs, crates/ruff_workspace/src/options.rs, crates/ty_ide/src/completion.rs |
| sw=0.70_dd=0.70_k=20.0 | 0.000 | 0.000 | 0.000 | crates/ruff_linter/src/rules/flake8_boolean_trap/mod.rs, crates/ruff_linter/src/rules/mod.rs, crates/ruff_linter/src/rules/flake8_annotations/mod.rs, crates/ruff_workspace/src/options.rs, crates/ty_ide/src/completion.rs (+7 more) |
| sw=0.70_dd=0.70_k=40.0 | 0.000 | 0.000 | 0.000 | crates/ruff_linter/src/rules/flake8_boolean_trap/mod.rs, crates/ruff_linter/src/rules/mod.rs, crates/ruff_linter/src/rules/flake8_annotations/mod.rs, crates/ruff_workspace/src/options.rs, crates/ty_ide/src/completion.rs |
| sw=0.70_dd=0.70_k=60.0 | 0.000 | 0.000 | 0.000 | crates/ruff_linter/src/rules/flake8_boolean_trap/mod.rs, crates/ruff_linter/src/rules/mod.rs, crates/ruff_linter/src/rules/flake8_annotations/mod.rs, crates/ruff_workspace/src/options.rs, crates/ty_ide/src/completion.rs |
| sw=0.70_dd=0.70_k=80.0 | 0.000 | 0.000 | 0.000 | crates/ruff_linter/src/rules/flake8_boolean_trap/mod.rs, crates/ruff_linter/src/rules/mod.rs, crates/ruff_linter/src/rules/flake8_annotations/mod.rs, crates/ruff_workspace/src/options.rs, crates/ty_ide/src/completion.rs |
| sw=0.70_dd=1.00_k=20.0 | 0.000 | 0.000 | 0.000 | crates/ruff_linter/src/rules/flake8_boolean_trap/mod.rs, crates/ruff_linter/src/rules/mod.rs, changelogs/0.3.x.md, crates/ruff_linter/src/rules/flake8_annotations/mod.rs, crates/ruff_workspace/src/options.rs (+9 more) |
| sw=0.70_dd=1.00_k=40.0 | 0.000 | 0.000 | 0.000 | crates/ruff_linter/src/rules/flake8_boolean_trap/mod.rs, crates/ruff_linter/src/rules/mod.rs, crates/ruff_linter/src/rules/flake8_annotations/mod.rs, crates/ruff_workspace/src/options.rs, changelogs/0.3.x.md (+8 more) |
| sw=0.70_dd=1.00_k=60.0 | 0.000 | 0.000 | 0.000 | crates/ruff_linter/src/rules/flake8_boolean_trap/mod.rs, crates/ruff_linter/src/rules/mod.rs, crates/ruff_linter/src/rules/flake8_annotations/mod.rs, crates/ruff_workspace/src/options.rs, crates/ty_ide/src/completion.rs (+9 more) |
| sw=0.70_dd=1.00_k=80.0 | 0.000 | 0.000 | 0.000 | crates/ruff_linter/src/rules/flake8_boolean_trap/mod.rs, crates/ruff_linter/src/rules/mod.rs, crates/ruff_linter/src/rules/flake8_annotations/mod.rs, crates/ruff_workspace/src/options.rs, crates/ty_ide/src/completion.rs (+9 more) |
| sw=0.80_dd=0.30_k=20.0 | 0.000 | 0.000 | 0.000 | crates/ruff_linter/src/rules/mod.rs, crates/ruff_linter/src/rules/flake8_boolean_trap/mod.rs, crates/ruff_linter/src/rules/flake8_annotations/mod.rs, crates/ruff_workspace/src/options.rs, crates/ty_ide/src/completion.rs |
| sw=0.80_dd=0.30_k=40.0 | 0.000 | 0.000 | 0.000 | crates/ruff_linter/src/rules/mod.rs, crates/ruff_linter/src/rules/flake8_boolean_trap/mod.rs, crates/ruff_linter/src/rules/flake8_annotations/mod.rs, crates/ruff_workspace/src/options.rs, crates/ty_ide/src/completion.rs |
| sw=0.80_dd=0.30_k=60.0 | 0.000 | 0.000 | 0.000 | crates/ruff_linter/src/rules/mod.rs, crates/ruff_linter/src/rules/flake8_boolean_trap/mod.rs, crates/ruff_linter/src/rules/flake8_annotations/mod.rs, crates/ruff_workspace/src/options.rs, crates/ty_ide/src/completion.rs |
| sw=0.80_dd=0.30_k=80.0 | 0.000 | 0.000 | 0.000 | crates/ruff_linter/src/rules/mod.rs, crates/ruff_linter/src/rules/flake8_boolean_trap/mod.rs, crates/ruff_linter/src/rules/flake8_annotations/mod.rs, crates/ruff_workspace/src/options.rs, crates/ty_ide/src/completion.rs |
| sw=0.80_dd=0.50_k=20.0 | 0.000 | 0.000 | 0.000 | crates/ruff_linter/src/rules/mod.rs, crates/ruff_linter/src/rules/flake8_boolean_trap/mod.rs, crates/ruff_linter/src/rules/flake8_annotations/mod.rs, crates/ruff_workspace/src/options.rs, crates/ty_ide/src/completion.rs (+9 more) |
| sw=0.80_dd=0.50_k=40.0 | 0.000 | 0.000 | 0.000 | crates/ruff_linter/src/rules/mod.rs, crates/ruff_linter/src/rules/flake8_boolean_trap/mod.rs, crates/ruff_linter/src/rules/flake8_annotations/mod.rs, crates/ruff_workspace/src/options.rs, crates/ty_ide/src/completion.rs (+9 more) |
| sw=0.80_dd=0.50_k=60.0 | 0.000 | 0.000 | 0.000 | crates/ruff_linter/src/rules/mod.rs, crates/ruff_linter/src/rules/flake8_boolean_trap/mod.rs, crates/ruff_linter/src/rules/flake8_annotations/mod.rs, crates/ruff_workspace/src/options.rs, crates/ty_ide/src/completion.rs (+9 more) |
| sw=0.80_dd=0.50_k=80.0 | 0.000 | 0.000 | 0.000 | crates/ruff_linter/src/rules/mod.rs, crates/ruff_linter/src/rules/flake8_boolean_trap/mod.rs, crates/ruff_linter/src/rules/flake8_annotations/mod.rs, crates/ruff_workspace/src/options.rs, crates/ty_ide/src/completion.rs (+9 more) |
| sw=0.80_dd=0.70_k=20.0 | 0.000 | 0.000 | 0.000 | crates/ruff_linter/src/rules/mod.rs, crates/ruff_linter/src/rules/flake8_boolean_trap/mod.rs, crates/ruff_linter/src/rules/flake8_annotations/mod.rs, crates/ruff_workspace/src/options.rs, crates/ty_ide/src/completion.rs (+9 more) |
| sw=0.80_dd=0.70_k=40.0 | 0.000 | 0.000 | 0.000 | crates/ruff_linter/src/rules/mod.rs, crates/ruff_linter/src/rules/flake8_boolean_trap/mod.rs, crates/ruff_linter/src/rules/flake8_annotations/mod.rs, crates/ruff_workspace/src/options.rs, crates/ty_ide/src/completion.rs (+10 more) |
| sw=0.80_dd=0.70_k=60.0 | 0.000 | 0.000 | 0.000 | crates/ruff_linter/src/rules/mod.rs, crates/ruff_linter/src/rules/flake8_boolean_trap/mod.rs, crates/ruff_linter/src/rules/flake8_annotations/mod.rs, crates/ruff_workspace/src/options.rs, crates/ty_ide/src/completion.rs (+11 more) |
| sw=0.80_dd=0.70_k=80.0 | 0.000 | 0.000 | 0.000 | crates/ruff_linter/src/rules/mod.rs, crates/ruff_linter/src/rules/flake8_boolean_trap/mod.rs, crates/ruff_linter/src/rules/flake8_annotations/mod.rs, crates/ruff_workspace/src/options.rs, crates/ty_ide/src/completion.rs (+12 more) |
| sw=0.80_dd=1.00_k=20.0 | 0.000 | 0.000 | 0.000 | crates/ruff_linter/src/rules/mod.rs, crates/ruff_linter/src/rules/flake8_boolean_trap/mod.rs, crates/ruff_linter/src/rules/flake8_annotations/mod.rs, crates/ruff_workspace/src/options.rs, crates/ty_ide/src/completion.rs (+9 more) |
| sw=0.80_dd=1.00_k=40.0 | 0.000 | 0.000 | 0.000 | crates/ruff_linter/src/rules/mod.rs, crates/ruff_linter/src/rules/flake8_boolean_trap/mod.rs, crates/ruff_linter/src/rules/flake8_annotations/mod.rs, crates/ruff_workspace/src/options.rs, crates/ty_ide/src/completion.rs (+12 more) |
| sw=0.80_dd=1.00_k=60.0 | 0.000 | 0.000 | 0.000 | crates/ruff_linter/src/rules/mod.rs, crates/ruff_linter/src/rules/flake8_boolean_trap/mod.rs, crates/ruff_linter/src/rules/flake8_annotations/mod.rs, crates/ruff_workspace/src/options.rs, crates/ty_ide/src/completion.rs (+12 more) |
| sw=0.80_dd=1.00_k=80.0 | 0.000 | 0.000 | 0.000 | crates/ruff_linter/src/rules/mod.rs, crates/ruff_linter/src/rules/flake8_boolean_trap/mod.rs, crates/ruff_linter/src/rules/flake8_annotations/mod.rs, crates/ruff_workspace/src/options.rs, crates/ty_ide/src/completion.rs (+12 more) |
| sw=0.90_dd=0.30_k=20.0 | 0.000 | 0.000 | 0.000 | crates/ruff_linter/src/rules/mod.rs, crates/ruff_linter/src/rules/flake8_boolean_trap/mod.rs, crates/ruff_linter/src/rules/flake8_annotations/mod.rs, crates/ruff_workspace/src/options.rs, crates/ty_ide/src/completion.rs (+9 more) |
| sw=0.90_dd=0.30_k=40.0 | 0.000 | 0.000 | 0.000 | crates/ruff_linter/src/rules/mod.rs, crates/ruff_linter/src/rules/flake8_boolean_trap/mod.rs, crates/ruff_linter/src/rules/flake8_annotations/mod.rs, crates/ruff_workspace/src/options.rs, crates/ty_ide/src/completion.rs (+9 more) |
| sw=0.90_dd=0.30_k=60.0 | 0.000 | 0.000 | 0.000 | crates/ruff_linter/src/rules/mod.rs, crates/ruff_linter/src/rules/flake8_boolean_trap/mod.rs, crates/ruff_linter/src/rules/flake8_annotations/mod.rs, crates/ruff_workspace/src/options.rs, crates/ty_ide/src/completion.rs (+10 more) |
| sw=0.90_dd=0.30_k=80.0 | 0.000 | 0.000 | 0.000 | crates/ruff_linter/src/rules/mod.rs, crates/ruff_linter/src/rules/flake8_boolean_trap/mod.rs, crates/ruff_linter/src/rules/flake8_annotations/mod.rs, crates/ruff_workspace/src/options.rs, crates/ty_ide/src/completion.rs (+11 more) |
| sw=0.90_dd=0.50_k=20.0 | 0.000 | 0.000 | 0.000 | crates/ruff_linter/src/rules/mod.rs, crates/ruff_linter/src/rules/flake8_boolean_trap/mod.rs, crates/ruff_linter/src/rules/flake8_annotations/mod.rs, crates/ruff_workspace/src/options.rs, crates/ty_ide/src/completion.rs (+12 more) |
| sw=0.90_dd=0.50_k=40.0 | 0.000 | 0.000 | 0.000 | crates/ruff_linter/src/rules/mod.rs, crates/ruff_linter/src/rules/flake8_boolean_trap/mod.rs, crates/ruff_linter/src/rules/flake8_annotations/mod.rs, crates/ruff_workspace/src/options.rs, crates/ty_ide/src/completion.rs (+12 more) |
| sw=0.90_dd=0.50_k=60.0 | 0.000 | 0.000 | 0.000 | crates/ruff_linter/src/rules/mod.rs, crates/ruff_linter/src/rules/flake8_boolean_trap/mod.rs, crates/ruff_linter/src/rules/flake8_annotations/mod.rs, crates/ruff_workspace/src/options.rs, crates/ty_ide/src/completion.rs (+12 more) |
| sw=0.90_dd=0.50_k=80.0 | 0.000 | 0.000 | 0.000 | crates/ruff_linter/src/rules/mod.rs, crates/ruff_linter/src/rules/flake8_boolean_trap/mod.rs, crates/ruff_linter/src/rules/flake8_annotations/mod.rs, crates/ruff_workspace/src/options.rs, crates/ty_ide/src/completion.rs (+12 more) |
| sw=0.90_dd=0.70_k=20.0 | 0.000 | 0.000 | 0.000 | crates/ruff_linter/src/rules/mod.rs, crates/ruff_linter/src/rules/flake8_boolean_trap/mod.rs, crates/ruff_linter/src/rules/flake8_annotations/mod.rs, crates/ruff_workspace/src/options.rs, crates/ty_ide/src/completion.rs (+12 more) |
| sw=0.90_dd=0.70_k=40.0 | 0.000 | 0.000 | 0.000 | crates/ruff_linter/src/rules/mod.rs, crates/ruff_linter/src/rules/flake8_boolean_trap/mod.rs, crates/ruff_linter/src/rules/flake8_annotations/mod.rs, crates/ruff_workspace/src/options.rs, crates/ty_ide/src/completion.rs (+12 more) |
| sw=0.90_dd=0.70_k=60.0 | 0.000 | 0.000 | 0.000 | crates/ruff_linter/src/rules/mod.rs, crates/ruff_linter/src/rules/flake8_boolean_trap/mod.rs, crates/ruff_linter/src/rules/flake8_annotations/mod.rs, crates/ruff_workspace/src/options.rs, crates/ty_ide/src/completion.rs (+12 more) |
| sw=0.90_dd=0.70_k=80.0 | 0.000 | 0.000 | 0.000 | crates/ruff_linter/src/rules/mod.rs, crates/ruff_linter/src/rules/flake8_boolean_trap/mod.rs, crates/ruff_linter/src/rules/flake8_annotations/mod.rs, crates/ruff_workspace/src/options.rs, crates/ty_ide/src/completion.rs (+12 more) |
| sw=0.90_dd=1.00_k=20.0 | 0.000 | 0.000 | 0.000 | crates/ruff_linter/src/rules/mod.rs, crates/ruff_linter/src/rules/flake8_boolean_trap/mod.rs, crates/ruff_linter/src/rules/flake8_annotations/mod.rs, crates/ruff_workspace/src/options.rs, crates/ty_ide/src/completion.rs (+12 more) |
| sw=0.90_dd=1.00_k=40.0 | 0.000 | 0.000 | 0.000 | crates/ruff_linter/src/rules/mod.rs, crates/ruff_linter/src/rules/flake8_boolean_trap/mod.rs, crates/ruff_linter/src/rules/flake8_annotations/mod.rs, crates/ruff_workspace/src/options.rs, crates/ty_ide/src/completion.rs (+12 more) |
| sw=0.90_dd=1.00_k=60.0 | 0.000 | 0.000 | 0.000 | crates/ruff_linter/src/rules/mod.rs, crates/ruff_linter/src/rules/flake8_boolean_trap/mod.rs, crates/ruff_linter/src/rules/flake8_annotations/mod.rs, crates/ruff_workspace/src/options.rs, crates/ty_ide/src/completion.rs (+12 more) |
| sw=0.90_dd=1.00_k=80.0 | 0.000 | 0.000 | 0.000 | crates/ruff_linter/src/rules/mod.rs, crates/ruff_linter/src/rules/flake8_boolean_trap/mod.rs, crates/ruff_linter/src/rules/flake8_annotations/mod.rs, crates/ruff_workspace/src/options.rs, crates/ty_ide/src/completion.rs (+12 more) |