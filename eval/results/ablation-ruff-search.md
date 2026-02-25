# Ruff Search-Level Ablation Report

Generated: 2026-02-25T21:17:58.098754+00:00

Baseline config: sw=0.9, dd=0.3, k=60.0, depth=1

## Summary (sorted by F1)

| Variant | Description | Tasks | Precision | Recall | F1 | Latency |
|---------|-------------|-------|-----------|--------|----|---------|
| no_semantic | Disable semantic search (pure keyword/BM25) | 4 | 0.267 | 0.392 | **0.301** | 982ms |
| no_keyword | Disable keyword search (pure semantic/embedding) | 4 | 0.144 | 0.383 | **0.201** | 973ms |
| baseline | Full pipeline: sw=0.90, dd=0.30, k=60, depth=1 | 4 | 0.135 | 0.383 | **0.194** | 1078ms |
| no_coupling | Disable temporal coupling expansion | 4 | 0.135 | 0.383 | **0.194** | 974ms |
| no_recency | Disable recency/freshness signal | 4 | 0.135 | 0.383 | **0.194** | 978ms |
| no_doc_demotion | Treat docs same as source (doc_demotion=1.0) | 4 | 0.081 | 0.175 | **0.100** | 969ms |

## Delta from Baseline

| Variant | F1 | Delta | Impact |
|---------|-----|-------|--------|
| no_semantic | 0.301 | +0.107 | Removing this helps (helps) |
| no_keyword | 0.201 | +0.007 | Removing this helps (helps) |
| baseline | 0.194 | BASELINE | — |
| no_coupling | 0.194 | 0.000 | Removing this has no effect (neutral) |
| no_recency | 0.194 | 0.000 | Removing this has no effect (neutral) |
| no_doc_demotion | 0.100 | -0.094 | Removing this hurts (hurts) |

## Per-Task Results


### ruff-001

Ground truth: crates/ruff_python_formatter/resources/test/fixtures/ruff/statement/try.py, crates/ruff_python_formatter/src/other/except_handler_except_handler.rs, crates/ruff_python_formatter/tests/snapshots/format@statement__try.py.snap

| Variant | Precision | Recall | F1 | Returned Files |
|---------|-----------|--------|----|----------------|
| baseline | 0.000 | 0.000 | 0.000 |  |
| no_semantic | 0.000 | 0.000 | 0.000 |  |
| no_keyword | 0.000 | 0.000 | 0.000 |  |
| no_coupling | 0.000 | 0.000 | 0.000 |  |
| no_doc_demotion | 0.000 | 0.000 | 0.000 |  |
| no_recency | 0.000 | 0.000 | 0.000 |  |

### ruff-002

Ground truth: crates/ruff_linter/resources/test/fixtures/flake8_boolean_trap/FBT.py, crates/ruff_linter/src/rules/flake8_boolean_trap/helpers.rs, crates/ruff_linter/src/rules/flake8_boolean_trap/snapshots/ruff_linter__rules__flake8_boolean_trap__tests__FBT001_FBT.py.snap, crates/ruff_linter/src/rules/flake8_boolean_trap/snapshots/ruff_linter__rules__flake8_boolean_trap__tests__FBT003_FBT.py.snap, crates/ruff_linter/src/rules/flake8_boolean_trap/snapshots/ruff_linter__rules__flake8_boolean_trap__tests__extend_allowed_callable.snap

| Variant | Precision | Recall | F1 | Returned Files |
|---------|-----------|--------|----|----------------|
| no_semantic | 0.400 | 0.400 | 0.400 | crates/ruff_linter/src/rules/flake8_boolean_trap/helpers.rs, crates/ruff_linter/src/checkers/ast/analyze/statement.rs, crates/ty_ide/src/completion.rs, crates/ruff_linter/src/rules/ruff/mod.rs, crates/ruff_linter/resources/test/fixtures/flake8_boolean_trap/FBT.py |
| no_doc_demotion | 0.200 | 0.200 | 0.200 | crates/ty_ide/src/completion.rs, changelogs/0.14.x.md, crates/ruff_linter/src/rules/flake8_boolean_trap/helpers.rs, crates/ruff_linter/src/rules/ruff/mod.rs, crates/ruff_linter/src/checkers/ast/analyze/statement.rs |
| baseline | 0.125 | 0.200 | 0.154 | crates/ty_ide/src/completion.rs, crates/ruff_linter/src/rules/flake8_boolean_trap/helpers.rs, crates/ruff_linter/src/rules/ruff/mod.rs, crates/ruff_linter/src/checkers/ast/analyze/statement.rs, crates/ruff_linter/src/rules/airflow/rules/removal_in_3.rs (+3 more) |
| no_keyword | 0.125 | 0.200 | 0.154 | crates/ty_ide/src/completion.rs, crates/ruff_linter/src/rules/flake8_boolean_trap/helpers.rs, crates/ruff_linter/src/rules/ruff/mod.rs, crates/ruff_linter/src/checkers/ast/analyze/statement.rs, crates/ruff_linter/src/rules/airflow/rules/removal_in_3.rs (+3 more) |
| no_coupling | 0.125 | 0.200 | 0.154 | crates/ty_ide/src/completion.rs, crates/ruff_linter/src/rules/flake8_boolean_trap/helpers.rs, crates/ruff_linter/src/rules/ruff/mod.rs, crates/ruff_linter/src/checkers/ast/analyze/statement.rs, crates/ruff_linter/src/rules/airflow/rules/removal_in_3.rs (+3 more) |
| no_recency | 0.125 | 0.200 | 0.154 | crates/ty_ide/src/completion.rs, crates/ruff_linter/src/rules/flake8_boolean_trap/helpers.rs, crates/ruff_linter/src/rules/ruff/mod.rs, crates/ruff_linter/src/checkers/ast/analyze/statement.rs, crates/ruff_linter/src/rules/airflow/rules/removal_in_3.rs (+3 more) |

### ruff-003

Ground truth: crates/ruff_linter/resources/test/fixtures/pylint/import_private_name/submodule/__main__.py, crates/ruff_linter/src/rules/pylint/rules/import_private_name.rs, crates/ruff_linter/src/rules/pylint/snapshots/ruff_linter__rules__pylint__tests__PLC2701_import_private_name__submodule____main__.py.snap

| Variant | Precision | Recall | F1 | Returned Files |
|---------|-----------|--------|----|----------------|
| no_keyword | 0.200 | 0.333 | 0.250 | crates/ty_python_semantic/src/types/infer/builder.rs, crates/ruff_python_ast/src/lib.rs, crates/ruff_linter/src/rules/pylint/rules/import_private_name.rs, crates/ty_python_semantic/src/types.rs, crates/ty/src/python_version.rs |
| baseline | 0.167 | 0.333 | 0.222 | crates/ruff_linter/src/rules/pyupgrade/mod.rs, crates/ty_python_semantic/src/types/infer/builder.rs, crates/ruff_linter/src/rules/pylint/rules/import_private_name.rs, crates/ruff_python_ast/src/lib.rs, crates/ty_python_semantic/src/types.rs (+1 more) |
| no_semantic | 0.167 | 0.333 | 0.222 | crates/ruff_linter/src/rules/pylint/rules/import_private_name.rs, crates/ruff_linter/src/rules/pyupgrade/mod.rs, crates/ruff_linter/src/rules/airflow/helpers.rs, crates/ty_python_semantic/src/types.rs, crates/ruff_linter/src/preview.rs (+1 more) |
| no_coupling | 0.167 | 0.333 | 0.222 | crates/ruff_linter/src/rules/pyupgrade/mod.rs, crates/ty_python_semantic/src/types/infer/builder.rs, crates/ruff_linter/src/rules/pylint/rules/import_private_name.rs, crates/ruff_python_ast/src/lib.rs, crates/ty_python_semantic/src/types.rs (+1 more) |
| no_recency | 0.167 | 0.333 | 0.222 | crates/ruff_linter/src/rules/pyupgrade/mod.rs, crates/ty_python_semantic/src/types/infer/builder.rs, crates/ruff_linter/src/rules/pylint/rules/import_private_name.rs, crates/ruff_python_ast/src/lib.rs, crates/ty_python_semantic/src/types.rs (+1 more) |
| no_doc_demotion | 0.000 | 0.000 | 0.000 | crates/ty/docs/rules.md, crates/ruff_linter/src/rules/pyupgrade/mod.rs, crates/ruff_python_ast/src/lib.rs |

### ruff-004

Ground truth: crates/ruff_linter/resources/test/fixtures/flake8_pyi/PYI034.py, crates/ruff_linter/src/rules/flake8_pyi/rules/non_self_return_type.rs, crates/ruff_linter/src/rules/flake8_pyi/snapshots/ruff_linter__rules__flake8_pyi__tests__PYI034_PYI034.py.snap

| Variant | Precision | Recall | F1 | Returned Files |
|---------|-----------|--------|----|----------------|
| no_semantic | 0.333 | 0.333 | 0.333 | crates/ruff_linter/resources/test/fixtures/flake8_pyi/PYI034.py, crates/ruff_linter/src/rules/flake8_pyi/mod.rs, crates/ty_python_semantic/src/types.rs |
| baseline | 0.000 | 0.000 | 0.000 | crates/ruff_linter/src/rules/flake8_pyi/mod.rs, crates/ruff_linter/src/rules/flake8_builtins/mod.rs, crates/ruff_linter/src/rules/flake8_bugbear/mod.rs, crates/ruff_linter/src/rules/pyflakes/mod.rs, CHANGELOG.md |
| no_keyword | 0.000 | 0.000 | 0.000 | crates/ruff_linter/src/rules/flake8_pyi/mod.rs, crates/ruff_linter/src/rules/flake8_bugbear/mod.rs, crates/ruff_linter/src/rules/pyflakes/mod.rs, crates/ruff_linter/src/rules/flake8_builtins/mod.rs, CHANGELOG.md |
| no_coupling | 0.000 | 0.000 | 0.000 | crates/ruff_linter/src/rules/flake8_pyi/mod.rs, crates/ruff_linter/src/rules/flake8_builtins/mod.rs, crates/ruff_linter/src/rules/flake8_bugbear/mod.rs, crates/ruff_linter/src/rules/pyflakes/mod.rs, CHANGELOG.md |
| no_doc_demotion | 0.000 | 0.000 | 0.000 | CHANGELOG.md, crates/ruff_linter/src/rules/flake8_pyi/mod.rs, crates/ruff_linter/src/rules/flake8_builtins/mod.rs, crates/ruff_linter/src/rules/flake8_bugbear/mod.rs, BREAKING_CHANGES.md (+1 more) |
| no_recency | 0.000 | 0.000 | 0.000 | crates/ruff_linter/src/rules/flake8_pyi/mod.rs, crates/ruff_linter/src/rules/flake8_builtins/mod.rs, crates/ruff_linter/src/rules/flake8_bugbear/mod.rs, crates/ruff_linter/src/rules/pyflakes/mod.rs, CHANGELOG.md |

### ruff-005

Ground truth: crates/ruff/src/commands/format.rs, crates/ruff/tests/cli/format.rs

| Variant | Precision | Recall | F1 | Returned Files |
|---------|-----------|--------|----|----------------|
| baseline | 0.250 | 1.000 | 0.400 | crates/ruff_markdown/src/lib.rs, crates/ruff/tests/cli/format.rs, crates/ruff/tests/cli/lint.rs, crates/ruff/src/commands/format.rs, docs/formatter.md (+3 more) |
| no_keyword | 0.250 | 1.000 | 0.400 | crates/ruff_markdown/src/lib.rs, crates/ruff/tests/cli/format.rs, crates/ruff/tests/cli/lint.rs, crates/ruff/src/commands/format.rs, BREAKING_CHANGES.md (+3 more) |
| no_coupling | 0.250 | 1.000 | 0.400 | crates/ruff_markdown/src/lib.rs, crates/ruff/tests/cli/format.rs, crates/ruff/tests/cli/lint.rs, crates/ruff/src/commands/format.rs, docs/formatter.md (+3 more) |
| no_recency | 0.250 | 1.000 | 0.400 | crates/ruff_markdown/src/lib.rs, crates/ruff/tests/cli/format.rs, crates/ruff/tests/cli/lint.rs, crates/ruff/src/commands/format.rs, docs/formatter.md (+3 more) |
| no_semantic | 0.167 | 0.500 | 0.250 | crates/ruff/tests/cli/format.rs, crates/ty/src/lib.rs, crates/ruff/src/lib.rs, crates/ruff/src/args.rs, crates/ty_project/src/metadata/options.rs (+1 more) |
| no_doc_demotion | 0.125 | 0.500 | 0.200 | docs/formatter.md, docs/configuration.md, BREAKING_CHANGES.md, docs/integrations.md, crates/ruff_markdown/src/lib.rs (+3 more) |