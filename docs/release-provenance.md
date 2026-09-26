# Release provenance

Each target's release archive carries three additional assets:

- `bobbin-VERSION-TARGET.cdx.json`: CycloneDX 1.5 Rust dependency inventory.
- `bobbin-VERSION-TARGET.tar.gz.sigstore.json`: offline build-provenance bundle.
- `bobbin-VERSION-TARGET.tar.gz.sbom.sigstore.json`: offline SBOM attestation bundle.

The provenance attestation binds both the archive and its original executable.
The standalone Linux downloads are copies of that executable and have the same
digest. The SBOM attestation binds the inventory to the archive digest. All three
metadata assets survive matrix uploads and appear in the final `SHA256SUMS.txt`.
Attestation failure fails the build and prevents release publication.

`cargo-auditable` 0.7.6 embeds dependency information in the executable.
`cargo-cyclonedx` 0.5.9 uses the build's target and `knowledge` feature selection;
the workflow rejects any lockfile modification during inventory generation.
This inventory covers Cargo dependencies. It does not inventory the bundled
ONNX Runtime's native dependency tree or the compiler/toolchain itself.

The production verification identity is:

```text
https://github.com/scbrown/bobbin/.github/workflows/release.yml@refs/heads/main
```

The issuer is `https://token.actions.githubusercontent.com`, and the provenance
predicate is SLSA v1 (`https://slsa.dev/provenance/v1`). Release Please builds in
the push-to-main run while checking out the resolved version tag. Checking out
that tag does not change the workflow's signing identity. Tag-triggered recovery
runs have a different identity and must not pass a verifier pinned to main.

Identity pins the workflow and ref, not the triggering event. A manual dispatch
against main can share that identity; consumers requiring push events exclusively
must verify the event separately. See GitHub's [dispatch ref contract][dispatch]
and [separate OIDC event and workflow claims][oidc].

[dispatch]: https://docs.github.com/en/actions/reference/workflows-and-actions/events-that-trigger-workflows#workflow_dispatch
[oidc]: https://docs.github.com/en/actions/reference/security/oidc

Workflow tests exercise both checksum and finalizer shell bodies with synthetic
archives for all four targets. Removing either metadata filename pattern must
fail those tests. These tests prove packaging behavior, not GitHub OIDC signing:
publication and independent verification of a real release remain the acceptance
check for signing and deployment.
