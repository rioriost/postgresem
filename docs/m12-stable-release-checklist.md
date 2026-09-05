# M12 stable release checklist

This checklist distinguishes a repository-prepared 1.0 contract from a
published formal release. `v1.0.0` requires accepted evidence under either the
ordinary process or the narrowly scoped
[ADR 0020 maintainer exception](adr/0020-v1-release-maintainer-exception.md),
plus all unchanged technical qualification and release automation.

| Requirement | Status | Evidence |
|---|---|---|
| stable contract inventory | implemented | [ADR 0018](adr/0018-stable-contract-and-release-evidence.md), `postgresem contract show`, `contracts/stable-v1.json` |
| historical RC immutability | implemented | `tests/contracts/verify.py` pins `contracts/rc-v1.json` |
| stable compatibility/deprecation periods | documented | [`compatibility.md`](compatibility.md), [`deprecation-policy.md`](deprecation-policy.md), [`SUPPORT.md`](../SUPPORT.md) |
| signed multi-platform release automation | implemented | `.github/workflows/release.yml` |
| stable 1.x release-evidence gate | required; v1.0.0 exception approved | `contracts/release-evidence-v1.json`, `tests/contracts/verify_release_evidence.py`; ordinary strict gates remain the default for later stable tags |
| query and ingestion operations | implemented | [operator workflow](rc-operator-workflow.md), `make test-rc-workflow` |
| unified application demonstration | implemented | [Meaning Lab](../examples/semantic_demo/README.md), real PostgreSQL comparison/ingestion/RLS via `make test-semantic-demo` |
| PostgreSQL 16–18 migration/recovery | implemented | CI recovery matrix |
| Linux amd64/arm64 execution | implemented | native runtime and release gates |
| final reference comparison and differentiation | documented | [final differentiation](final-differentiation.md) |
| vulnerability response | documented | [`SECURITY.md`](../SECURITY.md) |
| current maintainer and release ownership | documented | [`GOVERNANCE.md`](../GOVERNANCE.md) |
| automated source security review | **accepted under v1.0.0 maintainer exception** | [immutable report](https://github.com/rioriost/postgresem/blob/2797160ee431ee12722d339e23def6d8c8e7fbd5/docs/security-reviews/2026-09-05-381fe57.md) and [ADR 0020](adr/0020-v1-release-maintainer-exception.md); not independent external approval |
| post-review security corrections | **accepted under v1.0.0 maintainer exception** | [remediation evidence](security-reviews/2026-09-05-381fe57.md#post-review-remediation) binds `c8a2ca7a6a635de975d8e8b2324b652ac037075c` and its reviewed stable-contract digest; [ADR 0019](adr/0019-role-bound-reconciliation-and-jwk-only-dependencies.md) remains unchanged |
| independent external security review and reviewed container image | **waived for v1.0.0; not completed** | [ADR 0020](adr/0020-v1-release-maintainer-exception.md); no independent external review or image review was performed |
| ongoing supply-chain monitoring | continuing; not a current blocker by itself | maintainer determination on 2026-09-05; existing dependency and release integrity controls remain required |
| two accepted 28-day non-fixture pilots | **waived for v1.0.0; not completed** | [ADR 0020](adr/0020-v1-release-maintainer-exception.md); historical goal in [issue #4](https://github.com/rioriost/postgresem/issues/4) |
| unresolved field P0/P1 defects | **not measured / accepted limitation** | no qualifying field periods; not a zero-defect claim |

## v1.0.0 acceptance and publication

Owner `rioriost` approved this exception on 2026-09-05 in the release
implementation conversation. It accepts automated source review/remediation
only for corrected source `c8a2ca7a6a635de975d8e8b2324b652ac037075c` and reviewed
stable-manifest digest
`sha256:c3d58c9fd3670836da7f86c73c478dcee4a087601cb083a927e3f7617e4f18a2`.
The original `381fe57` review and hashes remain historical. See the
[corrected review status](beta-checklist.md#external-security-review-status-2026-09-05).

Register the separate strict exception shape specified in ADR 0020:
`source_security_review` with the immutable report URL and corrected identity,
`reviewed_image_digest: null`, source retest date `2026-09-05`, zero unresolved
source P0/P1 findings, `field_pilots: []`, and the exact `maintainer_exception`
scope, owner, date, and three waived requirements. Its `decision_url` must be
a genuine immutable commit-pinned URL for ADR 0020 after the policy commit
exists, not a placeholder.

The validator must hash the stable manifest at the reviewed source commit,
require the same public contract in the release tree, and reject subsequent
changes outside the exact release-only path allowlist. Generated stable
inventory or gate-code hashes are not independently reviewed. No source,
dependency, migration, packaging, or CI changes are covered by this exception.
The review and decision commits must be release ancestors and both documents
must remain unchanged in the release tree.

No fake reviewer, dates, pilot records, or image digest may replace missing
evidence. PostgreSQL GRANT/RLS, no-raw-SQL, audit/mutation controls, CI, native
Linux qualification, installer trust, signing, SBOM, and provenance remain
required. Future stable releases must use the ordinary independent-review,
reviewed-image, and two-pilot process, not inherit this waiver.

For the ordinary process, maintainers accept three external records and populate
`contracts/release-evidence-v1.json` with immutable HTTPS
evidence URLs, the reviewed commit/stable-manifest/image identity, dates, four
or more sorted weekly checkpoints per pilot, and zero unresolved P0/P1
findings. Pilot deployment identities must be full commits or image digests,
not mutable tags. Under either accepted process, run:

```sh
shasum -a 256 contracts/stable-v1.json
make stable-check
```

Until this command and all remaining release qualification/automation pass,
the repository is prepared for 1.0 qualification, not ready to publish
`v1.0.0`. Maintainer acceptance of the exception is not a claim that the release
has already been published.
