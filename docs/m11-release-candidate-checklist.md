# M11 release-candidate checklist

This checklist separates repository readiness from external evidence. It must
not self-certify production operation.

| Requirement | Status | Evidence |
|---|---|---|
| frozen candidate contracts | implemented | [ADR 0017](adr/0017-release-candidate-contract-freeze.md), `postgresem contract show`, `contracts/rc-v1.json` |
| unreviewed contract-drift gate | implemented | `make test-contracts`, CI |
| independent technical security review | completed for commit `c0984d3` | separate read-only security-review agent found no P0/P1-equivalent defect; this is not external review |
| query and ingestion operator workflow | implemented | [operator workflow](rc-operator-workflow.md), `make test-rc-workflow` |
| N-1 upgrade and same-name recovery | implemented | `make test-recovery` |
| previous-release binary rollback rehearsal | implemented | PostgreSQL 18 CI rebuilds immutable M10 commit `c0984d3` and runs it after restore |
| support policy | documented | [`SUPPORT.md`](../SUPPORT.md) |
| governance and release cadence | documented | [`GOVERNANCE.md`](../GOVERNANCE.md) |
| deprecation policy | documented | [deprecation policy](deprecation-policy.md) |
| P0/P1 repository defects | none known | full RC gate and security review |
| independent external security review | **outstanding** | tracked in [issue #4](https://github.com/rioriost/postgresem/issues/4) |
| two accepted 28-day non-fixture database pilots | **outstanding** | tracked in [issue #4](https://github.com/rioriost/postgresem/issues/4) |
| unresolved field P0/P1 defects | **not measurable** | external field periods are not complete |

The source can be repository-RC-ready while the M11 exit gate remains
incomplete. M11 completion requires the final three external-evidence rows to
be resolved with immutable, privacy-safe evidence.
