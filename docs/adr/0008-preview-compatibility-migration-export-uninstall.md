# ADR 0008: Preview compatibility, migration, export, and uninstall boundaries

- Status: Accepted
- Date: 2026-08-31

M5 operational decisions supersede the preview-only recovery and signing
status statements below; see
[ADR 0009](0009-beta-operations-transport-and-evidence.md).

## Context

M4 publishes a developer-preview contract spanning LSQ v1, Semantic
Snapshot/Schema v1, compiler semantics, MCP tools/resources, PostgreSQL
migrations, and locally built OCI images. Users need to know which changes are
compatible and how to preserve semantic metadata before an upgrade.

The implementation currently has forward-only migrations, immutable published
revisions, canonical hash verification, model export/diff commands, and a local
Compose volume. It does not have down migrations, N-1 upgrade tests,
backup/restore automation, a complete import command, a supported uninstaller,
or release signing. CI/release automation covers a PostgreSQL matrix, native
archives, checksums, multi-arch GHCR, and image SBOM/provenance. The
`v0.2.0-alpha.1` workflow provides the first published preview evidence.

## Decision

1. Use SemVer for package releases. Before 1.0, minor/prerelease releases may
   contain documented breaking changes; patch changes should remain compatible
   except for necessary security or correctness fixes.
2. Version serialized contracts independently:
   - LSQ `schema_version: "1"`;
   - Semantic Snapshot/Schema `schema_version: "1"`;
   - MCP protocol `2024-11-05`;
   - MCP tool schema `"1"`;
   - compiler semantic version `0.1.0`.
3. Fail closed on unsupported contract versions or semantic features. Never
   silently reinterpret an existing published revision.
4. Treat published revisions as immutable. A semantic change creates a new
   revision and is reviewed with `model diff`; revision-bound cursors are
   invalid after publication changes.
5. Keep database migrations forward-only, ordered, and append-only. Applied
   migration files are not edited. Migration execution precedes a newer
   gateway binary.
6. Define `model export` as the current semantic portability boundary. It
   exports one project's current published snapshot after strict loading and
   hash verification.
7. Explicitly exclude the following from `model export`: source data, audit
   history, import/catalog state, roles, grants, credentials, and a restorable
   database image.
8. Do not call retained local volumes a backup. Backup/restore automation,
   restore validation, and N-1 migration/recovery are M5 work.
9. Do not provide or document destructive schema/role/volume deletion as a
   supported uninstall in M4. Manual removal is DBA-owned, destructive, and
   requires prior export/backup and dependency review.
10. Claim PostgreSQL support only where current test evidence exists. For M4
    documentation, PostgreSQL 18 is locally verified and PostgreSQL 16, 17, and
    18 have passing Docker CI matrix evidence. Apple Container remains
    statically pinned to PostgreSQL 18 because `container-compose` does not
    interpolate image variables.
11. Distinguish release automation from published artifacts. Tag automation is
    configured for four native archives, `SHA256SUMS`, and a multi-arch GHCR
    image with image SBOM/provenance. Claim publication only for a matching
    successful release workflow; `v0.2.0-alpha.1` is the current evidence.
12. Treat installer checksum verification as integrity checking, not signing.
    Release signing remains unimplemented.

## Upgrade sequence

For a retained preview environment:

1. stop attached MCP processes;
2. record package and migration versions;
3. export current published projects;
4. take an environment-approved database backup if persistence matters;
5. review semantic diffs and release migration notes;
6. apply forward migrations;
7. start the newer gateway;
8. validate, explain, query, and inspect audit evidence;
9. retain the prior environment until acceptance.

Downgrade is not supported. If migration or validation fails, stop and recover
through the environment's independently tested database restore process rather
than editing migration history.

## Consequences

- Preview clients can reason about explicit contract versions and error rather
  than receive silently changed semantics.
- Breaking 0.x evolution remains possible, but must be visible in versions,
  ADRs, release notes, tests, and migration guidance.
- Semantic export is useful for review and reconstruction but cannot satisfy
  database recovery or uninstall requirements.
- M4 records configured PostgreSQL/release automation without converting
  configuration into passing-test or published-artifact claims.
- Checksums, image SBOM, and image provenance do not replace release signing.
- Production readiness still requires M5/M6 work and independent operational
  evidence.
