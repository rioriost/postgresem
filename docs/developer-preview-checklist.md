# M4 developer-preview checklist

This checklist records evidence; it does not self-certify external adoption or
production readiness.

## M4 exit checklist

| Requirement | Status | Evidence |
|---|---|---|
| Apple Container diagnostics and local startup | implemented | `make doctor`, `make dev-up` |
| read-only commerce pilot quickstart | documented | [quickstart](quickstart.md) |
| representative sample project | implemented | [commerce example](../examples/commerce/README.md) |
| stdio smoke covers initialize, five tools, and all resources | implemented | `examples/commerce/mcp_smoke.py` |
| operations documentation | documented | [operations](operations.md) |
| stable preview error taxonomy | documented from source | [error reference](error-reference.md) |
| preview compatibility policy | documented | [compatibility](compatibility.md), [ADR 0008](adr/0008-preview-compatibility-migration-export-uninstall.md) |
| guarded read-only execution and mandatory audit | implemented | ADRs [0006](adr/0006-guarded-database-execution.md) and [0007](adr/0007-mcp-stdio-mvp-adapter.md) |
| performance baseline and 100-model/catalog checks | implemented; reference run recorded | [performance baseline](performance.md), `make test-performance` |
| PostgreSQL 16/17/18 GitHub matrix | passed | [CI run 33389810710](https://github.com/rioriost/postgresem/actions/runs/33389810710) |
| archives/checksums/multi-arch GHCR/SBOM/provenance | published | [`v0.2.0-alpha.1`](https://github.com/rioriost/postgresem/releases/tag/v0.2.0-alpha.1), [release run 33390223411](https://github.com/rioriost/postgresem/actions/runs/33390223411) |
| release signing | **not implemented** | checksums are not signatures |
| new user completes a real-database read-only pilot in 30 minutes | evidence required | use the form below |
| design feedback from at least two independent external users/groups | **outstanding** | cannot be self-certified; attach two distinct issue links when received |

M4 remains incomplete until the last two evidence requirements are satisfied by
actual independent users. Maintainer or automated smoke runs are useful but do
not count as external feedback.

Use the repository's
[M4 design feedback form](https://github.com/rioriost/postgresem/issues/new?template=m4_design_feedback.yml).
Its source is
[`.github/ISSUE_TEMPLATE/m4_design_feedback.yml`](../.github/ISSUE_TEMPLATE/m4_design_feedback.yml).
The form is the preferred evidence record; the expanded form below can be used
for private preparation before submitting a redacted issue.

## Reproducible 30-minute pilot evidence

Copy one form per participant. Do not include credentials, source data, full
database dumps, raw LSQ literals from private systems, generated SQL, or
unredacted logs.

```text
Participant/organization alias:
Independent of implementation team: yes/no
Repository commit:
Date/time and timezone:

Host:
  macOS version:
  hardware/architecture:
  Apple Container version:
  container-compose version:
  Rust version:
  PostgreSQL image:

Pilot database:
  repository commerce fixture / external non-production database:
  confirmation that credentials were non-production:
  mapped PostgreSQL role:
  RLS present: yes/no

Timing (wall clock from clone to successful audited query):
  start:
  finish:
  elapsed minutes:
  under 30 minutes: yes/no

Commands completed:
  [ ] clone and enter repository
  [ ] copy/configure .env
  [ ] make doctor
  [ ] make dev-up
  [ ] migration verification
  [ ] MCP initialize
  [ ] tools/list
  [ ] list_semantic_models
  [ ] describe_semantic_model
  [ ] validate_semantic_query
  [ ] explain_semantic_query
  [ ] query_semantic_model
  [ ] resources/list and resources/read
  [ ] safe audit inspection
  [ ] make dev-down

Observed output shapes:
  protocol version:
  tool count:
  resource count:
  validation valid:
  query columns/types:
  query row count:
  truncated:
  audit status/profile:

Failures/workarounds (redacted):
Documentation unclear or inaccurate:
Security/privacy concerns:
Compatibility observations:
Would this support a further read-only pilot? why/why not:
Issue link:
```

## Maintainer evidence commands

Run from a clean checkout with local-only `.env` values:

```sh
make doctor
make dev-up
python3 examples/commerce/mcp_smoke.py -- make mcp
make test-performance
make dev-down
```

Record commit, versions, elapsed time, exit status, and only privacy-safe output
summaries. A successful maintainer run proves reproducibility on that machine;
it does not replace the two independent feedback reports.

The latest documented reference numbers are in
[performance.md](performance.md#reference-apple-container-run). They are dated,
environment-specific evidence, not universal performance guarantees.

## Explicit non-claims

Completing this checklist does not claim production readiness, TLS, HTTP,
transport cancellation, backup/restore, N-1 upgrades, release signing, a
stable release, an external security review, or a supported uninstall. Those
boundaries remain in
[compatibility](compatibility.md) and [operations](operations.md).
