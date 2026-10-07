# V2 results: replay and publication contract

**Partial snapshot only: 25 L1 cells and 858 L0 score records.** The authoritative
coverage artifact is [reports/partial-20260927.json](reports/partial-20260927.json).
This file, the [book page](../../docs/book/src/eval/summary.md), and the
[v0.3 draft](../../docs/paper-context-injection.md) do not report final findings.

The legacy `just eval-publish` generator produces v1 summaries. Do not run it
over the v2 page or treat its output as a v2 report.

## Replay performed

The saved `l0-pinned.jsonl` was copied before analysis. Its SHA-256 is in the
artifact. Two manifest records are preserved: the first has zero following scores,
the second owns all 858 rows; the task/arm/budget keys are unique. From `eval/`:

```sh
python -m runner.cli l0-report /path/to/frozen/l0-pinned.jsonl --budget 300
```

This reads stored scores, executes no agent and writes its report to stdout. It
does not prove the campaign completed. Check manifest identity, duplicate keys,
coverage and error/skip counts before interpreting its statistical output. The
partial replay digest is recorded; quality estimates remain withheld.

L1 coverage was read from the original and continuation `result.json` artifacts.
Task and cell identities, serving model, input pins and source separation were
checked. The published projection retains validity flags and content hashes,
not local paths, prompts, transcripts or credentials. It deliberately omits
outcome estimates until the completion gate. The nine original saved grader
outputs also have an unchanged-classification replay receipt from recovery.

To independently replay the published L1 coverage, run this at the repository root:

```python
import json
from collections import Counter, defaultdict
from pathlib import Path

snapshot = json.loads(Path("eval/v2/reports/partial-20260927.json").read_text())
assert snapshot["status"] == "partial"
rows = snapshot["l1"]["records"]
assert len({(r["source"], r["task"], r["cell"]) for r in rows}) == len(rows)
print("PARTIAL recorded cells:", len(rows))
print("PARTIAL by source:", dict(Counter(r["source"] for r in rows)))
tasks = defaultdict(list)
for row in rows:
    tasks[(row["source"], row["task"])].append(row)
for source in ("original", "continuation"):
    groups = [v for (s, _), v in tasks.items() if s == source]
    complete = [g for g in groups if {r["cell"] for r in g} ==
                {"none", "tool", "inject", "both"}]
    valid = [g for g in complete if all(r["agent_valid"] and r["test_valid"] for r in g)]
    print("PARTIAL", source, "four-file tasks", len(complete), "four-valid tasks", len(valid))
```

This checks the public projection; verifying raw results requires the preserved
source artifacts matching each digest. A projection is not a substitute for those
artifacts. No new paid run or fixture execution is part of reporting.

## Gate before final numbers

1. The bounded continuation is terminal, with a recorded exit status and cell
   inventory; process inactivity alone is insufficient.
2. Its completion report is posted and reviewed in the owning run record.
3. The report reconciles original/continuation provenance, fixture exclusions,
   invalid execution, incomplete tasks and any stream without a result.
4. Success rates and contrasts use the registered outcome policy, with explicit
   denominators and source labels. Diagnostic all-valid counts here do not change it.
5. Replace the partial snapshot with a frozen, reviewed report. Update the book,
   paper and launch source together; retain this snapshot as historical evidence.
6. L0 has its own completeness and error audit; L1 completion does not complete L0.

Until all applicable conditions hold, every interim table must say **partial**
and state its cell or record count. Do not turn the original 30-task plan or the
20-cell continuation allocation into a completed-study sample size.
