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
`SHA256SUMS.pem`. Download these files and your selected archive from the
[v1.0.0 release](https://github.com/rioriost/postgresem/releases/tag/v1.0.0).
Verify the checksums signature with Cosign:

```sh
cosign verify-blob \
  --certificate SHA256SUMS.pem \
  --signature SHA256SUMS.sig \
  --certificate-identity-regexp \
    '^https://github.com/rioriost/postgresem/.github/workflows/release.yml@refs/tags/v1\.0\.0$' \
  --certificate-oidc-issuer https://token.actions.githubusercontent.com \
  SHA256SUMS
```

Then verify the selected archive against its entry in `SHA256SUMS` before
extracting or running it. SHA-256 alone is not publisher authentication.

Verify the container image by immutable digest. Set `IMAGE_DIGEST` to the
`sha256:...` digest published for `ghcr.io/rioriost/postgresem:1.0.0` using your
OCI registry client; record that digest and verify/run the same immutable
image, not the mutable `latest` tag:

```sh
cosign verify \
  --certificate-identity-regexp \
    '^https://github.com/rioriost/postgresem/.github/workflows/release.yml@refs/tags/v1\.0\.0$' \
  --certificate-oidc-issuer https://token.actions.githubusercontent.com \
  "ghcr.io/rioriost/postgresem@${IMAGE_DIGEST:?Set the published v1.0.0 digest}"
```

The certificate identity and OIDC issuer checks are required. A signature
without an expected workflow identity does not establish the intended
publisher. Substitute the exact expected tag when verifying another release.

Signing authenticates the release publisher; it does not certify independent
security review or production operation. `v1.0.0` uses the
[maintainer-approved source-review exception](adr/0020-v1-release-maintainer-exception.md),
not an independent image review or completed field pilots.
