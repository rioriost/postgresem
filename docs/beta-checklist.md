# M5 beta checklist

This checklist separates repository implementation from operational evidence
that must come from independent environments.

| Requirement | Status | Evidence |
|---|---|---|
| beta scope and transport decision | implemented | [ADR 0009](adr/0009-beta-operations-transport-and-evidence.md) |
| N-1 migration validation | planned | must preserve the published revision and guarded-query behavior |
| semantic/audit backup and disposable restore validation | planned | source database and cluster-level backup remain operator-owned |
| failure-recovery checks | planned | migration, audit, timeout, and unavailable-database cases |
| local SLO/adoption report | planned | aggregate audit data only; no external telemetry by default |
| incident runbook | planned | detection, containment, evidence, recovery, and communication |
| release signing verification | planned | prefer GitHub OIDC/keyless identity |
| MCP Streamable HTTP | deferred | authentication and request identity are prerequisites |
| loopback commerce Web demo | planned | existing stdio MCP path; no raw SQL or browser-selected role |
| independent security review | **outstanding** | cannot be self-certified |
| two non-fixture databases operated for four weeks | **outstanding** | external evidence required |
| P0/P1 security or correctness defects during field period | **not measurable yet** | field period has not started |

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

