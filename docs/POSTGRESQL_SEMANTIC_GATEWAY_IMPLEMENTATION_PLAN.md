# PostgreSQL Semantic Gateway Implementation Plan

- Project name: `postgresem` / PostgreSQL Semantic Gateway
- Document status: Living implementation and release plan
- Created: 2026-08-31
- Last revised: 2026-09-01
- Target environments: Linux amd64 and arm64 for required runtime support; macOS amd64 and arm64 for development and native archives; Apple silicon Mac Studio with Apple Container as the maintainer reference environment; PostgreSQL 16–18
- Translations: [Japanese](POSTGRESQL_SEMANTIC_GATEWAY_IMPLEMENTATION_PLAN-jp.md)

## 1. Executive Summary

PostgreSQL Semantic Gateway aims to be an OSS that treats PostgreSQL itself not only as a store for business data but also as the Semantic Source of Truth for data meaning, exposing governed semantics, metrics, relationships, permissions, and lineage to AI agents and applications via MCP.

The core value proposition is not Text-to-SQL. Instead of having an LLM write SQL directly, it assembles versioned Logical Semantic Queries (LSQs). The Gateway rigorously validates LSQs and compiles them into deterministic, parameterized SQL using approved Semantic Models and PostgreSQL permissions.

```text
AI Agent / Application
        │ MCP: discovery / validate / query / explain
        ▼
┌────────────────────────────────────────────────────┐
│ PostgreSQL Semantic Gateway (single process)       │
│ MCP Adapter → AuthN/AuthZ → Semantic Catalog       │
│ → LSQ Validator → Planner → SQL Compiler           │
│ → Guarded Executor → Lineage / Audit / Telemetry   │
└───────────────────────┬────────────────────────────┘
                        │ PostgreSQL protocol
                        ▼
┌────────────────────────────────────────────────────┐
│ PostgreSQL                                         │
│ business schemas │ semantic schema │ pg_catalog    │
│ COMMENT / FK / CHECK / GRANT / RLS                 │
└────────────────────────────────────────────────────┘
```

The MVP is limited to PostgreSQL only, analytical read-only queries, a single Gateway, a single database, and explicitly authenticated users. Multi-data-source support, natural-language response UIs, caching, pre-aggregation, pgvector, free-form SQL, and write operations are excluded from the MVP.

The post-beta direction keeps PostgreSQL as the only target database while
expanding the governed contract beyond extraction. M6 is version `0.4`, not
`1.0`, and introduces a separate typed mutation contract for controlled data
ingestion plus mandatory Linux amd64/arm64 runtime evidence. After `0.4`, each
minor release re-evaluates current reference implementations such as Wren AI,
Cube, Malloy, and MetricFlow, selects evidence-backed gaps that fit the
PostgreSQL-native position, and advances through several compatibility stages
before `1.0`.

## 2. Goals and Success Criteria

### 2.1 Goals

1. Provide an explicit, migratable Semantic Schema within PostgreSQL that can hold concepts, dimensions, metrics, relationships, synonyms, policy references, and lineage.
2. Safely ingest useful structural and semantic candidates from `pg_catalog`, `COMMENT`, PK/UNIQUE/FK, `CHECK`, GRANT, and RLS.
3. Compile LSQ v1 into identical normalized SQL and parameter lists given the same input, the same Semantic Revision, and the same Compiler Version.
4. Prevent LLMs from directly manipulating physical schemas or free-form SQL, providing discovery, validation, execution, and explanation through MCP instead.
5. Make every query result traceable down to the Semantic Revision used, the metrics, source columns, policies, and the generated SQL hash.
6. Provide a development environment reproducible on a Mac Studio with Apple Container and a production/runtime path verified on Linux amd64 and arm64.
7. Add governed data ingestion without exposing raw SQL or arbitrary DML, while preserving PostgreSQL GRANT, RLS `WITH CHECK`, constraints, triggers, and transaction semantics as the final authority.
8. Use comparison with maintained reference implementations to prioritize missing capabilities without broadening into non-PostgreSQL dialect support.

### 2.2 MVP Success Criteria

- Ingest an existing PostgreSQL sample schema and generate candidate models.
- Compile 20 or more representative LSQs into correct parameterized SQL against a human-reviewed and published model.
- Detect key granularity issues, join fan-out, and ambiguous join paths, and explicitly reject them rather than producing incorrect results.
- In tenant-isolation tests with RLS enabled, never retrieve rows belonging to another tenant.
- Allow an MCP client to complete model discovery, validation, execution, and explanation, with no free-form SQL entry point.
- Audit every query for semantic revision, query hash, SQL hash, source lineage, policy context, execution time, and row count.
- Pass the CI matrix for PostgreSQL 16, 17, and 18.
- Enable a first-time developer to start the Apple Container environment within 30 minutes and run integration tests.

### 2.3 Direction from 0.4 to 1.0

- `0.4` adds a versioned Logical Semantic Mutation (LSM) contract for bounded
  `insert` and explicitly modeled idempotent `upsert` operations. It does not
  accept table names, column names, conflict SQL, predicates, expressions, or
  stored procedure names from callers.
- Writable models and fields are published explicitly and independently from
  query visibility. Server-managed columns, generated columns, immutable
  fields, allowed conflict keys, batch size, and returned fields are part of
  the published revision.
- Read and write credentials, mapped roles, transaction modes, audit records,
  rate limits, and MCP capabilities are separate. Enabling mutation must not
  make the read-only query path writable.
- PostgreSQL permissions, RLS including `WITH CHECK`, constraints, and triggers
  remain authoritative. The Gateway may narrow permitted mutations but cannot
  make a database-denied mutation succeed.
- Linux amd64 and arm64 are release-blocking runtime targets. Cross-compilation
  or a multi-architecture manifest alone is not sufficient evidence; the
  binary or image must start and pass architecture-specific smoke and contract
  tests.
- Versions `0.5` through `0.9` are comparison-driven compatibility stages.
  Features are selected from measured user needs and reference-implementation
  gaps, then accepted only when they preserve the PostgreSQL-only and no-raw-SQL
  boundaries.

### 2.4 Non-Goals

- Building a general-purpose Text-to-SQL product, chat UI, visualization tool, or BI product.
- Supporting warehouses or federated queries outside PostgreSQL.
- Replacing dbt, ETL/ELT, data catalogs, or MDM.
- Exposing arbitrary SQL, DDL, DML, or stored-procedure execution through MCP.
- Treating the governed mutation contract as a replacement for bulk ETL/ELT,
  replication, CDC, or database administration.
- Replacing PostgreSQL's GRANT/RLS with Gateway-proprietary authorization.
- Implementing caching, pre-aggregation, vector search, or learned join inference in the MVP.
- Automatically correcting complex many-to-many or non-additive metrics when their semantics are unclear.
- Supporting non-PostgreSQL execution engines before or at `1.0`.

## 3. Design Principles

1. **PostgreSQL-native**: Store authoritative Semantic Metadata within the target PostgreSQL instance, managed under the same backup, transaction, permission, and migration procedures.
2. **Database security is authoritative**: The Gateway must not weaken GRANT/RLS. Execution roles must not be owners, superusers, or `BYPASSRLS`.
3. **LLM proposes; deterministic code decides**: The LLM only constructs LSQ candidates; resolution, authorization, planning, SQL generation, and limit enforcement are handled by deterministic code.
4. **Fail closed**: Unknown fields, ambiguous relationships, unapproved metrics, type mismatches, and excessive costs are rejected rather than guessed.
5. **Semantic contract is versioned**: Version the Semantic Model and LSQ Schema, pinning to a published revision at execution time.
6. **No raw SQL in the public contract**: Expressions in the public API are typed ASTs or approved symbol references; arbitrary SQL strings are not accepted.
7. **Lineage by construction**: Build lineage edges during name resolution and planning rather than analyzing SQL after the fact.
8. **Small modular monolith first**: Begin the MVP with a single binary and single database, separating only the compiler's pure library boundary upfront.
9. **Explicit beats inferred**: Inferences drawn from the catalog are candidates only; publishing requires human approval.
10. **Correctness before coverage**: Limit the supported scope to guarantee correct answers or safe rejections, rather than expanding ambiguous automatic handling.

## 4. Scope and Key Use Cases

### 4.1 MVP Use Cases

- A data engineer scans an existing database and reviews candidate physical tables, columns, comments, constraints, relationships, and RLS policies.
- A data owner registers concept names, descriptions, published columns, metrics, permitted joins, and granularity, then publishes a revision.
- An AI agent discovers available models and fields via MCP.
- An AI agent validates an LSQ and executes a read-only query.
- An auditor reviews which definitions, columns, relationships, and policies a result depended on.
- CI verifies Semantic Model migrations, golden SQL, known answers, RLS boundaries, and backward compatibility.

### 4.2 Semantic Constraints for the MVP

- Each query anchors on exactly one fact model.
- Joins from a fact to dimensions allow only many-to-one or one-to-one in principle.
- Many-to-many, bridge, simultaneous multi-fact aggregation, and symmetric aggregates are rejected in v1.
- Metrics start with `count`, `count_distinct`, `sum`, `min`, `max`, `avg`, and safe arithmetic compositions thereof.
- Window functions, arbitrary subqueries, and user-defined functions are out of scope for the MVP.
- Dimension filters and metric post-aggregate filters (the equivalent of HAVING) are distinguished.
- Time granularity levels are `day`, `week`, `month`, `quarter`, and `year`. Timezone is chosen from values permitted in the model or request.

## 5. PostgreSQL-native Semantic Schema

### 5.1 Placement and Ownership

- The dedicated schema name defaults to `semantic` and is configurable.
- The schema owner is the migration-only role `postgresem_owner`.
- The Gateway connection role `postgresem_runtime` defaults to `LOGIN NOINHERIT` and holds no direct business-data privileges. It switches to a permitted `NOLOGIN` execution role only within a transaction.
- Catalog scanning is separated to the administrative role `postgresem_introspector`, and audit writes to `postgresem_auditor`. Connections for normal queries are not made read-write.
- Model editing is separated to `postgresem_editor` and publishing to `postgresem_publisher`.
- Business table owners and Gateway execution roles must never be the same.
- RLS is also applied to metadata tables so that physical objects and descriptions invisible to a given user are not exposed through the catalog API.

### 5.2 Core Table Design

| Table | Purpose | MVP |
|---|---|---:|
| `semantic.project` | Semantic Project within the target database | Required |
| `semantic.revision` | draft/published/retired status, parent revision, canonical hash | Required |
| `semantic.model` | Business model, anchor relation, grain, publication status | Required |
| `semantic.field` | dimension/entity key/time dimension, type, physical column reference | Required |
| `semantic.relationship` | Join cardinality, column mapping, permitted direction, priority | Required |
| `semantic.metric` | Aggregation, typed expression AST, filter, additivity | Required |
| `semantic.term` | Display name, synonyms, description, locale | Required |
| `semantic.policy_binding` | References to DB role/RLS/semantic visibility; not a copy of RLS expressions | Required |
| `semantic.source_snapshot` | Physical object fingerprint at catalog scan time | Required |
| `semantic.import_run` / `import_issue` | Import history, candidates, warnings, drift | Required |
| `semantic.lineage_edge` | Design-time lineage between model/field/metric/source | Required |
| `semantic.query_audit` | Runtime lineage, hash, elapsed time, result size | Required |
| `semantic.example_query` | Approved LSQs and known-result conditions | Phase 2 |
| `semantic.embedding` | Embeddings for terms/examples | After pgvector adoption |

### 5.3 Model Identification and References

- The public API uses a stable `semantic_name` unique within a revision and a UUID, rather than sequential IDs.
- Physical references use the canonical name `database/schema/relation/column` as the source of truth; OIDs are limited to auxiliary values at scan time. OIDs are not used as persistent IDs because they can change across dump/restore or recreation.
- Identifiers are quoted appropriately using only values the Gateway obtained from the catalog. Strings received from requests are never inserted directly into SQL identifiers.
- Draft updates are treated as new immutable revision creations; published revisions are never overwritten.
- `canonical_hash` is computed from the order-normalized model JSON, schema version, and compiler semantic version.

### 5.4 Expression AST

`metric.expression` and computed fields are stored as versioned JSONB ASTs. SQL fragments are not stored.

```json
{
  "version": "1",
  "op": "aggregate",
  "function": "sum",
  "arg": { "op": "field_ref", "field": "order_amount" },
  "filter": {
    "op": "eq",
    "left": { "op": "field_ref", "field": "status" },
    "right": { "op": "literal", "type": "text", "value": "paid" }
  }
}
```

- The JSONB shape is minimally validated with database `CHECK` constraints; full JSON Schema and type validation is performed by the Gateway and CI.
- Permitted operators and functions are allowlisted.
- Function volatility, input/output types, and NULL semantics are fixed in the compiler-side registry.
- New AST versions are introduced explicitly through migrations and compiler capabilities; unknown versions are rejected.

### 5.5 Semantic Schema Integrity

- Name uniqueness, referential integrity, and state transitions within the same revision are enforced by PK/UNIQUE/FK/CHECK.
- Transition to `published` is only permitted when all validations pass within a Gateway/CLI transaction.
- `relationship.cardinality`, `metric.aggregation`, and `revision.status` use CHECK constraints equivalent to enums. PostgreSQL enum types are avoided in the MVP because they are difficult to alter.
- Avoid EAV designs that pack critical semantics into arbitrary JSONB. JSONB is limited to versioned ASTs, compatible auxiliary attributes, and audit payloads.

## 6. Ingestion of `pg_catalog` / `COMMENT` / FK / CHECK / RLS

### 6.1 Ingestion Pipeline

```text
catalog scan (read-only, repeatable read)
  → normalize physical objects
  → fingerprint
  → infer candidates + confidence + evidence
  → compare with current published revision
  → import report / drift report
  → human review
  → new draft revision
  → validate and publish
```

Scans are idempotent and never automatically rewrite published models. They are invoked explicitly via `postgresem catalog scan` and the admin API using dedicated introspector credentials; the MVP does not require event triggers. The regular Gateway runtime is not granted scan privileges.

### 6.2 Primary Catalog Sources

- relation/schema: `pg_class`, `pg_namespace`
- column/type/default: `pg_attribute`, `pg_type`, `pg_attrdef`
- key/constraint: `pg_constraint`, `pg_index`
- comments: `pg_description`, `obj_description`, `col_description`
- functions used by views/expressions: `pg_proc`, `pg_get_functiondef` (minimal use)
- views: do not interpret `pg_rewrite` directly; use `pg_get_viewdef`
- privilege: `has_schema_privilege`, `has_table_privilege`, `has_column_privilege`, etc.
- RLS: `pg_policy`, `pg_class.relrowsecurity`, `relforcerowsecurity`, `pg_get_expr`
- dependency/lineage auxiliary: `pg_depend`, `pg_rewrite`

No direct updates are made to the catalog. Fixtures are maintained for each supported PostgreSQL version, and dependency diffs on published catalog columns are detected in CI.

### 6.3 Ingestion Rules

| Input | Generated Candidate | Handling |
|---|---|---|
| table/view name | model name, term | Normalized by naming convention; always requires review |
| table/column `COMMENT` | description, business term | Plain text; not interpreted as a structured DSL |
| PK/UNIQUE | entity key, grain candidate | High confidence; however, business granularity requires approval |
| FK | relationship, join columns, cardinality candidate | High confidence; composite FKs are handled with preserved column order |
| NOT NULL | nullability information | Ingested as-is |
| CHECK | domain/value range/enum candidate | Only forms that the parser can safely understand are converted to hints |
| view definition | source lineage candidate | Uses the PostgreSQL parser/dependency information; does not analyze SQL via string regex |
| GRANT | model/field visibility candidate | Runtime DB privilege checks are authoritative |
| RLS policy | policy presence, target role, command, mode | For discovery/explanation only; not translated into a Gateway-proprietary expression |

`COMMENT` is visible to the connected user and is not a suitable storage location for sensitive information. Accordingly, the Gateway does not indiscriminately copy comments into audit logs; only descriptions explicitly approved for publication in the Semantic Catalog are returned via MCP.

### 6.4 CHECK Handling

- Only forms that the parser can safely convert into a typed AST are promoted to candidates, such as `col IN (...)`, range, simple comparison, and NULL conditions.
- CHECKs that depend on other rows or external functions are not trusted as semantic definitions.
- CHECKs are treated as "allowed-value hints" and are not promoted to metric definitions or authorization conditions.
- Expressions not supported by the parser are not copied as raw SQL into the semantic schema; only a hash and a human-readable warning are recorded.

### 6.5 RLS and Principal Propagation

- A static mapping is performed from the Gateway's authenticated principal to a pre-registered `NOLOGIN` DB role or an approved session context. Role names or GUC values specified in the request are not accepted.
- After obtaining a connection from the pool, a transaction is started, `SET LOCAL ROLE <mapped_role>` and any required `set_config(..., true)` are applied, and then the query is executed. Roles are quoted from a catalog-derived allowlist; arbitrary strings are never concatenated.
- `postgresem_runtime` is granted membership only to the necessary mapped roles, with `NOINHERIT` and `SET ROLE` capability. If the number of principals exceeds operational role limits, ADR-005 selects a fixed-role plus RLS session-context approach.
- The execution role must not be a table owner, superuser, or `BYPASSRLS`. Enabling `FORCE ROW LEVEL SECURITY` on necessary tables is recommended.
- Integration tests verify that the context is reliably destroyed when the transaction ends.
- The compiler does not re-implement RLS expressions. RLS is enforced at the database level; the Gateway additionally reduces model/field visibility and query capabilities.
- Security review items include referential integrity bypassing RLS and information leakage caused by conflicting RLS subqueries.

## 7. Semantic Gateway Architecture

The MVP is a modularized monolith without network distribution.

| Module | Responsibility |
|---|---|
| MCP transport | stdio; later streamable HTTP. Protocol framing and pagination |
| Identity | Token verification, principal-to-DB-role mapping, request context |
| Catalog | Loading/caching published revisions, discovery API filtered by permissions |
| Importer | Catalog scan, candidate generation, drift detection |
| LSQ validator | JSON Schema, semantic name/type/capability validation |
| Planner | Anchor, join graph, grain, aggregate, policy binding, cost guard |
| Compiler | Generates PostgreSQL AST/SQL and bind parameters from a typed relational IR |
| Executor | Read-only transaction, timeout, row/byte limit, cancel |
| Mutation compiler/executor | M6: typed insert/upsert plan, separate writer role, idempotency, rollback |
| Lineage/Audit | Design-time/query-time/mutation-time edges, hashes, audit events |
| Telemetry | Structured logs, metrics, traces, health/readiness |
| Admin CLI | migrate, scan, validate, publish, diff, doctor |

### 7.1 Technology Stack Policy

- The primary candidate is a single Rust workspace. The rationale is that typed IR, a deterministic compiler, low distribution dependencies, a single binary, and async PostgreSQL connections can be implemented coherently.
- Web/MCP transport, PostgreSQL driver, JSON Schema validator, and SQL parser/AST renderer are evaluated via ADR before adoption for license, maintenance, PostgreSQL 16–18 support, and fuzz track record.
- The SQL parser is not used to permit input SQL; it is used for post-generation syntax re-parse, view lineage assistance, and golden tests.
- To avoid over-reliance on MCP SDK maturity, the internal application service is separated from the MCP adapter.
- Splitting into separate services is deferred until the need is measured in terms of load, independent release, or privilege boundaries.

### 7.2 Configuration

- Configuration precedence: CLI flag > environment > TOML file > default.
- Secrets are sourced from environment variables or an external secret store; they are never stored in config files, the Semantic Schema, COMMENTs, or logs.
- Principal mapping, statement timeout, result limits, permitted revisions, and audit retention period are configurable.
- At startup, the database version, migration version, required privileges, and RLS safety conditions are verified via a `doctor`-equivalent check; the process fails if a dangerous execution role is detected.

### 7.3 Governed Mutation Boundary (M6 Target)

- Query and mutation planning share immutable semantic snapshots and typed
  scalar semantics, but use distinct request types, compiler entry points,
  credentials, roles, budgets, audit records, and executors.
- The current query executor remains transaction-level `READ ONLY`. Mutation
  support cannot be implemented by weakening or parameterizing that invariant.
- A dedicated login assumes only explicitly allowed non-owner, non-superuser,
  non-`BYPASSRLS` writer roles. The request cannot select a connection, role,
  project, conflict policy, or transaction isolation level.
- The M6 compiler emits one parameterized `INSERT` or approved
  `INSERT ... ON CONFLICT` statement against a published writable model.
  Arbitrary `UPDATE`, `DELETE`, `MERGE`, `COPY`, `CALL`, DDL, expressions, and
  multi-statement input remain rejected in `0.4`.
- Idempotency keys, maximum rows and bytes, statement/lock timeouts, atomic
  audit start/finish, and explicit affected-row expectations are mandatory.
- The compiler crate remains free of database, transport, logging, and audit
  I/O. Database-enforced rejection is surfaced as a stable mutation error, not
  converted into a success-shaped response.

## 8. MCP API

### 8.1 API Principles

- MCP operates on semantic objects. No raw SQL tool is provided.
- Discovery results are filtered to only the published models available to the authenticated principal.
- Tool inputs/outputs are defined with versioned JSON Schemas, with `additionalProperties: false` as the default.
- Large catalogs/results use cursor pagination, with upper bounds on row count, bytes, and execution time.
- Validation errors return machine-readable codes, JSON Pointers, and candidate names, but do not reveal the existence of non-public objects.

For the MVP stdio transport, the principal and scope are fixed from the Gateway's startup configuration; self-declared values in MCP requests are not trusted. For the later HTTP transport, OIDC/JWT is verified by a reverse proxy or the Gateway itself, establishing the principal per request. Both stdio and HTTP share the same internal authorization context.

### 8.2 MVP Tools

| Tool | Purpose | Execution |
|---|---|---:|
| `list_semantic_models` | Paginated retrieval of available models | None |
| `describe_semantic_model` | Retrieve fields, metrics, relationships, grain, and constraints | None |
| `validate_semantic_query` | Pre-validate LSQ for schema/semantic/security/cost | None |
| `query_semantic_model` | validate → compile → execute → result with lineage | Yes |
| `explain_semantic_query` | Explain normalized LSQ, join plan, source lineage, and constraints | Typically none |

Administrative scan/publish operations are not exposed to general MCP consumers; they are limited to the CLI or a separate administrative scope. The `compile` feature that returns full physical SQL is also disabled in the MVP's general scope and provided only under a developer debug scope.

### 8.3 Resources

- `semantic://projects/{project}/revisions/current`
- `semantic://projects/{project}/models/{model}`
- `semantic://schemas/lsq/v1`

Resources pass through the same authorization filter as tools. Prompt templates and natural-language response generation are not part of the core scope.

### 8.4 Query Response

```json
{
  "schema_version": "1",
  "query_id": "uuid",
  "semantic_revision": "sha256:...",
  "columns": [{"name": "month", "type": "date"}, {"name": "revenue", "type": "numeric"}],
  "rows": [["2026-08-01", "12345.67"]],
  "truncated": false,
  "lineage": {
    "models": ["orders"],
    "metrics": ["revenue"],
    "source_columns": ["sales.orders.amount"]
  },
  "warnings": []
}
```

numeric, timestamp, date, interval, and similar types follow documented precision and timezone conventions for JSON. In the MVP, numeric is returned as a string, timestamp as RFC 3339, and date as ISO 8601.

### 8.5 Post-MVP Mutation API

M6 adds mutation only through a separately versioned capability. Candidate CLI
and MCP operations are `validate_semantic_mutation` and
`mutate_semantic_model`; final names and schemas require an ADR. A process that
has no configured writer profile does not advertise or accept mutation tools.
Mutation requests use published semantic model and field names only, and never
return generated SQL or accept physical identifiers. Request-supplied
principals, roles, projects, credentials, idempotency storage, or conflict
expressions are rejected.

The original five read-only MCP tools retain their existing meaning. Mutation
capability negotiation, audit taxonomy, response shape, replay semantics, and
compatibility rules are versioned independently so a read-only deployment
cannot become writable through a client request.

## 9. Logical Semantic Query JSON Schema Policy

### 9.1 LSQ v1 Shape

```json
{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "schema_version": "1",
  "model": "orders",
  "dimensions": [
    {"field": "ordered_at", "time_grain": "month"},
    {"field": "customer_region"}
  ],
  "metrics": [{"metric": "revenue"}],
  "filters": {
    "op": "and",
    "args": [
      {"op": "gte", "field": "ordered_at", "value": {"type": "date", "value": "2026-01-01"}},
      {"op": "in", "field": "status", "values": [{"type": "text", "value": "paid"}]}
    ]
  },
  "order_by": [{"ref": "revenue", "direction": "desc"}],
  "limit": 100
}
```

### 9.2 Schema Design Conventions

- JSON Schema draft 2020-12 is adopted; `schema_version` is required.
- The top level and each node use `additionalProperties: false`.
- Free-form SQL, table names, column names, expression strings, and function names are prohibited; only published semantic symbols may be referenced.
- Literals are typed and validated against the compiler's type system before being converted to bind parameters.
- Filters have upper bounds on depth, node count, and `in` element count.
- `limit` is either required or has a server default, and cannot exceed a hard maximum.
- Dimension and metric output aliases are determined by the server; arbitrary identifiers are not accepted.
- Join paths are chosen uniquely from declared relationships by the planner. If multiple candidates have equal priority, an error is raised.
- JSON Schema handles syntactic validation only. Reference existence, types, permissions, grain, aggregate compatibility, and cost are handled by semantic validation.

### 9.3 Compatibility

- patch: Changes to error messages or descriptions. Input semantics are unchanged.
- minor: Addition of optional fields/operators. Existing queries retain the same meaning.
- major: When semantics change, a new `schema_version` is added with a parallel support period.
- For each golden LSQ, the normalized IR, SQL, parameters, and lineage are pinned, and semantic diffs are reviewed on compiler updates.

## 10. Deterministic SQL Compiler

### 10.1 Pipeline

```text
LSQ JSON
  1. JSON Schema validation
  2. principal-filtered symbol resolution
  3. type / grain / aggregation validation
  4. relationship graph planning
  5. typed relational IR generation
  6. policy and resource guard annotation
  7. PostgreSQL AST generation
  8. stable SQL rendering + bind parameter ordering
  9. generated SQL re-parse / structural assertion
 10. optional EXPLAIN cost guard
 11. read-only execution
```

### 10.2 Definition of Determinism

Given the same inputs below, the normalized IR, SQL text, parameter type/order, lineage, and query hash are guaranteed to be identical:

- LSQ semantics (object key ordering is ignored)
- Published Semantic Revision
- Compiler semantic version
- Capability/config profile
- Principal's permission set and policy context

Stable alias, join order, projection order, parameter numbering, and predicate normalization rules are specified. The PostgreSQL planner's execution plan itself may vary with statistics and version and is therefore excluded from the determinism guarantee.

### 10.3 SQL Generation Safety Requirements

- All values use bind parameters. Literals are never produced via string concatenation.
- Identifiers are quoted using verified values from the catalog.
- Only a single `SELECT` or read-only CTE is generated.
- Multiple statements via semicolons, DDL/DML, COPY, CALL, volatile functions, and unapproved UDFs are never generated.
- `SET TRANSACTION READ ONLY`, `statement_timeout`, `lock_timeout`, and `idle_in_transaction_session_timeout` are set transaction-locally.
- In addition to a hard row limit, result byte limits and cancel are implemented.
- An optional `EXPLAIN (FORMAT JSON)` inspects estimated cost/rows, but EXPLAIN results are not treated as proof of correctness.

### 10.4 Join and Aggregation Correctness

- Relationships explicitly declare `one_to_one`, `many_to_one`, `one_to_many`, `many_to_many`, and join keys.
- The MVP compiler auto-selects only fact-to-many-to-one/one-to-one paths.
- Requests for dimensions on the one-to-many side or multiple facts are rejected as fan-out risks.
- Additivity (across time, entity, all dimensions) is maintained per metric; aggregation along non-additive axes is rejected or warned.
- `count_distinct` is permitted only on explicitly declared entity keys.
- NULL join semantics, timezone, week start, and currency/unit are included in the model contract.

### 10.5 Compiler API Boundary

The compiler core is kept as close to a pure function with no I/O as possible.

```text
compile(
  normalized_lsq,
  immutable_semantic_snapshot,
  principal_capabilities,
  compiler_options
) -> { sql, typed_parameters, output_schema, lineage, warnings, hash }
```

No database connections, MCP, logging, or execution reside in the core. This boundary is the focal point for property tests, fuzz tests, and golden tests.

### 10.6 Deterministic Mutation Compiler (M6 Target)

```text
compile_mutation(
  normalized_lsm,
  immutable_semantic_snapshot,
  principal_capabilities,
  mutation_options
) -> { statement, typed_parameters, affected_model, write_lineage, hash }
```

The M6 mutation compiler supports bounded inserts and explicitly modeled
idempotent upserts only. It validates writable visibility, required/defaulted
fields, PostgreSQL scalar types, nullability, generated/identity columns,
immutable fields, allowed conflict keys, batch limits, and return visibility.
The same input, revision, compiler semantic version, and capability profile
must produce identical statement text, parameter ordering, lineage, and hash.
Unknown or ambiguous fields, client-selected physical names, partial conflict
keys, unsafe defaults, and unsupported expressions fail closed.

## 11. Security

### 11.1 Threat Model

- Prompt injection attempting to discover non-public models, execute raw SQL, or bypass limits.
- SQL injection or identifier injection.
- Connection role or pool context leaks that bypass RLS/GRANT.
- Mutation attempts that bypass writable-field policy, RLS `WITH CHECK`,
  constraints, idempotency, or affected-row expectations.
- DoS via high-cost queries, massive `IN` lists, Cartesian joins, or enormous result sets.
- Sensitive information leakage through catalog/comment/error/log channels.
- Malicious modification of the Semantic Model or supply-chain contamination.
- Inability to audit due to stale revisions or compiler diffs.

### 11.2 Defenses

- In stdio mode, the principal/scope is established from the launch context and fixed configuration. In HTTP mode, the Gateway or reverse proxy verifies OIDC/JWT signatures, issuers, audiences, expiry, and scopes. In neither mode is a self-declared principal in the request body trusted, and anonymous remote access is never permitted.
- Least-privilege separated roles, enforced RLS, and principal mapping allowlists.
- Multi-layered defense: LSQ schema, semantic validation, typed IR, and parameterized SQL.
- Visibility and capabilities at the model/field/metric level. DB permissions serve as the last line of defense.
- Budgets for query complexity, join count, filter nodes, time range, limit, timeout, concurrency, and result bytes.
- Errors are separated into public codes and internal details; non-public object names and SQL are never returned to the general scope.
- Logs do not record actual query/result values by default; instead, hashes, types, counts, and timings are recorded.
- Source query read-only pool, introspection, and audit writer are separated by credential and pool. The audit writer is permitted only to append/update `semantic.query_audit`.
- Mutation uses a dedicated writer credential and a separate allowlisted mapped
  role. Read-only credentials retain no business-data write privilege, and
  writer credentials receive only the modeled table/column operations required
  by the published mutation contract.
- Migration/model publishing goes through signed releases or review-required CI.
- Dependency audit, SBOM, container image scanning, and secret scanning are release gates.

### 11.3 Required Security Tests

- With tenant A's principal, rows from tenant B return zero results or a permission error.
- After pool reuse, no `SET LOCAL ROLE`/GUC from the previous request persists.
- Connections as table owner, superuser, or `BYPASSRLS` are rejected at startup.
- Guessing hidden model/field names does not reveal whether they exist.
- Malicious literals, Unicode identifiers, deeply nested filters, massive IN lists, NaN/Infinity, and timezone edge cases are rejected or handled safely.
- After cancel/timeout, the transaction is aborted/rolled back, and the connection is returned to the pool in a safe state.
- The stdio fixed principal and the HTTP request principal produce the same visibility under the same authorization fixtures.
- The source execution role cannot write to the Semantic Schema or audit tables, and the audit writer cannot read business data.
- Read-only deployments do not advertise mutation capability and cannot be made
  writable through request fields.
- Mutation tests cover cross-tenant inserts, RLS `WITH CHECK`, generated and
  immutable fields, duplicate idempotency keys, partial batches, constraint and
  trigger failures, timeout/cancel rollback, affected-row mismatch, and audit
  failure. No denied mutation may be reported as successful.

## 12. Semantic Lineage and Audit

### 12.1 Three Types of Lineage

1. **Design-time lineage**: metric → field → physical column, model → view/table, relationship → join columns.
2. **Query-time lineage**: query → revision → metrics/dimensions → relationships → physical objects → policy context → SQL hash.
3. **Mutation-time lineage**: mutation → revision → writable model/fields →
   physical target columns → policy context → statement hash → affected-row
   outcome.

### 12.2 Recorded Items

- `query_id`, request/correlation ID, timestamp
- Irreversible ID or audit subject ID of the principal. Tokens and secrets are not stored
- LSQ schema version, canonical LSQ hash
- Semantic revision/hash, compiler version, config profile
- Resolved model/field/metric/relationship IDs and definition hashes
- Source relation/column, policy binding ID, DB role ID
- Generated SQL hash. Storing the SQL text itself is explicit opt-in under a debug/audit scope
- Parameter types and count. Values are not stored by default
- Elapsed time for each stage: validation/compile/queue/DB/result serialization
- Status, error code, row count, byte count, truncated/cancelled

A `started` event is appended via a dedicated audit connection before execution; if recording fails, the query is not started. Upon completion, a terminal event/status is written to the same `query_id`. `started` records without a terminal event (e.g., due to process crash) are flagged for monitoring. This ensures that the read-only source transaction is not compromised and eliminates the possibility of a query being executed without an audit record.

Mutation audit uses a separate record type and identifier. It records typed
field names, row count, payload byte count, idempotency-key hash, statement
hash, policy context, and terminal outcome, but not field values by default.
The audit start must be durable before the source transaction and a terminal
status must distinguish committed, rejected, rolled back, indeterminate, and
reconciled outcomes.

### 12.3 Drift

- The physical schema fingerprint is compared against the source snapshot of the published revision.
- Column drops/type changes, constraint/FK/RLS changes are surfaced as issues with severity levels.
- A configuration option is provided to fail-closed on new queries for models with breaking drift.
- Immediate detection via event triggers or logical decoding is deferred due to the required privileges and operational overhead. Explicit scans and periodic CI are sufficient for the MVP.

## 13. pgvector Is Deferred

pgvector is not an MVP dependency. Discovery accuracy using semantic names and explicit descriptions/synonyms is measured first.

Adoption candidates are limited to:

- Similarity search for business terms, descriptions, and approved example queries
- Ranking candidates returned to MCP in large catalogs
- Detection of term duplication and semantic drift candidates

Embeddings are not used for query correctness, permissions, join selection, or SQL generation. Embedding model/version, source text hash, and locale are stored to ensure reproducibility. The adoption decision requires measured evidence that discovery recall clearly improves over a non-vector baseline and that the improvement outweighs operational cost and leakage risk.

## 14. Container Development Environment

### 14.1 Local Prerequisites

The maintainer reference command on the Mac Studio is
`container-compose`. The macOS quickstart continues to use Apple Container,
while Linux documentation and CI use a supported OCI/Compose runtime. The
portable contract is the repository configuration and released artifacts, not
one host runtime. Bootstrap must work from an unstarted state on each documented
development path.

### 14.2 Compose Services

| Service | Description | Always |
|---|---|---:|
| `postgres` | PostgreSQL 18 default, named volume, healthcheck, fixtures | Required |
| `gateway` | postgresem binary, read-only source mount or build image | Required |
| `migrate` | One-shot migration | Required |
| `test` | Unit/integration/contract test runner | Profile |
| `otel-collector` | Local trace/metric inspection | Observability profile |
| `prometheus` / `grafana` | Dashboard development | Observability profile; deferred |

The Compose file is limited to the intersection of features that work with both Apple Container and Linux CI. It does not depend on special Docker sockets, privileged containers, or implicit host networking. PostgreSQL data defaults to a named volume rather than a bind mount to avoid permission discrepancies.

### 14.3 Development Command Goals

```text
make doctor       # Verify Apple Container, container-compose, versions, ports
make dev-up       # Start PostgreSQL and Gateway after migration
make test         # unit + fast integration
make test-all     # PG 16–18, security, golden, migration
make dev-down     # Stop containers; volumes are preserved
make clean-data   # Delete development volumes only, with explicit confirmation
```

`.env.example` contains only non-secret values. Fixtures use fictional data exclusively; real data dumps are never committed to the repository.

### 14.4 PostgreSQL Support

- Initial support: PostgreSQL 16, 17, 18. The development default is 18.
- Core features must work without extensions.
- pgvector, pg_stat_statements, etc. are detected as optional capabilities.
- Version-specific catalog differences are absorbed by adapters, and the support matrix is documented.

### 14.5 Operating-System and Architecture Support

- Required by M6: released binaries and OCI images execute on Linux amd64 and
  Linux arm64.
- Maintained development/archive targets: macOS amd64 and macOS arm64.
- CI must execute the binary or image on both Linux architectures. Building a
  manifest without starting it does not satisfy the support gate.
- At least the CLI contract, TLS initialization, migration compatibility,
  catalog loading, guarded query execution, governed mutation rejection/smoke,
  and installer verification run on both Linux architectures.
- PostgreSQL 16–18 behavior remains a separate matrix from CPU architecture;
  the release gate documents any reduced cross-product and why it is safe.
- Architecture-specific native dependencies, OpenSSL/TLS behavior, endianness,
  filesystem assumptions, and archive naming are covered by release tests.

## 15. Repository Structure

```text
postgresem/
├── Cargo.toml
├── Cargo.lock
├── README.md
├── LICENSE
├── SECURITY.md
├── CONTRIBUTING.md
├── CODE_OF_CONDUCT.md
├── AGENTS.md
├── Makefile
├── compose.yaml
├── Containerfile
├── crates/
│   ├── postgresem/              # gateway binary, CLI, wiring
│   └── postgresem-compiler/     # pure LSQ/IR/planner/compiler core
├── migrations/                  # semantic schema, forward-only
├── schemas/
│   ├── lsq/v1.schema.json
│   ├── mcp/
│   └── semantic-expression/
├── fixtures/
│   ├── commerce/
│   ├── rls-multitenant/
│   └── catalog-compat/
├── tests/
│   ├── golden/
│   ├── integration/
│   ├── security/
│   ├── compatibility/
│   └── evals/
├── docs/
│   ├── POSTGRESQL_SEMANTIC_GATEWAY_IMPLEMENTATION_PLAN.md
│   ├── POSTGRESQL_SEMANTIC_GATEWAY_IMPLEMENTATION_PLAN-jp.md
│   ├── architecture/
│   ├── adr/
│   ├── threat-model.md
│   ├── lsq-v1.md
│   └── operations/
└── .github/workflows/           # Adjusted for the chosen CI system
```

Avoid over-splitting crates in the MVP. Only the compiler core is separated; everything else remains as Rust modules within `postgresem`. Add crates when independent releases or clear dependency inversions become necessary.

## 16. Test Strategy

### 16.1 Test Pyramid

| Layer | Target | Example |
|---|---|---|
| Unit | AST, types, name resolution, normalization, budget | Same hash for semantically identical JSON key orders |
| Property/Fuzz | LSQ parser, filter AST, renderer | No panics, unknown nodes rejected, no literal leaks |
| Golden compiler | LSQ → IR/SQL/params/lineage | Reviewable diffs |
| Integration | Real PostgreSQL 16–18 | Catalog scan, migration, query results |
| Security | GRANT/RLS/pool/timeout | No cross-tenant leakage |
| Mutation security | writer role/RLS/idempotency/rollback | No unauthorized or partially reported write |
| Contract | MCP/JSON Schema | Compatibility with client fixtures |
| Migration | fresh + N-1 upgrade | Published revision preserved, rollback policy verified |
| Known-answer eval | Semantic correctness | Expected values/expected rejections for representative questions |
| Performance | compile/execute/catalog | Regression budget |
| Platform | Linux amd64/arm64 binaries and images | Install, start, query, mutation smoke, TLS initialization |

### 16.2 Correctness Oracle

- Each fixture maintains hand-written, trusted SQL and expected results.
- LSQ results are compared against the oracle; correctness is not judged by SQL string matching alone.
- Approximately the same number of "queries that should be rejected" are prepared as correct cases.
- Join fan-out, NULL, empty set, timezone, DST, numeric precision, duplicate key, and RLS are mandatory fixtures.

### 16.3 Quality Gates

- New operators in the compiler core require unit, golden, property, and integration tests.
- Security-critical modules require code-owner review.
- Flaky tests are not ignored; they are quarantined with an expiration date.
- Coverage percentage alone is not a gate; a coverage matrix of specification branches, threats, and known answers is maintained.

## 17. Observability

### 17.1 Structured Logs

- JSON format, UTC timestamp, severity, service version, request/query/mutation ID.
- Stage, error code, semantic revision, compiler version, query/statement hash, duration, row/byte/affected-row count.
- Literals, tokens, connection strings, result rows, and unpublished comments are not recorded by default.
- Debug SQL logging is explicitly enabled in local or limited scopes, with a short retention period.

### 17.2 Metrics

- Request count, validation/compile/execute error count, rate by error code
- Validation/compile/DB/serialization latency histogram
- Active/queued queries, pool utilization, timeout/cancel, result truncation
- Catalog scan time, object count, drift issue count, publish count
- Mutation validation/commit/reject/rollback count, idempotent replay count, and
  indeterminate/reconciliation count
- Per-model/metric usage counts avoid high cardinality; detailed analysis is done in the audit DB if needed

### 17.3 Traces

`mcp.request → auth → catalog.resolve → validate → plan → compile → db.acquire → db.query → serialize → audit` is captured as spans. M6 mutation traces use a distinct `validate → compile_mutation → db.mutate → commit/rollback → audit` path. OpenTelemetry export is optional, and the core remains vendor-neutral.

### 17.4 SLO Candidates (Finalized at Beta)

- Gateway-attributable validation + compile p95 < 50 ms (warm state at 100-model scale)
- Audit event loss: 0
- Mutation audit loss and success-shaped indeterminate outcomes: 0 after M6
- Security test pass: 100%
- Semantic correctness eval for supported LSQs: 100%; unsupported LSQs are explicitly rejected

DB execution time depends on data volume and indexes and is therefore separated from the Gateway SLO.

## 18. CI/CD and Release

### 18.1 Pull Request CI

1. Format, lint, license header, JSON Schema lint
2. Unit, property tests, compiler golden diff
3. PostgreSQL 16–18 integration matrix
4. RLS/security, MCP contract, migration tests
5. Dependency/secret/license scan
6. Container image build and smoke test
7. Documentation link check, migration checksum verification

### 18.2 Release Pipeline

- SemVer is adopted; compatibility notes are generated for the LSQ schema, Semantic Schema migration, MCP contract, and compiler semantics.
- Tags produce reproducible binaries and multi-arch OCI images (at minimum `linux/amd64` and `linux/arm64`).
- Release CI executes architecture-specific smoke and contract tests for both
  Linux amd64 and arm64; successful cross-build alone is insufficient.
- SBOM, provenance, checksums, and signatures are attached to release artifacts.
- Migrations are forward-only by default; pre-release backup, N-1 upgrade tests, and a compatibility period are defined.
- Database migration and binary rollout ordering follows an expand/contract approach.
- Release candidates are smoke-tested on the Mac Studio with Apple Container to catch compatibility differences not visible in CI alone.

### 18.3 Branch/Release Policy

- `main` is always releasable.
- Major semantic changes are accompanied by an ADR, threat model diff, and golden diff.
- Before 1.0, minor versions may include breaking changes, but a migration guide is required.
- After 1.0, parallel support periods are established for each LSQ major version.

## 19. Milestones

Timelines are not committed before staffing is confirmed. On 2026-09-01, the
project owner authorized M6 implementation based on the demonstrated beta
value. Outstanding M5 independent field/security evidence remains tracked and
is not retroactively marked complete; unresolved P0/P1 findings still block a
`0.4` release.

### M0: Project Foundation / RFC

- Repository infrastructure, license, governance, ADR template, threat model
- Apple Container / `container-compose` bootstrap, PG 16–18 matrix
- LSQ v1, Semantic Schema v1, compiler semantics RFC
- Comparative evaluation against Wren AI / Cube / Malloy / MetricFlow

**Exit gate**: Using 3 representative datasets and a 30-question eval, approve the PostgreSQL-native value hypothesis and MVP boundary.

### M1: Semantic Catalog Alpha

- Migration, revision, model, field, relationship, metric, term
- `pg_catalog` / COMMENT / constraint / RLS scan and drift report
- CLI: `migrate`, `doctor`, `catalog scan`, `model validate`, `model publish`

**Exit gate**: Scans are idempotent, published revisions are immutable, and normalized snapshots are equivalent across PG 16–18.

### M2: LSQ Compiler Alpha

- LSQ JSON Schema, typed IR, symbol/type/grain validation
- Single-fact + many-to-one join, basic aggregate/filter/time grain
- Parameterized PostgreSQL SQL, golden/property/fuzz tests
- Design-time / query-time lineage generation

**Exit gate**: 100% correct answers on the covered eval set; unsafe/ambiguous queries are rejected without incorrect generation.

### M3: Secure Execution + MCP MVP

- MCP stdio tools/resources
- Auth scope, principal-to-role mapping, GRANT/RLS, read-only executor
- Budget/timeout/cancel/pagination, audit, structured logs/metrics
- End-to-end agent demo

**Exit gate**: Security suite 100%, cross-tenant leakage 0, audit event loss 0.

### M4: Developer Preview

- Installer/container image, quickstart, sample project, operations docs
- Public API polish, error taxonomy, compatibility policy
- Performance baseline, catalog test at 100-model scale
- Design feedback from 2+ external users

**Exit gate**: A new user can start up within 30 minutes and complete a read-only pilot on a real database.

### M5: Beta

- N-1 migration, backup/restore, failure recovery, SLO/dashboard
- Evaluate the need for MCP streamable HTTP; if adopted, implement with authentication
- Hardening, SBOM/signing, security review, incident runbook
- Adoption/value metrics measurement

**Exit gate**: 4 weeks of operation on 2+ non-fixture databases with no P0/P1 security/correctness defects.

### M6: 0.4 — Governed Ingestion and Portable Linux

**Implementation status:** complete in the `0.4.0` source tree. Promotion
requires the PostgreSQL 16–18 suite, native Linux amd64/arm64 runtime gates,
and post-implementation review to pass on the release commit.

- Specify LSM v1 and a writable-model projection in the Semantic Schema.
- Implement bounded typed `insert` and approved idempotent `upsert`; keep raw
  SQL, arbitrary DML, physical identifiers, `UPDATE`, `DELETE`, `MERGE`,
  `COPY`, and `CALL` outside the public contract.
- Add separate writer credentials/roles, RLS `WITH CHECK` enforcement,
  idempotency, atomic mutation audit, rollback/reconciliation behavior, and
  safe rejection tests.
- Add Linux amd64 and arm64 runtime jobs for released binaries and OCI images,
  including installer, TLS, query, and mutation smoke coverage.
- Publish `0.4` compatibility, migration, operations, incident, and threat-model
  updates without claiming 1.0 stability.

**Exit gate**: On PostgreSQL 16–18, approved inserts/upserts succeed and denied,
ambiguous, duplicate, cross-tenant, or partially failed mutations fail closed
with complete audit evidence; Linux amd64 and arm64 artifacts both execute their
required smoke/contract suites.

### M7: 0.5 — Reference Comparison and Interoperability

- Re-run a documented comparison against current Wren AI, Cube, Malloy, and
  MetricFlow releases using common PostgreSQL datasets and tasks.
- Publish a capability/gap matrix covering authoring, discovery, query
  semantics, mutation, APIs/SDKs, lineage, governance, and operations.
- Add import/export or model-conversion adapters only where they reduce
  adoption cost without making an external model the runtime source of truth.
- Prioritize subsequent work from measured user value rather than feature-count
  parity.

**Exit gate**: The comparison is reproducible, the selected gaps have PostgreSQL
users and fixtures, and every accepted feature preserves PostgreSQL as the only
execution engine and semantic authority.

### M8: 0.6 — Semantic and Mutation Coverage

- Implement the highest-value safe semantic gaps, such as explicitly modeled
  time comparisons, cumulative metrics, or additional relationship patterns.
- Extend mutations to typed `update`/`delete` only if an ADR defines bounded
  semantic predicates, optimistic concurrency, affected-row expectations,
  immutable fields, and recovery behavior.
- Expand known-answer and rejection suites before expanding operators.

**Exit gate**: New query and mutation semantics reach 100% correctness on their
supported fixtures, with ambiguous or unsafe cases rejected and no weakening of
GRANT/RLS.

### M9: 0.7 — Application and Agent Integration

- Add authenticated MCP Streamable HTTP if demand and threat-model gates are
  met.
- Provide generated client schemas/SDK guidance, capability negotiation,
  cancellation, pagination/streaming, and stable idempotency behavior.
- Keep stdio supported and keep remote mutation disabled unless explicit
  authentication, authorization, origin, rate, and audit requirements pass.

**Exit gate**: Multi-user remote deployments preserve the same visibility,
role/RLS, privacy, query, and mutation invariants as local stdio deployments.

### M10: 0.8 — PostgreSQL-native Scale and Operations

- Address measured bottlenecks with PostgreSQL-native techniques such as
  prepared plans, connection management, materialized views, or optional
  pre-aggregation; do not add a second authoritative datastore by default.
- Add large-catalog/model authoring workflows, operational dashboards, upgrade
  automation, and architecture-specific performance baselines.
- Re-run the reference comparison and document intentional non-parity.

**Exit gate**: Supported scale targets and failure recovery are reproducible on
Linux amd64/arm64 and do not compromise determinism, freshness, or database
authorization.

### M11: 0.9 — 1.0 Release Candidate

- Freeze candidate LSQ, LSM, Semantic Schema, MCP, CLI, error, migration, and
  audit contracts.
- Complete independent security review, production pilot evidence, upgrade and
  rollback rehearsals, support policy, governance, and deprecation policy.
- Remove or explicitly defer experimental surfaces that cannot meet 1.0
  compatibility guarantees.

**Exit gate**: No unresolved P0/P1 correctness or security defects, required
platforms pass, N-1 upgrades and recovery rehearsals pass, and release-candidate
users can operate query and ingestion workflows.

### M12: 1.0 — Stable PostgreSQL Semantic Gateway

- Publish stable contracts and documented compatibility/support periods.
- Establish maintainers, release cadence, vulnerability response, and
  sustainability ownership.
- Publish the final reference-comparison and differentiation statement.

**Exit gate**: Correctness, mutation safety, security, migratability,
operability, Linux portability, interoperability, differentiation, governance,
and maintainer sustainability gates are all met.

## 20. Stages from MVP to Official Project

| Stage | Delivered Value | Not Expanded | Promotion Criteria |
|---|---|---|---|
| Spike | End-to-end slice: catalog → simple model → LSQ → SQL | MCP remote, vector, complex joins | Confirm value and feasibility within a ~2-week scope |
| MVP | Single DB, read-only, stdio MCP, basic metrics, RLS | UI, cache, multi-DB, free-form SQL | Eval/security/lineage criteria met |
| Preview | Docs, packaging, real-DB pilot | Distribution, pre-aggregation | External users can self-onboard |
| Beta / 0.3 | Migration, operations, HTTP decision, SLO | Writes and non-PostgreSQL support | 4-week production run and security review |
| 0.4 | Governed insert/upsert and Linux amd64/arm64 runtime support | Arbitrary DML and non-PostgreSQL engines | Mutation security/correctness and dual-architecture runtime gates |
| 0.5 | Reproducible reference comparison and targeted interoperability | Feature-count parity | Evidence-backed gap priorities |
| 0.6 | Broader safe query/mutation semantics | Ambiguous automatic semantics | Correctness and rejection gates |
| 0.7 | Authenticated application/agent integration | Anonymous or request-selected authority | Remote invariants match local invariants |
| 0.8 | PostgreSQL-native scale and operations | Mandatory external cache/source of truth | Measured scale and recovery targets |
| 0.9 | Frozen release-candidate contracts | New experimental surfaces | Production/security/platform evidence complete |
| 1.0 | Stable contract, support, governance | Non-PostgreSQL execution | Ongoing maintainers and compatibility guarantees |

Formal promotion is judged not by code volume but by the following evidence:

- Correctness, safe rejection rate, and auditability improve over raw SQL / MCP servers.
- Users exist for whom placing semantics in PostgreSQL is more advantageous than dual-maintaining external YAML.
- RLS principal propagation can be operated safely.
- Schema drift and migration can be handled realistically.
- There is a rational basis for maintaining an independent core rather than forking/integrating existing OSS.

## 21. Differentiation from Existing OSS

Comparison is framed not as superiority but as differences in scope and where the source of truth resides. Each OSS evolves rapidly, so official documentation is re-evaluated at M0, immediately after M6, at M10, and before 1.0.

| Aspect | Wren AI | Cube | Malloy | MetricFlow | postgresem |
|---|---|---|---|---|---|
| Focus | AI/GenBI context + semantic engine | General-purpose semantic/analytics layer | Semantic modeling/query language | dbt-centric metrics compiler | PostgreSQL-native semantic contract + guarded agent gateway |
| Model source of truth | MDL/YAML etc. project files | YAML/JavaScript etc. code | `.malloy` files | dbt manifest/YAML | PostgreSQL `semantic` schema |
| DB catalog/COMMENT/FK ingestion | Scaffold feature available | Schema generation available | Uses connection schema | dbt manifest centric | catalog/COMMENT/FK/CHECK/RLS as first-class evidence with revision management |
| Compiler | Multi-data-source semantic engine | Multi-data-source semantic layer | Malloy → SQL compiler | Metric query → SQL compiler | PostgreSQL-only typed LSQ → SQL; determinism and safe rejection specified |
| MCP/agent | Available | Available | Available via Publisher, etc. | Not the primary focus | No raw SQL; current LSQ discovery/validate/query/explain, with separately gated typed mutation planned |
| Governed writes | Product/API dependent | API/pre-aggregation workflows | Not a primary semantic contract | Not a primary metric contract | PostgreSQL-only typed ingestion with GRANT/RLS/constraints as authority; starts in 0.4 |
| Security source of truth | Semantic/product policy | Semantic access policy | Combined with connection-target permissions | Combined with dbt/platform | PostgreSQL GRANT/RLS as the ultimate authority; principal propagated to DB |
| Lineage | Product/engine feature | Product/semantic feature | Compiler metadata | Semantic manifest/plan | Built per query from revision, compiler, policy, and source columns |
| Target databases | Many | Many | Multiple | Multiple warehouses | PostgreSQL only |

### 21.1 Unique Differentiators

1. **Meaning lives with data**: The semantic model is subject to PostgreSQL transactions, backups, roles, and migrations.
2. **Native evidence ingestion**: PostgreSQL-specific catalog, COMMENT, constraints, RLS, and dependencies are treated not as auxiliary information but as managed artifacts.
3. **Database-enforced identity**: Rather than relying solely on Gateway policy, the principal is conveyed to the database as a non-`BYPASSRLS` role/context.
4. **Narrow deterministic contract**: Free-form SQL and natural language are not compiler inputs; versioned LSQ and typed IR are the public boundaries.
5. **Lineage by compilation**: Tracks from Semantic Revision to execution source/policy within the same pipeline.
6. **PostgreSQL-only depth**: Rather than abstracting across multiple dialects, deeply leverages PostgreSQL types, RLS, catalog, EXPLAIN, and timezone semantics.

### 21.2 Areas Not Differentiated

- No direct competition with Wren AI's GenBI experience, Cube's cache/pre-aggregation and diverse APIs, Malloy's expressiveness, or the MetricFlow/dbt ecosystem.
- Future import/export adapters and compiler comparison fixtures may be provided to explore coexistence.
- M0 established the initial boundary. M7 and M10 repeat the comparison against
  current releases and may adopt import/export or implementation techniques,
  but not a non-PostgreSQL runtime abstraction or a second semantic source of
  truth.

## 22. Key Risks and Decision Gates

| Risk | Impact | Mitigation | Decision Gate |
|---|---|---|---|
| Small gap from existing OSS | Insufficient value for an independent project | Comparative eval with 3 datasets / 30 questions, user interviews | M0: continue / fork / integrate / stop |
| Join fan-out and metric semantics | Silent incorrect answers | v1 scope restriction, grain/additivity, known-answer/rejection eval | M2: if 100% correct not achieved, narrow scope further |
| RLS principal propagation | Privilege escalation / leakage | Non-owner role, `SET LOCAL`, pool tests, external review | M3: if not met, do not expose remote execution |
| Governed mutation bypass or partial success | Unauthorized/corrupt data or false success | Separate writer role, typed LSM, RLS `WITH CHECK`, constraints, idempotency, atomic audit, rollback/reconciliation tests | M6: if not met, do not expose mutation |
| Semantic Schema pollutes the DB | Adoption refusal / migration accidents | Dedicated schema/role, forward migration, uninstall/export | Pre-Preview: evaluated via real-DB pilot |
| Catalog version diffs / drift | Incorrect model / outage | PG 16–18 fixtures, fingerprinting, explicit scans | Support update decision per PG release |
| COMMENT quality insufficient | Poor discovery accuracy | Explicit term/editor workflow, candidate confidence | Preview: measure operational effort |
| Rust / MCP ecosystem dependencies | Implementation delays / protocol chasing | Adapter separation, protocol contract tests | M0 ADR: finalize stack |
| Self-built compiler maintenance cost | Project unsustainable | Pure core, limited operators, evaluation of reusing existing compilers | M0/M4: re-evaluate build-vs-integrate |
| DoS / high-cost queries | Database outage | Budget, EXPLAIN guard, timeout, concurrency | M3 load/security tests |
| Apple Container / Compose differences | Poor local reproducibility | Compose intersection features, Mac smoke test, Linux CI | M1: confirm identical fixture results |
| Linux architecture gap | Published artifact does not run on a supported CPU | Execute installer/binary/image tests on amd64 and arm64, track native dependencies | M6: both architectures are release blocking |
| Metadata / audit confidentiality | Schema or usage pattern leakage | Metadata RLS, redaction, retention | M3 threat model review |
| Premature pgvector adoption | Increased complexity, misplaced correctness dependency | Post-MVP, ranking only, baseline comparison | Independent ADR after Beta |

### 22.1 Stop Conditions

If any of the following apply, halt feature additions and consider stopping, integrating, or pivoting:

- No pilot users can demonstrate the operational advantages of a PostgreSQL-internal Semantic Schema.
- A thin adapter on existing OSS can satisfy the same requirements.
- Even with a narrowed scope, metric correctness cannot be reliably guaranteed.
- RLS/role mapping cannot be operated safely and understandably.
- No maintainer organization can sustain the independent compiler and schema migrations.

## 23. Priorities

### P0: What Makes the MVP Viable

- LSQ v1 and Semantic Schema v1 specifications
- Immutable revision/publish
- catalog/COMMENT/FK/CHECK/RLS scan and drift
- Typed IR and limited deterministic compiler
- Read-only / RLS-aware executor
- MCP discovery/validate/query/explain
- Lineage/audit, security/golden/integration tests
- Reproducible environment with Apple Container + Linux CI

### P1: Required for Preview/Beta

- Model diff, error UX, expanded samples/evals
- HTTP transport and full authentication (after demand is confirmed)
- Backup/restore, N-1 migration, SLO/dashboard
- Packaging, signed releases, external security review
- Large-catalog pagination/performance

### P2: Required for 0.4

- LSM v1, writable model metadata, insert/upsert compiler and executor
- Separate writer roles, mutation audit/idempotency/reconciliation
- Linux amd64/arm64 runtime and installer execution gates

### P3: Comparison-Driven Before 1.0

- pgvector discovery ranking
- Approved example query retrieval
- Event trigger / CDC drift detection
- Many-to-many / symmetric aggregate
- Cache / pre-aggregation
- Import/export adapters (Wren / Cube / Malloy / MetricFlow)
- UI and natural-language responses
- Non-PostgreSQL dialects remain out of scope through 1.0

## 24. ADRs to Create Before Implementation

1. ADR-001: Rust adoption and dependency selection
2. ADR-002: Semantic Schema v1 and revision/publish model
3. ADR-003: LSQ v1, type system, NULL/timezone/numeric semantics
4. ADR-004: Join cardinality, grain, additivity, rejection rules
5. ADR-005: Principal-to-PostgreSQL role/session context and RLS
6. ADR-006: MCP transport, authentication boundary, tool/resource contract
7. ADR-007: Audit/lineage retention period and sensitive information redaction
8. ADR-008: Migration, backup, compatibility, uninstall/export
9. ADR-009: Build vs. Wren/Malloy/other compiler integration evaluation
10. ADR-010: LSM v1, writable model metadata, insert/upsert semantics, and
    idempotency
11. ADR-011: Writer role/RLS/audit/reconciliation security boundary
12. ADR-012: Linux amd64/arm64 support evidence and release matrix

## 25. Initial Implementation Backlog

1. Finalize repository metadata, license, and contribution/security documentation.
2. Create `compose.yaml`, Containerfile, PG 16–18 fixtures, and `make doctor/dev-up/test`.
3. Build the commerce and RLS multi-tenant fixture and the 30-question correct/rejection eval first.
4. Draft the LSQ v1 and Semantic Expression v1 JSON Schemas as RFCs.
5. Implement Semantic Schema migration v1 and role/privilege design.
6. Implement catalog snapshot/import/drift and pin PG 16–18 golden snapshots.
7. Implement the compiler core's symbol/type/grain validator and typed IR.
8. Implement single-fact, many-to-one, basic aggregate/filter/time grain.
9. Implement the guarded executor, RLS principal mapping, and timeout/limit/cancel.
10. Implement the MCP stdio adapter with 5 tools, query response, and error taxonomy.
11. Implement query-time lineage/audit and observability.
12. Conduct the end-to-end eval and MVP exit review.

### 25.1 M6 Implementation Backlog

1. Write ADRs 010–012 and update the threat model before adding a write-capable
   connection.
2. Define LSM v1 JSON Schema and writable-model snapshot projection with
   accepted and rejection fixtures.
3. Implement a pure deterministic insert/upsert compiler without database,
   transport, logging, or audit I/O.
4. Add dedicated writer roles and migrations without granting write capability
   to the existing runtime query role.
5. Implement idempotency, audit lifecycle, transaction rollback, affected-row
   checks, and reconciliation.
6. Add CLI/MCP capability negotiation with mutation disabled by default.
7. Add Linux amd64/arm64 installer, binary, OCI, TLS, query, and mutation smoke
   jobs.
8. Publish `0.4` migration, compatibility, security, operations, and incident
   documentation.

## 26. Self-Review Results and Reflected Items

**Current review verdict: GO for M6 (`0.4`) with gated scope.** The original
conditional M0 review led to a working read-only beta and is retained below as
historical design rationale. The next justified expansion is governed
ingestion and required Linux portability, not declaring 1.0 or becoming a
general-purpose multi-database semantic layer.

### Technical Weaknesses

- **Join/aggregate correctness is the hardest challenge**: A general-purpose join graph—an easy initial assumption—is dangerous, so the MVP was restricted to single-fact + many-to-one / one-to-one.
- **Re-implementing RLS in the Gateway creates dual sources of authorization**: RLS expressions are treated as ingestion/explanation targets only; enforcement is at the database, with principal propagated transaction-locally.
- **JSON Schema alone cannot guarantee semantics**: Schema validation and semantic/type/grain/cost validation were clearly separated.
- **OIDs are not stable IDs**: Canonical physical names and fingerprints are the source of truth; OIDs are limited to snapshot auxiliaries.
- **Generated SQL determinism and DB execution plan determinism are easily conflated**: The guarantee scope was limited to IR/SQL/parameter/lineage.

### Over-Engineering Reductions

- Microservices, message queues, and an independent policy service were not adopted; a single Gateway was chosen.
- Crate splitting was limited to two: compiler core and binary.
- Event triggers, logical decoding, cache, pre-aggregation, UI, and vector search were excluded from the MVP.
- Many-to-many, multiple facts, window functions, and arbitrary UDFs were deferred.
- The management API was kept CLI-centric rather than exposed as a general MCP tool.

### Gaps Addressed

- Principal mapping and connection pool context-leak countermeasures.
- The unsuitability of COMMENT for storing sensitive information, and RLS on metadata itself.
- numeric/timezone/NULL, join grain/additivity, and result byte limits.
- Schema drift, migration, backup/restore, and uninstall/export considerations.
- "Queries that should be safely rejected" added as a quality metric alongside correct answers.
- Stop conditions and build-vs-integrate re-evaluation gates.

### Priority Review

The M6 priority is the mutation contract, writer/RLS boundary, rollback and
audit correctness, and Linux amd64/arm64 execution evidence. After `0.4`, the
priority is a reproducible reference comparison and user evidence. MCP or
natural-language breadth, caching, and richer semantics are added only when
their gates preserve the safe core. pgvector is adopted only if the discovery
baseline proves insufficient.

## 27. Official References

- PostgreSQL: [System Catalogs](https://www.postgresql.org/docs/current/catalogs.html)
- PostgreSQL: [`COMMENT`](https://www.postgresql.org/docs/current/sql-comment.html)
- PostgreSQL: [Constraints](https://www.postgresql.org/docs/current/ddl-constraints.html)
- PostgreSQL: [Row Security Policies](https://www.postgresql.org/docs/current/ddl-rowsecurity.html)
- PostgreSQL: [`INSERT`](https://www.postgresql.org/docs/current/sql-insert.html)
- PostgreSQL: [`CREATE POLICY`](https://www.postgresql.org/docs/current/sql-createpolicy.html)
- Apple: [`container`](https://github.com/apple/container)
- Wren AI: [What is Modeling Definition Language (MDL)?](https://docs.getwren.ai/oss/engine/concept/what_is_mdl)
- Wren AI: [MDL schema reference](https://docs.getwren.ai/oss/reference/mdl)
- Cube: [Introduction / Semantic Layer Architecture](https://docs.cube.dev/docs/introduction)
- Cube: [Access Control](https://docs.cube.dev/docs/data-modeling/access-control/index)
- Malloy: [Official repository and architecture overview](https://github.com/malloydata/malloy)
- MetricFlow: [Metric semantics in dbt-core](https://github.com/dbt-labs/dbt-core/blob/main/crates/dbt-metricflow/docs/metric-semantics.md)
