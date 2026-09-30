# Interrupted pilot recovery

`python -m runner.pilot_resume <original-output> --manifest-sha256 <reviewed-sha256>`
prints a read-only recovery plan. It never starts an agent, reads credentials, writes
to the campaign, or authorizes spending. Run from `eval/` in a checkout containing
the original harness commit. The manifest digest must identify the preserved run.

The planner verifies the original preregistration against that commit, the recorded
binary and wrapper hashes, the complete 120-cell schedule, and all 30 current task
definitions against their original bytes. Missing historical source, changed pins,
unrecognized artifacts and symlinks in prior evidence are refusals. It hashes the
prior result artifacts so a reviewer can identify exactly what was inspected.

Every existing task directory is excluded, even an empty directory or one containing
only an interrupted stream. Looking only for `result.json` would permit another paid
invocation after a crash between spending and recording its result. Skipping the whole
task also avoids rebuilding a potentially different index for the remaining cells of
an incomplete pair. Fixture failures remain recorded; the plan does not retry them.

The remaining schedule retains the original cell order. Excluded tasks, missing cells
and incomplete pairs are reported explicitly; they are never successes or completed
pairs. A continuation of a continuation needs the ancestry-aware
[chained executor](#chained-continuation) below; this planner alone still refuses it.

This command is a review artifact, **not a continuation executor**. The original pilot
may predate client, index or runtime library locks. The plan reports whether a runtime
library lock exists but does not verify or manufacture one. It does not claim the
campaign is quiescent or acquire a spend lock. Before any paid continuation, review the
protocol deviation for omitted partial tasks, derive and verify the runtime lock from
the original archive pins, verify the client/model contract, and review the exclusive
continuation claim with revalidation before spending described below. Never restart the full pilot to
work around a refusal.

## Free fixture controls

`python -m runner.pilot_fixture_controls <original-output> --manifest-sha256 <sha256>
--output <new-directory>` checks every remaining task before any paid continuation.
It runs the parent and known-fix controls without agent invocation, indexing or model
credentials. It records fixture and infrastructure exclusions separately from arm
outcomes and continues checking the other tasks after a subprocess failure. Output
must be new and outside the preserved campaign. `all_tasks_checked` is false until
the whole remaining list has been checked; an interrupted preflight is not approval.

The custom Typst summary is now recognized using its explicit passed/failed/skipped
counts. Zero executed tests still fail the control. A Go package-level `ok` line is
still insufficient evidence that the selected regression test executed.

## Single continuation executor

`python -m runner.pilot_continue <original-output> --manifest-sha256 <sha256>
--controls <completed-report.json> --controls-sha256 <sha256>
--runtime <derived-runtime-lock.json> --runtime-sha256 <sha256>
--client <resolved-client-executable> --client-sha256 <sha256>` validates and prints
an execution plan without claiming or running the campaign. Each digest identifies
an input reviewed before launch; do not regenerate expected hashes to bypass a refusal.

The complete free control report must cover every untouched task. Eligible tasks need
an executed failing parent and passing fix, and matching original evidence/task pins.
The new runtime lock must preserve every original archive/configuration field and pass
verification of the actual libraries loaded. The original campaign did not pin its
client executable: the continuation explicitly records that limitation and its new
client pin. Do not claim byte-identical original client provenance.

After implementation review, complete CI, approval of the control results and the
landed preregistration amendment, append `--execute --output <new-directory>`.
Execution creates `continuation-claim.json` exclusively in the original campaign;
this is its only source write. The output must be new and outside that campaign.
Never run this alongside the original campaign: independently verify it is stopped.
Two concurrent continuations cannot acquire the same claim. A setup failure or
interruption retains the claim and requires a separately reviewed recovery; deleting
it to retry is unsupported.

Only eligible untouched tasks run, with the original cell order and limits. Each task
gets one new parent index shared by its four cells; no old partial pair is extended.
Original artifacts, the claim, copied executables/configuration, client and fixture
receipts are checked again before each paid cell. The runtime verifier checks actual
library loading before cells and the GPU wrapper enforces the hold/proof contract.
The original complete pairs and continuation pairs must remain labelled by source in
any descriptive pooling; the original partial pair remains incomplete.

## Chained continuation

`python -m runner.pilot_chain <original-output> --manifest-sha256 <sha256>
--parent <first-continuation-output> --parent-sha256 <its manifest sha256>
--parent-evidence-sha256 <sha256> --controls <standalone-report.json>
--controls-sha256 <sha256> --runtime <lock> --runtime-sha256 <sha256>
--client <client> --client-sha256 <sha256>` plans a second continuation. Like the
first, it prints a plan by default and needs `--execute --output <new-directory>`.

It skips every task attempted by the original campaign **and** by the first
continuation, whatever the outcome. The first continuation must be the one named by
the original campaign's claim, with an unchanged manifest; its task evidence is hashed
and must match `--parent-evidence-sha256`, the `parent.evidence_sha256` a reviewer
read from a dry run. Original evidence fields recorded by the first continuation must
be unchanged. Runtime and client pins must equal the first continuation's, so both
cohorts ran the same treatment binaries.

Task definitions may have been repaired since the original run, but only for tasks
with no paid evidence in either cohort; a changed definition of any paid task is
refused. The fixture report must be a complete standalone report
(`runner.pilot_fixture_controls --tasks ...`) covering exactly the never-attempted
tasks in schedule order, and its recorded task hashes must equal the definitions that
will run. Eligibility uses the same discriminating-control rule as before.

The claim is created exclusively in the first continuation's output, so the chain can
run once; the original campaign's claim is not modified. Original evidence, the first
continuation's manifest and evidence, the task definitions, pins and receipts are
checked again before each paid cell. Deeper chains are refused until reviewed.
