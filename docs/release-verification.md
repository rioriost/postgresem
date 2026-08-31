# Release verification

`v0.2.0-alpha.1` predates signing and is checksum-only. The M5 release workflow
is configured to add keyless Sigstore signatures to future releases.

Future release assets include `SHA256SUMS`, `SHA256SUMS.sig`, and
`SHA256SUMS.pem`. Verify them with Cosign:

```sh
cosign verify-blob \
  --certificate SHA256SUMS.pem \
  --signature SHA256SUMS.sig \
  --certificate-identity-regexp \
    '^https://github.com/rioriost/postgresem/.github/workflows/release.yml@refs/tags/v' \
  --certificate-oidc-issuer https://token.actions.githubusercontent.com \
  SHA256SUMS
```

Then verify the selected archive against `SHA256SUMS`.

Verify a future signed image by immutable digest:

```sh
cosign verify \
  --certificate-identity-regexp \
    '^https://github.com/rioriost/postgresem/.github/workflows/release.yml@refs/tags/v' \
  --certificate-oidc-issuer https://token.actions.githubusercontent.com \
  ghcr.io/rioriost/postgresem@sha256:<digest>
```

The certificate identity and OIDC issuer checks are required. A signature
without an expected workflow identity does not establish the intended
publisher.

