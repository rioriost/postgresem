# PostgreSQL Semantic Gateway

[日本語](README-jp.md)

`postgresem` is a PostgreSQL-native semantic gateway for AI agents and
applications. It accepts strict, versioned Logical Semantic Queries (LSQ) and
Logical Semantic Mutations (LSM), resolves them against an immutable published
semantic revision, and executes deterministic parameterized operations through
separate guarded PostgreSQL query and mutation boundaries.

The current source version is the **0.9.0 release candidate**; the latest
published release is **0.7.0**. M11 freezes the candidate contracts, adds
previous-binary rollback and query/ingestion operator gates, and publishes
support, governance, and deprecation policy. It is not a production-readiness
or SLA promise.

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

## Roadmap to 1.0

The current source implements the repository-controlled M11 scope as `0.9`,
not `1.0`. M6's separate typed
mutation contract remains limited to bounded inserts and explicitly modeled
idempotent upserts. M7 adds catalog-bound Apache Ossie `0.1.1` candidate import
and authorization-aware catalog drift without weakening the existing
`READ ONLY` query executor or exposing raw SQL, arbitrary DML, physical
identifiers, or request-selected database roles. PostgreSQL GRANT, RLS
`WITH CHECK`, constraints, and triggers remain authoritative.

`0.4` also adds native Linux amd64 and arm64 runtime gates for both packaged
binaries and runtime images instead of treating cross-built archives or a
multi-architecture manifest as execution evidence. The Mac Studio and Apple
Container remain the maintainer's local reference environment, not the only
supported target.

M7 ran pinned Wren AI, Cube, Malloy, and MetricFlow OSS runtimes against one
PostgreSQL 18 dataset. Every reference produced the same expected aggregate,
while the comparison kept their materially different trust boundaries
explicit. M8 uses that evidence to add explicit metric aggregation anchors and
a two-stage PostgreSQL plan that removes duplicate child rows at the declared
root entity grain before applying the requested aggregate. M9 adds a stateless
MCP `2026-07-28` HTTP resource server without moving identity or authorization
out of PostgreSQL. M10 removes the measured catalog N+1 bottleneck, adds
catalog-bound large-model scaffolding and operational/upgrade surfaces, and
keeps persisted acceleration deferred because guarded execution was not the
measured bottleneck. M11 freezes these contracts and adds release-candidate
operation and rollback gates. Independent external security review and two
28-day non-fixture pilots remain outstanding in
[issue #4](https://github.com/rioriost/postgresem/issues/4). Feature-count parity
is not the objective: PostgreSQL remains the only execution engine and semantic
source of truth through `1.0`.

See the [implementation plan](docs/POSTGRESQL_SEMANTIC_GATEWAY_IMPLEMENTATION_PLAN.md)
for the M6–M12 gates.

## Start here

- [30-minute Apple Container quickstart](docs/quickstart.md)
- [Linux Docker Compose and Podman Quadlet](docs/linux-containers.md)
- [Commerce sample and stdio smoke client](examples/commerce/README.md)
- [Authenticated MCP HTTP deployment and SDK guidance](docs/mcp-http.md)
- [Local commerce Web demo](examples/web_demo/README.md)
- [Operations guide](docs/operations.md)
- [M11 release-candidate checklist](docs/m11-release-candidate-checklist.md)
- [Release-candidate operator workflow](docs/rc-operator-workflow.md)
- [Support policy](SUPPORT.md)
- [Governance](GOVERNANCE.md)
- [Deprecation policy](docs/deprecation-policy.md)
- [Error reference](docs/error-reference.md)
- [Compatibility policy and support matrix](docs/compatibility.md)
- [M10 reference comparison](docs/reference-comparison/2026-09-03.md)
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

## What 0.9 implements

- LSQ v1 validation and deterministic compilation
- Semantic Snapshot/Schema v2 backed by PostgreSQL, with Snapshot v1 loading
  and canonical-hash compatibility
- explicit metric additivity and root entity-key aggregation anchors
- deterministic two-stage aggregation across approved direct one-to-many
  dimensions and filters, with duplicate-child and multi-branch protection
- immutable published revisions with canonical hashes
- guarded read-only execution with row and byte limits
- LSM v1 validation and deterministic bounded insert/approved-upsert compilation
- separate writer credentials, mapped writer roles, and guarded mutation
  transactions with idempotent replay and reconciliation
- fixed PostgreSQL role mapping with GRANT and RLS enforcement
- mandatory query and mutation audit lifecycle records
- MCP `2024-11-05` over line-delimited JSON-RPC stdio
- authenticated stateless MCP `2026-07-28` over loopback Streamable HTTP,
  with RFC 9728 metadata and local asymmetric JWT verification
- exact verified subject-to-query/writer-role mappings, per-authority limits,
  private discovery, request SSE, and disconnect-to-PostgreSQL cancellation
- eight mutation-enabled semantic-only tools and four resource URI forms
- deterministic semantic model compatibility diffs with a breaking-change gate
- fingerprinted PostgreSQL catalog drift with GRANT, RLS, role authorization,
  complete role-graph evidence, security-definer view-owner authority,
  normalized object ACLs, executable function/window/aggregate evidence,
  relation ownership, constraint, and type changes treated as breaking evidence
- one-way Apache Ossie `0.1.1` import into a reviewable, query-only candidate
  that is cross-checked against PostgreSQL catalog evidence
- pinned Wren AI, Cube, Malloy, and MetricFlow runtime comparisons against one
  PostgreSQL 18 task, with machine-readable evidence
- an M10 scale baseline with 1,000-model compilation, deterministic
  1,000-relation catalog scans, and guarded-execution result hashing
- set-based PostgreSQL catalog scanning with a 1-second 1,000-relation
  regression gate
- deterministic catalog-bound scaffolding for up to 1,000 review-only models
- a fixed, privacy-preserving M10 operational dashboard
- verified-backup-gated local Apple Container upgrade automation
- a deterministic frozen release-candidate contract inventory
- previous-release binary execution after isolated same-name restore
- a combined guarded-query, governed-ingestion, replay, and audit workflow gate
- local Apple Container Compose development stack using PostgreSQL 18
- Linux Docker Compose and rootless Podman Quadlet deployment paths

The MCP tools are `list_semantic_models`, `describe_semantic_model`,
`validate_semantic_query`, `query_semantic_model`, and
`explain_semantic_query`, plus `validate_semantic_mutation`,
`mutate_semantic_model`, and `reconcile_semantic_mutation` when mutation
configuration is present. There is no raw SQL or compiler-output MCP tool, and
MCP responses do not expose generated SQL or physical lineage.

## Security boundary

Runtime and audit credentials, project, mapped database role, principal, and
execution profile are fixed by environment at process startup; requests cannot
override them. Execution requires a durable `started` audit row, then uses a
`READ ONLY` transaction with `SET LOCAL ROLE` and transaction-local timeouts.
The executor rejects missing role membership, superuser or `BYPASSRLS` roles,
and roles that own a source relation used by the query.

Mutation uses a distinct login, mapped writer role, compiler, executor,
idempotency store, and audit lifecycle. Business DML, the committed
idempotency result, and the terminal committed audit state share one
transaction. PostgreSQL column GRANT, RLS `USING`/`WITH CHECK`, constraints,
and triggers remain the final authority.

The HTTP adapter is only an OAuth resource server. It reads a strict authority
document, JWKS, and principal HMAC key from local read-only files, maps exact
verified JWT subjects to preconfigured roles, and binds only to loopback behind
a colocated HTTPS reverse proxy. It does not issue tokens, fetch remote keys,
trust forwarded identity headers, or accept request-selected roles. Remote
mutation is disabled unless the operator gate, verified scope, mapped writer
role, and existing PostgreSQL mutation boundary are all active.

Apple Container requires the gateway Compose configuration user to be root for
its `/etc/hosts` fallback. The startup command immediately drops to
`postgresem` for the idle process, and `make mcp` explicitly execs MCP as
`postgresem`; the application processes are unprivileged even though the
container configuration is not nonroot.

MCP diagnostics go to stderr as structured JSON and omit request values,
connection data, SQL, result rows, private names, and principal data. Hidden
and unknown semantic objects receive the same public “not available” errors.

## Beta limitations

- PostgreSQL connections require an explicit `sslmode`. Use
  `sslmode=require` for remote connections; `sslmode=disable` is accepted only
  as an explicit choice for local or independently protected connections.
- The authenticated HTTP listener does not terminate TLS and cannot bind to a
  non-loopback address. A colocated HTTPS reverse proxy must preserve the
  public Host, disable SSE buffering, and propagate disconnects.
- HTTP authority/JWKS reload, runtime OIDC discovery, distributed rate-limit
  state, resumable sessions, GET event streams, and connection pooling are not
  implemented.
- N-1 and same-name restore paths are fixture-tested, but production backup,
  RPO/RTO, disaster recovery, and down migrations remain operator-owned.
- The M10 operational report observes materialized-view state but does not
  create, refresh, or route queries to materialized views or pre-aggregations.
- `v0.7.0` checksums and immutable container image digest are keyless
  signed by the GitHub release workflow.
- PostgreSQL 18 is the verified local development target; PostgreSQL 16, 17,
  and 18 pass the Docker CI migration, integration, and recovery matrix. See the
  [compatibility matrix](docs/compatibility.md) for the exact boundary.
- Native Linux amd64/arm64 CI runtime gates execute the runtime image against
  PostgreSQL 18; tagged releases additionally gate packaged binaries and both
  architecture-specific images before publication.
- Governed writes are limited to published insert/upsert projections. Update,
  delete, merge, copy, calls, DDL, raw SQL, caller-selected conflict targets,
  and caller-selected returning fields remain unsupported.
- Fan-out-safe aggregation is limited to one root model, direct one-to-many
  relationships, one shared root entity-key anchor, and root-local metric
  inputs/filters. It does not allocate facts across groups or support
  multi-fact, bridge, reverse, or multi-hop planning.
- Ossie import is intentionally one-way and supports only direct ANSI fields,
  single-column key-backed relationships, and approved single-field
  aggregates. Unsupported or lossy semantics fail closed.

## Packaging status

Tag-triggered automation is configured to build four native archives, generate
`SHA256SUMS`, and publish a multi-architecture GHCR image with image SBOM and
provenance. [`scripts/install.sh`](scripts/install.sh) requires Cosign,
authenticates the signed `SHA256SUMS` against the exact release workflow/tag,
and then verifies the matching archive checksum before installation.

The
[`v0.7.0` release](https://github.com/rioriost/postgresem/releases/tag/v0.7.0)
contains Linux and macOS archives for amd64 and arm64, native Linux binary and
image runtime evidence, `SHA256SUMS`, and its Sigstore signature and
certificate. The public image is `ghcr.io/rioriost/postgresem:0.7.0`. The
checksum and immutable image digest are GitHub OIDC keyless-signed;
verification must constrain the expected workflow identity and issuer. See the
[artifact matrix](docs/compatibility.md#artifact-release-and-runtime-matrix).

## Development

```sh
make doctor
make test
make check
```

The complete repository release-candidate gate is:

```sh
make rc-check
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
