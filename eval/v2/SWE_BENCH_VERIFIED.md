# L2: frozen SWE-bench Verified subset

This prepares an **exploratory external task set**, not an evaluation result.
The 23 instances are frozen in `swebench-verified-manifest.json`. Selection uses
at most two instances per repository, ranked by SHA-256 of seed `20260925`, a
newline and the instance ID. No model output, retrieval score, patch size or
observed test outcome enters selection. The source has 500 instances in 12
repositories; Flask contributes its only instance. Malformed records abort
preparation rather than silently changing eligibility.

The source is [SWE-bench/SWE-bench_Verified](https://huggingface.co/datasets/SWE-bench/SWE-bench_Verified),
revision `78f471bf655a3137b2e8a75af1501690ec009ec3`, test split. The manifest pins
the Parquet SHA-256, every selected ID/base revision, each complete record hash,
and the prepared evaluator JSONL hash. Downloading a changed snapshot or changing
the selection refuses before output is created.

## Prepare without running a benchmark

From the repository root, with `uv` installed:

```bash
curl -fL --connect-timeout 10 --max-time 120 \
  -o /tmp/swebench-verified.parquet \
  https://huggingface.co/datasets/SWE-bench/SWE-bench_Verified/resolve/78f471bf655a3137b2e8a75af1501690ec009ec3/data/test-00000-of-00001.parquet
uv run --project eval --with pyarrow==25.0.1 python -m runner.swebench \
  --source /tmp/swebench-verified.parquet --output /tmp/bobbin-l2
```

The output directory must be new. Existing evidence is never overwritten.
This command downloads no models, clones no task repositories and executes no
commands from the dataset. PyArrow is needed only for preparation; ordinary
eval commands and the synthetic tests do not need it.

- `prompts.jsonl`: only instance ID, repository, explicit `base_commit` and
  verbatim `problem_statement`. Agents receive one matching record and a checkout
  at that base revision, **not its parent**.
- `evaluator/instances.jsonl`: complete source records, including `patch`,
  `test_patch`, `FAIL_TO_PASS`, `PASS_TO_PASS`, environment metadata and hints.
  Preserve this outside agent checkouts, indexes, prompts and readable mounts.
  The directory permissions are convenience, not isolation from an agent with
  the same Unix identity; the runner must enforce the mount/access boundary.
- `manifest.json`: exact copy of the frozen selection contract.

No gold/test patches are committed to Bobbin or injected into the task prompt.
The preparation test against the pinned source produces evaluator SHA-256
`81e6fdf2bd725e28b4f264305c661cb7059a4d1d74e0d6a22f8183d131d680ac`.

## Grading boundary

Use SWE-bench's official evaluator and its per-instance environment, gold tests,
and log parser. Do not manufacture `pytest` commands or convert `base_commit`
into the fixing-commit field used by `eval/tasks/*.yaml`: those runners use
`commit^` and would evaluate the wrong tree. This dataset is intentionally not
fed to the legacy `bobbin-eval pilot` command.

The [official evaluator](https://www.swebench.com/SWE-bench/guides/evaluation/)
accepts prediction records with `instance_id`, `model_name_or_path` and
`model_patch`. Its [local JSONL loader](https://github.com/SWE-bench/SWE-bench/blob/02e7a74ffd0b707aab73d203fe87bdc7c76afc8e/swebench/harness/utils.py)
accepts the prepared evaluator file. Pin that harness revision in the eventual
run manifest. In a provisioned evaluator environment, after separate run approval:

```bash
python -m swebench.harness.run_evaluation \
  --dataset_name /tmp/bobbin-l2/evaluator/instances.jsonl \
  --predictions_path predictions.jsonl --max_workers 1 --run_id l2-reviewed-run
```

Before scoring agents, require a gold-patch positive control and a buggy-base
negative control for each selected instance. Record infrastructure failures,
missing reports and zero executed tests as unproven outcomes, never as passes.
This change has **not** run those controls, provisioned Docker images, or run
agents. Preparation and schema tests prove data integrity and separation, not
that every historical build environment currently works.

## Interpretation and contamination

These are public Python issue/fix pairs. Adding repositories improves external
coverage but does not add language diversity or make the tasks private. The
balanced 23-task subset is not the full SWE-bench Verified benchmark, and its
resolution fraction must not be presented as a leaderboard score. Publish the
selected IDs, per-repository denominators, missing/failed infrastructure outcomes
and uncertainty alongside any result; do not replace failed instances after
looking at outcomes.

The dataset's `created_at` is the task/issue timestamp, **not a fixing-commit
date or proof of first public availability**. The manifest preserves it without
relabelling it. Gold patches and issue discussions may have appeared in model
training; prominence of this benchmark makes exposure a serious limitation.
Record the exact model, provider-stated training cutoff and source for that
cutoff when running. Until actual fix/publication dates are established,
contamination status is unknown: no post-cutoff or out-of-distribution claim is
supported. Do not select a supposedly uncontaminated subset using `created_at`.

For an eventual L2 comparison, report this external set separately and retain
our curated `eval/tasks/*.yaml` set as **secondary**, with separate estimates.
The registered L1 pilot's 30 tasks, model, hypotheses and analysis remain unchanged.
This small L2 set is exploratory; confirmatory power, agent budget and run
approval are separate decisions. No L2 result is implied by preparing it.
