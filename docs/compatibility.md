# Developer-preview compatibility

## Version policy

The project uses Semantic Versioning. The latest published release is
`0.3.0-beta.1`. Before 1.0:

- patch/prerelease increments should preserve documented behavior except for
  security or correctness fixes;
- a `0.x` minor or new prerelease may contain documented breaking changes;
- no 0.x release is a production-readiness or long-term-support promise;
- release notes and a migration note are required for a known breaking change.

When practical, a public feature is deprecated for at least one preview minor
before removal. Unsafe, privacy-breaking, or incorrect behavior may be removed
immediately with release-note justification.

## Contract versions

| Contract | Current | Compatibility boundary |
|---|---|---|
| LSQ | `schema_version: "1"` | strict JSON; unknown fields rejected; only v1 accepted |
| Semantic Snapshot/Schema | `schema_version: "1"` | loader/compiler reject other snapshot versions |
| MCP protocol | `2024-11-05` | initialize returns this version; stdio JSON-RPC only |
| MCP tool schema | `"1"` | every tool requires this exact version |
| compiler semantics | `0.1.0` | recorded in published revisions/audit; deterministic output applies only to identical inputs and compiler semantics |
| database migrations | `0001`–`0004` | forward-only; N-1 upgrade and same-name restore are tested; no down migrations |
| package | `0.3.0-beta.1` | current beta package |

LSQ v1 names the serialized shape and current type/time/null semantics. Before
1.0, a breaking shape or meaning change must either increment the LSQ schema
version or be called out as a preview-breaking correction. The current binary
does not negotiate or run multiple LSQ versions.

Semantic Schema v1 is both normalized PostgreSQL metadata and its strict
`SemanticSnapshot` projection. Published revisions are immutable and
hash-verified. Unsupported normalized features fail closed rather than being
silently ignored.

MCP tool additions are compatible when existing request/response meanings do
not change. Removing/renaming a tool, changing strict arguments, changing
public result meaning, changing resource URI forms, or changing error meaning
is breaking. Clients must ignore unknown response fields, but tool request
objects currently reject unknown properties.

## Semantic model changes

Use:

```sh
postgresem model diff \
  --from BEFORE.json --to AFTER.json --fail-on-breaking
```

From a source checkout, replace `postgresem` with
`cargo run -p postgresem --`.

The command normalizes both snapshots and emits deterministic compatibility
JSON with this shape:

```json
{
  "schema_version": "1",
  "from_revision": "sha256:<64 hex>",
  "to_revision": "sha256:<64 hex>",
  "compatibility": "compatible | breaking",
  "summary": {"total": 0, "compatible": 0, "breaking": 0},
  "changes": [
    {
      "path": "models.<name>...",
      "object_kind": "model | field | metric | relationship",
      "change": "added | removed | modified",
      "compatibility": "compatible | breaking",
      "before": {},
      "after": {}
    }
  ]
}
```

Changes are sorted by semantic path and then removed, modified, added. Optional
`before`/`after` values are omitted where not applicable. JSON is printed even
when `--fail-on-breaking` makes a breaking result exit nonzero. Without that
flag, a breaking classification is reported but does not itself fail the
command.

The implemented diff marks removal of a visible model/field/metric, source or
timezone changes, queryable-to-nonqueryable changes, visible object
modifications, and relationship modifications as breaking. Additions and
changes involving only previously hidden objects can be compatible. This is a
preview classifier, not a complete business-semantic proof.

Model-list cursors include the revision hash. A publication between pages makes
the old cursor invalid; clients must relist.

Performance measurements are a separate environment-specific signal. See the
[M4 performance baseline](performance.md); passing it does not make a semantic
change compatible.

## Migration and upgrade compatibility

Migrations are forward-only and recorded in `semantic.schema_migration`.
Rerunning the current migration runner skips recorded versions. Applied
migration files must never be edited or reordered.

The repository tests the current binary against the N-1 schema, upgrades it to
the current schema, and restores a full fixture backup under the original
database name before running a guarded query. Down migrations and a general
expand/contract rollout guarantee are not implemented. Follow
[backup and restore](backup-restore.md) and do not assume downgrade
compatibility.

`model export` is the implemented semantic portability boundary. It exports one
project's current published snapshot, not a full database backup or uninstall
bundle. See [ADR 0008](adr/0008-preview-compatibility-migration-export-uninstall.md).

## PostgreSQL support

| PostgreSQL | Preview status | Evidence/boundary |
|---|---|---|
| 18 | supported for the documented local pilot and CI | static Apple Container image, local integration target, and passing Docker Actions matrix |
| 17 | supported in Docker CI | migrations, database integration, guarded execution, and MCP integration passed |
| 16 | supported in Docker CI | migrations, database integration, guarded execution, and MCP integration passed |
| 15 and older | unsupported | outside the project plan and current test target |
| future major versions | unsupported until evaluated | catalog and behavior changes require explicit validation |

The [CI workflow](../.github/workflows/ci.yml) defines separate PostgreSQL 16,
17, and 18 jobs for migrations, database integration, guarded execution, MCP
integration, N-1 migration, and backup/restore recovery. Integration images
use the matching PostgreSQL client major. The performance service runs only in
the PostgreSQL 18 matrix job. All three jobs passed in
[CI run 33399102194](https://github.com/rioriost/postgresem/actions/runs/33399102194)
on 2026-08-31. Core operation requires no PostgreSQL extension.

Apple Container uses the static `postgres:18` image in `compose.yaml`;
`container-compose` does not interpolate `${POSTGRES_IMAGE}`. Local version
selection through `.env` is therefore neither supported nor documented.
`compose.ci.yaml` is a Docker Compose overlay selected by CI through
`COMPOSE_FILE`; it is not an Apple Container version-selection mechanism.

All current Rust PostgreSQL connections use `NoTls`; even a supported server
version must be local or reached through an independently protected channel.

## Artifact, release, and runtime matrix

| Artifact/runtime | Status |
|---|---|
| source checkout + Rust 1.85 build | supported developer path |
| Apple Container 1.0.0 + `container-compose` 1.1.0 on macOS arm64 | supported quickstart path |
| locally built `gateway:latest` OCI image | supported by `make dev-up`; Linux arm64 on the documented M4 path |
| Docker/Docker Compose | doctor can detect it, but the Make targets invoke `container-compose`; not the documented M4 path |
| native binary archives | `v0.3.0-beta.1` published for Linux amd64/arm64 and macOS amd64/arm64 |
| `SHA256SUMS` | published with a keyless Sigstore signature and certificate |
| `scripts/install.sh` | supports macOS/Linux amd64/arm64, verifies `SHA256SUMS`, and was exercised against the published `v0.3.0-beta.1` macOS arm64 archive |
| versioned GHCR image | public as `ghcr.io/rioriost/postgresem:0.3.0-beta.1` |
| multi-architecture OCI manifest | published for `linux/amd64` and `linux/arm64`; index digest `sha256:b2f67b4a8da954b129b93a47641a55810ce36772d3efc6960a39bdaaad7a282d` |
| image SBOM and provenance | published by Docker Buildx with the release image |
| binary SBOM/provenance | not configured |
| cryptographic release signatures | `v0.3.0-beta.1` checksums and immutable image digest are keyless-signed by the GitHub release workflow |
| MCP HTTP/server artifact | not implemented; the loopback Web demo is a sample adapter over stdio |

The [release workflow](../.github/workflows/release.yml) runs only for `v*`
tags, requires the tag to match the workspace version, builds the four native
archives, generates `SHA256SUMS`, publishes the multi-architecture image, then
creates a GitHub release.
[Release run 33399332825](https://github.com/rioriost/postgresem/actions/runs/33399332825)
completed successfully and published the
[`v0.3.0-beta.1` pre-release](https://github.com/rioriost/postgresem/releases/tag/v0.3.0-beta.1).

The installer uses HTTPS and verifies SHA-256 equality, and also rejects unsafe
archive paths and link entry types. `v0.3.0-beta.1` publishes a signed checksum;
publisher authentication requires the workflow identity checks in
[release verification](release-verification.md), because the installer itself
does not invoke Cosign.

The local `gateway:latest` image is built from the current checkout. `latest`
is not a stable release identifier and must not be treated as a signed
supply-chain artifact.

## Known limitations

- developer preview, not production-ready;
- stdio MCP only; no HTTP, remote authentication, or TLS termination;
- PostgreSQL client uses `NoTls`;
- no concurrent MCP cancellation;
- no connection pool or remote multi-user service;
- snapshot is reloaded per operation;
- semantic discovery is not a full source-GRANT preflight;
- strict subset of single-fact and safe many-to-one semantics; unsupported
  relationships/metrics fail closed;
- query row limit and result-byte truncation, with no result pagination;
- no down migrations, production backup retention, RPO/RTO guarantee, disaster
  recovery service, or uninstall;
- the current published release is unsigned; future signing is configured but
  has no published evidence yet;
- external feedback from two independent users remains an M4 exit dependency.

## Breaking-change checklist

A change is breaking when an existing valid client/query/revision can no longer
be used or has materially different semantics. Before merging such a preview
change:

1. write/update an ADR for semantic, authorization, migration, or protocol
   changes;
2. update the relevant contract version when required;
3. add accepted/rejected compatibility tests;
4. provide semantic model diff and database migration guidance;
5. update error, quickstart, operations, and release notes;
6. identify whether existing published revisions can still load safely;
7. prefer explicit rejection over silent reinterpretation.
