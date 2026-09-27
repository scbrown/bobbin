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
pairs. A continuation of a continuation is refused until an ancestry-aware mechanism
is implemented and reviewed.

This command is a review artifact, **not a continuation executor**. The original pilot
may predate client, index or runtime library locks. The plan reports whether a runtime
library lock exists but does not verify or manufacture one. It does not claim the
campaign is quiescent or acquire a spend lock. Before any paid continuation, review the
protocol deviation for omitted partial tasks, derive and verify the runtime lock from
the original archive pins, verify the client/model contract, and implement an exclusive
continuation claim with revalidation before spending. Never restart the full pilot to
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
