# ADR 0010: LSM v1 and governed insert/upsert

- Status: Accepted
- Date: 2026-09-01

## Context

Postgresem 0.3 exposes only Logical Semantic Query (LSQ). M6 adds data input
without exposing raw SQL, physical identifiers, arbitrary predicates, or a
general DML escape hatch. The compiler must remain deterministic and free of
database, transport, logging, and audit I/O.

Retries must not create duplicate rows. Writable fields, generated fields,
conflict keys, row limits, and returned fields therefore need to be part of the
published semantic revision rather than caller-selected options.

## Decision

1. Logical Semantic Mutation (LSM) v1 is a strict JSON document containing
   `schema_version`, `operation`, `model`, `idempotency_key`, and `rows`.
2. M6 supports only `insert` and `upsert`. `UPDATE`, `DELETE`, `MERGE`, `COPY`,
   `CALL`, DDL, raw SQL, expressions, physical table/column names, caller-chosen
   conflict targets, and caller-chosen returning fields are rejected.
3. Every request requires a non-empty idempotency key. A key can be replayed
   only when its canonical LSM hash and semantic revision match the committed
   request.
4. A published writable-model projection defines:
   - whether insert and upsert are enabled;
   - maximum request rows and bytes;
   - insertable and required fields;
   - fields mutable during conflict handling;
   - the complete ordered conflict key; and
   - the fields returned after mutation.
5. All rows in one request must contain the same semantic field set. This
   produces one bounded parameterized statement and one atomic PostgreSQL
   transaction.
6. The compiler sorts semantic input fields deterministically, emits one
   parameterized `INSERT` or approved `INSERT ... ON CONFLICT DO UPDATE`, and
   requires the returned row count to equal the requested row count.
7. Null is an explicit typed mutation value and is accepted only for fields
   published as nullable. Generated, relationship-backed, unknown, duplicate,
   non-writable, missing required, or immutable upsert fields fail closed.
8. The compiler output contains physical SQL for the executor, but CLI and MCP
   mutation responses never expose it.

## Consequences

The 0.4 mutation surface is intentionally narrower than PostgreSQL DML.
Defaults and triggers may still execute inside PostgreSQL, but callers cannot
select them. Adding update/delete or semantic predicates requires a later ADR
and a new contract version.

