# PostgreSQL Semantic Gateway

`postgresem` is a PostgreSQL-native semantic gateway for AI agents and
applications. It accepts strict, versioned Logical Semantic Queries (LSQ),
resolves them against an immutable published semantic revision, and executes
deterministic parameterized `SELECT` queries through a guarded PostgreSQL
boundary.

The current release is **0.2.0-alpha.1 developer preview**. It is suitable for
local evaluation and read-only pilots, not production deployment.

## Start here

- [30-minute Apple Container quickstart](docs/quickstart.md)
- [Commerce sample and stdio smoke client](examples/commerce/README.md)
- [Operations guide](docs/operations.md)
- [Error reference](docs/error-reference.md)
- [Compatibility policy and support matrix](docs/compatibility.md)
- [Performance baseline and reproduction](docs/performance.md)
- [Developer-preview exit checklist](docs/developer-preview-checklist.md)
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

- PostgreSQL connections use `NoTls`; use only local or otherwise protected
  connections.
- MCP is stdio only. There is no HTTP listener or remote authentication layer.
- Concurrent MCP cancellation is not implemented; PostgreSQL statement timeout
  is the cancellation boundary.
- Backup/restore automation, N-1 migration testing, release signing, and
  production hardening are not implemented.
- PostgreSQL 18 is the currently verified development target. See the
  [compatibility matrix](docs/compatibility.md) before trying another version.

## Packaging status

Tag-triggered automation is configured to build four native archives, generate
`SHA256SUMS`, and publish a multi-architecture GHCR image with image SBOM and
provenance. [`scripts/install.sh`](scripts/install.sh) downloads a matching
archive and verifies its SHA-256 checksum before installation.

No release/tag, GitHub release, archive, checksum file, or GHCR release image
has been published yet. Release signing is not implemented; a checksum verifies
integrity against the downloaded checksum file, not publisher authenticity. See the
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
