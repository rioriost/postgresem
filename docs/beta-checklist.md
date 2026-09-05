# M5 beta checklist

This checklist separates repository implementation from operational evidence
that must come from independent environments.

| Requirement | Status | Evidence |
|---|---|---|
| beta scope and transport decision | implemented | [ADR 0009](adr/0009-beta-operations-transport-and-evidence.md) |
| N-1 migration validation | implemented | `make test-recovery` builds `0001`–`0003`, executes, then upgrades; [PostgreSQL 16/17/18 CI](https://github.com/rioriost/postgresem/actions/runs/33399102194) passed |
| backup and same-name restore validation | implemented for isolated fixture | [backup/restore](backup-restore.md); [PostgreSQL 16/17/18 CI](https://github.com/rioriost/postgresem/actions/runs/33399102194) passed; production policy remains operator-owned |
| failure-recovery checks | implemented for isolated fixtures | mandatory audit failures, unsafe roles, timeouts, crash-like incomplete audit detection and explicit reconciliation, N-1, and restore paths passed in [CI run 33465942053](https://github.com/rioriost/postgresem/actions/runs/33465942053); production incident policy remains operator-owned |
| local SLO/adoption report | implemented | [SLO and adoption](slo-and-adoption.md), `postgresem report beta` |
| incident runbook | documented | [incident runbook](incident-runbook.md) |
| release signing verification | implemented and exercised | [`v0.3.0-beta.1`](https://github.com/rioriost/postgresem/releases/tag/v0.3.0-beta.1) checksum and image signatures verified against the release workflow identity |
| security review preparation | documented | [security review checklist](security-review-checklist.md) |
| internal security pre-review | completed | workflow supply-chain finding remediated by immutable action pinning and retested; [CI run 33465942053](https://github.com/rioriost/postgresem/actions/runs/33465942053) passed; does not satisfy independent review |
| external evidence collection workflow | implemented | [M5 external evidence process](m5-external-evidence.md) and structured field/security review issue forms |
| MCP Streamable HTTP | deferred | authentication and request identity are prerequisites |
| loopback semantic comparison demo | implemented | [Meaning Lab](../examples/semantic_demo/README.md); real PostgreSQL reads and governed writes through stdio MCP |
| automated source security review and remediation | **accepted under v1.0.0 maintainer exception** | [immutable review and post-fix evidence](https://github.com/rioriost/postgresem/blob/2797160ee431ee12722d339e23def6d8c8e7fbd5/docs/security-reviews/2026-09-05-381fe57.md); [ADR 0020](adr/0020-v1-release-maintainer-exception.md) |
| independent security review and reviewed container image | **waived for v1.0.0; not completed** | maintainer-approved source review is not independent external review; no image review was performed |
| two non-fixture databases operated for four weeks | **waived for v1.0.0; not completed** | [ADR 0020](adr/0020-v1-release-maintainer-exception.md); historical evidence goal tracked in [#4](https://github.com/rioriost/postgresem/issues/4) |
| P0/P1 security or correctness defects during field period | **not measured / accepted limitation** | no qualifying field periods or accepted pilot records exist; this is not a zero-defect claim |
| machine-readable 1.0 evidence gate | required; v1.0.0 exception approved | `contracts/release-evidence-v1.json`; accepted evidence must pass strict identity/decision binding and unchanged technical qualification before publication |

## External security review status (2026-09-05)

This corrects the earlier description of a completed external review: the
remotely supplied report is an **automated source review**, not an independent
external review. Its remediation and retest evidence is accepted by project
owner `rioriost` under [ADR 0020](adr/0020-v1-release-maintainer-exception.md),
expressly approved in the release implementation conversation on 2026-09-05
for the exact tag `v1.0.0` only.

Acceptance binds corrected source commit
`c8a2ca7a6a635de975d8e8b2324b652ac037075c` and reviewed stable-manifest digest
`sha256:c3d58c9fd3670836da7f86c73c478dcee4a087601cb083a927e3f7617e4f18a2`.
The [immutable report](https://github.com/rioriost/postgresem/blob/2797160ee431ee12722d339e23def6d8c8e7fbd5/docs/security-reviews/2026-09-05-381fe57.md)
preserves its original `381fe57` review and historical hashes. The corrected
source retest date is 2026-09-05 with zero unresolved P0/P1 source findings;
this says nothing about unmeasured field defects. New release-gate/inventory
hashes are not independently reviewed.

No image review was performed and no qualifying pilots were completed.
The reviewed image is therefore `null`, field pilots are `[]`, and the
exception explicitly waives those prerequisites and reviewer independence
rather than fabricating evidence. The release record also requires a genuine
immutable commit-pinned URL for ADR 0020 once its policy commit exists; a
relative documentation link does not satisfy that machine gate.

The maintainer also confirms that potential future supply-chain
vulnerabilities are an ongoing monitoring responsibility, not a current release
blocker by themselves. Existing dependency checks, action pinning, signature
verification, and release gates remain in place; newly discovered findings
must still be triaged under the security policy.

The [ordinary external evidence process](m5-external-evidence.md) remains the
default for later stable releases; they cannot inherit this exception.
Historical M5 evidence remains incomplete. `v1.0.0` publication still requires
technical qualification and release automation; this acceptance does not
claim an already published release.

## Beta SLO candidates

| Indicator | Objective |
|---|---|
| validation plus compilation latency | p95 below 50 ms for the documented 100-model warm benchmark |
| mandatory audit lifecycle | zero incomplete audit records after the configured recovery interval |
| supported semantic correctness cases | 100% pass |
| security integration cases | 100% pass |
| unsupported or unsafe requests | explicit rejection; never success-shaped |

Database execution latency is reported separately because it depends on source
data size, indexes, locks, and deployment topology.

## Explicit non-claims

M5 repository work does not by itself establish production readiness, remote
HTTP safety, a recovery-time guarantee, a recovery-point guarantee, regulatory
compliance, successful external adoption, or completion of an independent
security review.
