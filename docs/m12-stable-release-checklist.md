# M12 stable release checklist

This checklist distinguishes a repository-prepared 1.0 contract from a
published formal release. `v1.0.0` must not be tagged while any required
external evidence is pending.

| Requirement | Status | Evidence |
|---|---|---|
| stable contract inventory | implemented | [ADR 0018](adr/0018-stable-contract-and-release-evidence.md), `postgresem contract show`, `contracts/stable-v1.json` |
| historical RC immutability | implemented | `tests/contracts/verify.py` pins `contracts/rc-v1.json` |
| stable compatibility/deprecation periods | documented | [`compatibility.md`](compatibility.md), [`deprecation-policy.md`](deprecation-policy.md), [`SUPPORT.md`](../SUPPORT.md) |
| signed multi-platform release automation | implemented | `.github/workflows/release.yml` |
| stable 1.x external-evidence release gate | implemented, pending evidence | `contracts/release-evidence-v1.json`, `tests/contracts/verify_release_evidence.py` |
| query and ingestion operations | implemented | [operator workflow](rc-operator-workflow.md), `make test-rc-workflow` |
| PostgreSQL 16–18 migration/recovery | implemented | CI recovery matrix |
| Linux amd64/arm64 execution | implemented | native runtime and release gates |
| final reference comparison and differentiation | documented | [final differentiation](final-differentiation.md) |
| vulnerability response | documented | [`SECURITY.md`](../SECURITY.md) |
| current maintainer and release ownership | documented | [`GOVERNANCE.md`](../GOVERNANCE.md) |
| independent external security review | **outstanding** | [issue #4](https://github.com/rioriost/postgresem/issues/4) |
| two accepted 28-day non-fixture pilots | **outstanding** | [issue #4](https://github.com/rioriost/postgresem/issues/4) |
| unresolved field P0/P1 defects | **not measurable** | accepted pilot records are not yet available |

When maintainers accept the three external records, they must replace the
pending fields in `contracts/release-evidence-v1.json` with immutable HTTPS
evidence URLs, the reviewed commit/stable-manifest/image identity, dates, four
or more sorted weekly checkpoints per pilot, and zero unresolved P0/P1
findings. Pilot deployment identities must be full commits or image digests,
not mutable tags. Then:

```sh
shasum -a 256 contracts/stable-v1.json
make stable-check
```

Until this command passes, the repository is prepared for 1.0 qualification,
not authorized to publish `v1.0.0`.
