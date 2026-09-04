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
| loopback commerce Web demo | implemented | [Web demo](../examples/web_demo/README.md); existing stdio MCP path |
| independent security review | **outstanding** | external evidence required; tracked in [#4](https://github.com/rioriost/postgresem/issues/4) |
| two non-fixture databases operated for four weeks | **outstanding** | one evidence record per database is required; tracked in [#4](https://github.com/rioriost/postgresem/issues/4) |
| P0/P1 security or correctness defects during field period | **not measurable yet** | accepted 28-day field records are not yet available; tracked in [#4](https://github.com/rioriost/postgresem/issues/4) |
| machine-readable 1.0 evidence gate | implemented, pending evidence | `contracts/release-evidence-v1.json`; `v1.0.0` release validation fails closed until accepted URLs and immutable identities replace pending values |

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
