# ADR-002: Semantic Schema v1 and revision lifecycle

- Status: Accepted
- Date: 2026-08-31

## Context

Semantic definitions must be backed up, migrated, authorized, and audited with
the PostgreSQL data they describe. Published definitions must not change in
place, and persistent identifiers must survive relation recreation and
dump/restore operations.

## Decision

Store semantic metadata in the `semantic` schema. Use application-generated
UUIDs as persistent identifiers and canonical database/schema/relation/column
names for physical references. PostgreSQL OIDs are snapshot evidence only and
are not stable identifiers.

A project contains monotonically numbered revisions with these transitions:

```text
draft -> published -> retired
```

Only draft child objects are mutable. PostgreSQL constraints and triggers
enforce the lifecycle baseline; the application publish command will add full
cross-object, expression, type, grain, and authorization validation before a
revision can be published.

The initial schema includes projects, revisions, models, fields,
relationships, relationship columns, metrics, terms, policy bindings, source
snapshots, import runs and issues, lineage edges, and query audit records.
Expression bodies use versioned JSONB but cannot contain public raw SQL.

Migrations are forward-only SQL files. Each migration records its version in
`semantic.schema_migration` in the same transaction as its schema changes.

## Security boundaries

The schema is owned by `postgresem_owner`. Runtime, editor, publisher,
introspector, and auditor roles are separate. Public schema and object
privileges are revoked. Business query roles remain separate from metadata
roles and are entered only for guarded source transactions.

## Consequences

Published revisions are stable rollback and audit anchors. Some consistency
rules, including publish-time semantic validation and principal-filtered
metadata visibility, remain application responsibilities and must be
implemented before M1 and M3 exit gates.

