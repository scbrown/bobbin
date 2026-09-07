# Publishing bobbin-ai to crates.io

The release workflow publishes the crate in the same workflow run, after all
binary builds, checksums, and GitHub release publication succeed. A release event
created with GITHUB_TOKEN cannot trigger another workflow, so crates.yml is only
the manual recovery lane. Both lanes use .github/actions/crates-publish.

The automatic lane checks out the resolved release tag and derives the expected
version from that tag. The manual lane requires an independent version input:

```sh
gh workflow run crates.yml --ref main -f version=0.16.2 -f dry_run=true
```

Manual runs default to dry-run. A dry run reports that no publication was requested;
its green conclusion is not proof of registry visibility. Setting dry_run=false
requests a real, permanent crate publication and requires crates.io Trusted
Publishing to authorize this repository and workflow.

Before authentication, the shared action refuses a version mismatch, a prerelease
or malformed version, and test-/rehearsal- refs. These refusals do not depend on
Trusted Publishing being unavailable. The guard selftest includes a permitted
stable release and runs in CI and before release asset builds.

The action checks the exact version endpoint, not only the newest version. An
already-published version succeeds without authenticating or uploading again.
HTTP 404 means absent; transport failures, malformed replies and other HTTP errors
mean unknown and cannot authorize a publish based on assumed absence. Registry
requests include a User-Agent. After an upload, bounded polling must observe the
intended version or the job fails.

Verification commands:

```sh
python3 scripts/test-crates-workflow.py
python3 scripts/test-release-workflow-contract.py
```

The tests exercise allowed/refused versions and refs, registry success/absence/
unknown outcomes, and shared-action/release dependency wiring. Real workflow
acceptance additionally checks that a rehearsal refusal skips every later action
step. A valid publication trial must only be attempted after verifying the current
Trusted Publishing gate; a historical authentication failure does not establish
that it is still closed.
