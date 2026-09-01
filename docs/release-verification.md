# Release verification

`v0.2.0-alpha.1` predates signing and is checksum-only. `v0.3.0-beta.1` is the
first release with keyless Sigstore signatures for its checksums and immutable
container image digest.

Beginning with `v0.4.0`, publication is additionally gated by native Linux
amd64/arm64 execution. Each packaged binary and each architecture-specific
runtime image must start and complete explicit-TLS, catalog, query, and
governed-mutation smoke checks against PostgreSQL 18. The workflow uploads
machine-readable `*-runtime.json` evidence alongside the release artifacts.
The multi-architecture manifest is published only after both native image jobs
pass.

Signed release assets include `SHA256SUMS`, `SHA256SUMS.sig`, and
`SHA256SUMS.pem`. Verify them with Cosign:

```sh
cosign verify-blob \
  --certificate SHA256SUMS.pem \
  --signature SHA256SUMS.sig \
  --certificate-identity-regexp \
    '^https://github.com/rioriost/postgresem/.github/workflows/release.yml@refs/tags/v0\.4\.0$' \
  --certificate-oidc-issuer https://token.actions.githubusercontent.com \
  SHA256SUMS
```

Then verify the selected archive against `SHA256SUMS`.

Verify `v0.4.0` by immutable image digest:

```sh
cosign verify \
  --certificate-identity-regexp \
    '^https://github.com/rioriost/postgresem/.github/workflows/release.yml@refs/tags/v0\.4\.0$' \
  --certificate-oidc-issuer https://token.actions.githubusercontent.com \
  ghcr.io/rioriost/postgresem@sha256:de4a77a9852b227e444fb8938cdb9d93a20336740cd10427c445458536313bd2
```

The certificate identity and OIDC issuer checks are required. A signature
without an expected workflow identity does not establish the intended
publisher. Substitute the exact expected tag when verifying another release.

These commands were exercised against the published `v0.4.0` assets. The
checksum certificate matched the release workflow tag identity, and one valid
image signature was found in the transparency log.
