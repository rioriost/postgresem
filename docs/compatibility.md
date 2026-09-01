# Beta compatibility and release roadmap

## Version policy

The project uses Semantic Versioning. The latest published release is `0.4.0`
and the current source package is `0.5.0`. Before 1.0:

- patch/prerelease increments should preserve documented behavior except for
  security or correctness fixes;
- a `0.x` minor or new prerelease may contain documented breaking changes;
- no 0.x release is a production-readiness or long-term-support promise;
- release notes and a migration note are required for a known breaking change.

When practical, a public feature is deprecated for at least one preview minor
before removal. Unsafe, privacy-breaking, or incorrect behavior may be removed
immediately with release-note justification.

M6 is assigned to `0.4`, not `1.0`. It adds a separately versioned governed
mutation contract and release-blocking Linux amd64/arm64 runtime evidence.
Versions `0.5` through `0.9` are comparison-driven compatibility stages before
the stable `1.0` contract. PostgreSQL remains the only execution engine through
`1.0`; non-PostgreSQL dialect support is not part of this roadmap.

## Contract versions

| Contract | Current | Compatibility boundary |
|---|---|---|
| LSQ | `schema_version: "1"` | strict JSON; unknown fields rejected; only v1 accepted |
| Semantic Snapshot/Schema | `schema_version: "1"` | loader/compiler reject other snapshot versions |
| MCP protocol | `2024-11-05` | initialize returns this version; stdio JSON-RPC only |
| MCP tool schema | `"1"` | every tool requires this exact version |
| compiler semantics | `0.1.0` | recorded in published revisions/audit; deterministic output applies only to identical inputs and compiler semantics |
| LSM | `schema_version: "1"` | strict JSON; bounded insert and approved idempotent upsert only |
| mutation compiler semantics | `0.1.0` | deterministic output for identical LSM, published writable projection, and options |
| database migrations | `0001`–`0005` | forward-only; N-1 upgrade and same-name restore are tested; no down migrations |
| source package | `0.5.0` | M7 catalog drift, catalog-bound Ossie import, and reference evidence |
| latest published package | `0.4.0` | signed preview release; governed query and mutation contracts |

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

## PostgreSQL catalog drift

Capture catalog evidence before and after a schema, privilege, or RLS change
with `postgresem catalog scan`, then compare it with:

```sh
postgresem catalog diff \
  --from BEFORE-CATALOG.json \
  --to AFTER-CATALOG.json \
  --fail-on-breaking
```

Catalog diff verifies both canonical snapshot fingerprints and requires the
same database and introspection role. It reports deterministic JSON using
JSON-pointer paths and three classifications:

- `compatible`: relation or column additions;
- `review_required`: server-version changes, comments, and newly observed
  constraints;
- `breaking`: removals and changes to relation kinds, column types/nullability,
  effective grants, constraints, RLS state, or RLS policies.

The conservative classifier is a publication gate, not proof that a compatible
addition has correct business meaning. RLS and GRANT changes are always
breaking because the same structural shape can have a different authorization
boundary. A role change is rejected instead of being misreported as drift
because catalog visibility is role-dependent.

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

M7 adds a one-way Apache Ossie `0.1.1` candidate importer:

```sh
postgresem model import osi \
  --from semantic-model.yaml \
  --catalog catalog-snapshot.json
```

The importer requires fingerprinted catalog evidence, accepts only direct
single-column ANSI field expressions and supported single-field aggregate
metrics, and cross-checks PostgreSQL types, nullability, primary keys, foreign
keys, relation visibility, and database identity. It emits a reviewable
snapshot and warnings; it does not write or publish Semantic Schema rows and
never creates a writable model. Verified relationship fields are projected as
`<relationship>_<field>` names. The mutable Ossie `0.2.0.dev0` draft, custom
extensions, computed or multi-dialect expressions, unique-key semantics,
cross-dataset metrics, composite primary keys, and composite relationships are
rejected. Time-dimension roles require PostgreSQL `date` or
timestamp-without-time-zone evidence; the importer does not invent a timezone
for timestamp-with-time-zone fields.

M7 reference execution is independently reproducible through
[`tests/reference-comparison/runtime`](../tests/reference-comparison/runtime/)
and the dedicated
[`reference-comparison.yml`](../.github/workflows/reference-comparison.yml)
workflow. Run
[`33515921966`](https://github.com/rioriost/postgresem/actions/runs/33515921966)
executed Wren AI, Cube, Malloy, and MetricFlow against one PostgreSQL 18
dataset; every engine returned the expected `545.50`. This is query-result
evidence, not a claim that the engines share postgresem's no-raw-SQL,
immutable-publication, mutation, or PostgreSQL-authorization boundary.

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

Rust PostgreSQL connections require an explicit `sslmode`. `sslmode=require`
uses the platform-native TLS implementation and validates the server
certificate and hostname. `sslmode=disable` is retained for the loopback and
compose-local development paths. Omitted `sslmode` and downgrade-capable
`sslmode=prefer` are rejected.

## Artifact, release, and runtime matrix

| Artifact/runtime | Status |
|---|---|
| source checkout + Rust 1.85 build | supported developer path |
| Apple Container 1.0.0 + `container-compose` 1.1.0 on macOS arm64 | supported quickstart path |
| locally built `gateway:latest` OCI image | supported by `make dev-up`; Apple Container runs the image in a Linux arm64 VM on the documented macOS path |
| Docker/Docker Compose on Linux amd64 | exercised by GitHub Actions integration jobs; not yet the end-user quickstart |
| Linux amd64/arm64 runtime execution gate | native CI jobs execute the runtime image against PostgreSQL 18; tagged releases also execute each packaged binary and architecture-specific image before publication |
| native binary archives | `v0.4.0` published for Linux amd64/arm64 and macOS amd64/arm64 |
| `SHA256SUMS` | published with a keyless Sigstore signature and certificate |
| `scripts/install.sh` | supports macOS/Linux amd64/arm64, requires Cosign, verifies the exact release workflow/tag identity and `SHA256SUMS`; CI covers successful Linux architecture selection and failed-signature rejection |
| versioned GHCR image | public as `ghcr.io/rioriost/postgresem:0.4.0` |
| multi-architecture OCI manifest | published for `linux/amd64` and `linux/arm64`; index digest `sha256:de4a77a9852b227e444fb8938cdb9d93a20336740cd10427c445458536313bd2` |
| image SBOM and provenance | published by Docker Buildx with the release image |
| binary SBOM/provenance | not configured |
| cryptographic release signatures | `v0.4.0` checksums and immutable image digest are keyless-signed by the GitHub release workflow |
| MCP HTTP/server artifact | not implemented; the loopback Web demo is a sample adapter over stdio |

The [release workflow](../.github/workflows/release.yml) runs only for `v*`
tags, requires the tag to match the workspace version, builds the four native
archives, executes Linux amd64/arm64 packaged-binary smoke tests, executes both
native runtime images, generates `SHA256SUMS`, publishes the
multi-architecture image only after those gates, then creates a GitHub release.
[Release run 33494960050](https://github.com/rioriost/postgresem/actions/runs/33494960050)
completed successfully and published the
[`v0.4.0` release](https://github.com/rioriost/postgresem/releases/tag/v0.4.0).

The installer uses HTTPS, requires Cosign, verifies the signed checksum against
the exact repository release workflow and tag identity, verifies SHA-256
equality, and rejects unsafe archive paths and link entry types.

The local `gateway:latest` image is built from the current checkout. `latest`
is not a stable release identifier and must not be treated as a signed
supply-chain artifact.

## Known limitations

- beta, not production-ready;
- stdio MCP only; no HTTP, remote authentication, or TLS termination;
- remote PostgreSQL TLS requires a platform-trusted certificate and hostname;
  custom trust roots and client certificates are not yet configurable;
- no concurrent MCP cancellation;
- no connection pool or remote multi-user service;
- governed mutation is limited to published insert/upsert projections; no
  arbitrary update/delete/merge/copy/call/DDL surface;
- snapshot is reloaded per operation;
- semantic discovery is not a full source-GRANT preflight;
- strict subset of single-fact and safe many-to-one semantics; unsupported
  relationships/metrics fail closed;
- query row limit and result-byte truncation, with no result pagination;
- no down migrations, production backup retention, RPO/RTO guarantee, disaster
  recovery service, or uninstall;
- `0.5.0` release artifacts and signatures are pending the tag-triggered
  release gates;
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
