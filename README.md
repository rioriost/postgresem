# PostgreSQL Semantic Gateway

[日本語](README-jp.md)

`postgresem` is a PostgreSQL-native semantic gateway for AI agents and
applications. It accepts strict, versioned Logical Semantic Queries (LSQ),
resolves them against an immutable published semantic revision, and executes
deterministic parameterized `SELECT` queries through a guarded PostgreSQL
boundary.

The latest published release is **0.3.0-beta.1**. It is suitable for local
evaluation and governed read-only pilots, not production deployment.

## What problem does postgresem solve?

A PostgreSQL database already knows a great deal about its data: schemas,
tables, columns, types, keys, foreign keys, constraints, comments, privileges,
and row-level security policies. What it usually does not express completely
is the business meaning of that structure. An application or AI agent still
needs to know which table represents an order, which timestamp defines revenue
recognition, which joins are safe, which metric definition is approved, and
which fields a particular user is allowed to discover or query.

Without a governed semantic layer, those answers tend to be duplicated across
application code, BI tools, prompts, documentation, and YAML files. The copies
drift independently from the database and from one another. An AI agent that
sees only physical schema metadata may produce syntactically valid SQL that
uses the wrong grain, creates join fan-out, applies an inconsistent metric
definition, or bypasses the intended access path.

`postgresem` keeps the missing semantic contract in PostgreSQL alongside the
data. It combines database-native evidence such as `pg_catalog`, `COMMENT`,
PK/UNIQUE/FK, `CHECK`, GRANT, and RLS with explicitly reviewed models, fields,
relationships, metrics, terms, and policy bindings. These definitions are
published as immutable revisions. Agents query the approved semantic names
through LSQ instead of submitting raw SQL, and the deterministic compiler
either produces a bounded parameterized `SELECT` or rejects an ambiguous or
unsupported request.

Keeping this contract in PostgreSQL provides several practical benefits:

- **One governed source of truth:** physical metadata, business semantics,
  permissions, and revision history live under the same database operational
  boundary instead of being synchronized across a separate metadata service.
- **Security remains authoritative in the database:** PostgreSQL GRANT and RLS
  still enforce access at execution time. The semantic layer can narrow what
  is visible or queryable, but it cannot grant access the database denies.
- **Meaning changes with the data model:** semantic migrations, publication,
  backup, restore, and drift checks can be coordinated with the schema they
  describe.
- **Safer AI access:** agents discover approved concepts and construct typed
  LSQs; they do not receive an unrestricted SQL execution interface or need to
  infer business meaning from table and column names alone.
- **Lineage and audit by construction:** each result is tied to the semantic
  revision, metrics, relationships, source columns, policy context, compiler
  version, and SQL hash used to produce it.
- **Less infrastructure for PostgreSQL-centered systems:** the core contract
  requires no external catalog, vector database, or policy engine. PostgreSQL
  remains the durable system of record for both data and its governed meaning.

This approach is intentionally PostgreSQL-specific. It favors deep integration
with PostgreSQL types, catalog metadata, roles, RLS, transactions, and backup
procedures over a broad abstraction across many database dialects.

## Start here

- [30-minute Apple Container quickstart](docs/quickstart.md)
- [Commerce sample and stdio smoke client](examples/commerce/README.md)
- [Local commerce Web demo](examples/web_demo/README.md)
- [Operations guide](docs/operations.md)
- [Error reference](docs/error-reference.md)
- [Compatibility policy and support matrix](docs/compatibility.md)
- [Performance baseline and reproduction](docs/performance.md)
- [Developer-preview exit checklist](docs/developer-preview-checklist.md)
- [M5 beta checklist](docs/beta-checklist.md)
- [M5 external evidence process](docs/m5-external-evidence.md)
- [Backup and restore](docs/backup-restore.md)
- [SLO and adoption reporting](docs/slo-and-adoption.md)
- [Incident runbook](docs/incident-runbook.md)
- [Beta security review checklist](docs/security-review-checklist.md)
- [M4 design feedback form](https://github.com/rioriost/postgresem/issues/new?template=m4_design_feedback.yml)
- [Configured CI](.github/workflows/ci.yml) and
  [release automation](.github/workflows/release.yml)
- [Architecture decisions](docs/adr/)
- [Implementation plan](docs/POSTGRESQL_SEMANTIC_GATEWAY_IMPLEMENTATION_PLAN.md)

## What the preview implements

- LSQ v1 validation and deterministic compilation
- Semantic Snapshot/Schema v1 backed by PostgreSQL
- immutable published revisions with canonical hashes
- guarded read-only execution with row and byte limits
- fixed PostgreSQL role mapping with GRANT and RLS enforcement
- mandatory query audit lifecycle records
- MCP `2024-11-05` over line-delimited JSON-RPC stdio
- five semantic-only tools and three resource URI forms
- deterministic semantic model compatibility diffs with a breaking-change gate
- a 100-model compiler baseline and deterministic 100-relation catalog check
- local Apple Container Compose development stack using PostgreSQL 18

The MCP tools are `list_semantic_models`, `describe_semantic_model`,
`validate_semantic_query`, `query_semantic_model`, and
`explain_semantic_query`. There is no raw SQL or compiler-output MCP tool, and
MCP responses do not expose generated SQL or physical lineage.

## Security boundary

Runtime and audit credentials, project, mapped database role, principal, and
execution profile are fixed by environment at process startup; requests cannot
override them. Execution requires a durable `started` audit row, then uses a
`READ ONLY` transaction with `SET LOCAL ROLE` and transaction-local timeouts.
The executor rejects missing role membership, superuser or `BYPASSRLS` roles,
and roles that own a source relation used by the query.

Apple Container requires the gateway Compose configuration user to be root for
its `/etc/hosts` fallback. The startup command immediately drops to
`postgresem` for the idle process, and `make mcp` explicitly execs MCP as
`postgresem`; the application processes are unprivileged even though the
container configuration is not nonroot.

MCP diagnostics go to stderr as structured JSON and omit request values,
connection data, SQL, result rows, private names, and principal data. Hidden
and unknown semantic objects receive the same public “not available” errors.

## Preview limitations

- PostgreSQL connections require an explicit `sslmode`. Use
  `sslmode=require` for remote connections; `sslmode=disable` is accepted only
  as an explicit choice for local or independently protected connections.
- MCP is stdio only. There is no HTTP listener or remote authentication layer.
- Concurrent MCP cancellation is not implemented; PostgreSQL statement timeout
  is the cancellation boundary.
- N-1 and same-name restore paths are fixture-tested, but production backup,
  RPO/RTO, disaster recovery, and down migrations remain operator-owned.
- `v0.3.0-beta.1` checksums and immutable container image digest are keyless
  signed by the GitHub release workflow.
- PostgreSQL 18 is the verified local development target; PostgreSQL 16, 17,
  and 18 pass the Docker CI migration, integration, and recovery matrix. See the
  [compatibility matrix](docs/compatibility.md) for the exact boundary.

## Packaging status

Tag-triggered automation is configured to build four native archives, generate
`SHA256SUMS`, and publish a multi-architecture GHCR image with image SBOM and
provenance. [`scripts/install.sh`](scripts/install.sh) requires Cosign,
authenticates the signed `SHA256SUMS` against the exact release workflow/tag,
and then verifies the matching archive checksum before installation.

The
[`v0.3.0-beta.1` pre-release](https://github.com/rioriost/postgresem/releases/tag/v0.3.0-beta.1)
contains Linux and macOS archives for amd64 and arm64, `SHA256SUMS`, and its
Sigstore signature and certificate. The public image is
`ghcr.io/rioriost/postgresem:0.3.0-beta.1`. The checksum and immutable image
digest are GitHub OIDC keyless-signed; verification must constrain the expected
workflow identity and issuer. See the
[artifact matrix](docs/compatibility.md#artifact-release-and-runtime-matrix).

## Development

```sh
make doctor
make test
make check
```

The complete preview gate is:

```sh
make preview-check
```

Run the M4 compatibility and performance surfaces directly:

```sh
postgresem model diff --from BEFORE.json --to AFTER.json --fail-on-breaking
postgresem benchmark compiler \
  --models 100 --warmup 100 --iterations 1000 --threshold-ms 50
make test-performance
```

Both CLI commands emit structured JSON. The benchmark exits nonzero when p95
does not remain strictly below the threshold; model diff exits nonzero on a
breaking diff only when `--fail-on-breaking` is present. See
[performance.md](docs/performance.md) for scope and reference measurements.

See [CONTRIBUTING.md](CONTRIBUTING.md) and [SECURITY.md](SECURITY.md) before
submitting changes or reports.
