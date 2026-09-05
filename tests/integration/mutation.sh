#!/bin/sh
set -eu

export POSTGRESEM_MUTATION_DATABASE_URL="host=${PGHOST} port=${PGPORT} dbname=${PGDATABASE} user=postgresem_mutation_runtime password=${POSTGRESEM_MUTATION_RUNTIME_PASSWORD} sslmode=disable"
export POSTGRESEM_AUDIT_DATABASE_URL="host=${PGHOST} port=${PGPORT} dbname=${PGDATABASE} user=postgresem_audit_writer password=${POSTGRESEM_AUDIT_WRITER_PASSWORD} sslmode=disable"
export POSTGRESEM_MUTATION_DB_ROLE=postgresem_order_writer
export POSTGRESEM_MAX_MUTATION_RESULT_BYTES=1048576
TEST_ROOT=${TEST_ROOT:-/tests}

psql --no-psqlrc -v ON_ERROR_STOP=1 -f "${TEST_ROOT}/mutation_reconciliation.sql"

psql --no-psqlrc -v ON_ERROR_STOP=1 <<'SQL'
TRUNCATE semantic.mutation_audit, semantic.mutation_idempotency;
DELETE FROM commerce.orders WHERE external_id LIKE 'integration-%';
DELETE FROM rls_fixture.orders WHERE external_id LIKE 'integration-%';

DO $$
BEGIN
  IF pg_has_role('postgresem_runtime', 'postgresem_order_writer', 'MEMBER') THEN
    RAISE EXCEPTION 'query runtime can assume a writer role';
  END IF;
  IF pg_has_role('postgresem_mutation_runtime', 'postgresem_analyst', 'MEMBER')
    OR pg_has_role('postgresem_mutation_runtime', 'postgresem_source_owner', 'MEMBER')
    OR pg_has_role('postgresem_mutation_runtime', 'postgresem_test_superuser', 'MEMBER')
    OR pg_has_role('postgresem_mutation_runtime', 'postgresem_test_bypassrls', 'MEMBER')
  THEN
    RAISE EXCEPTION 'mutation runtime can assume a query or unsafe role';
  END IF;
  IF has_table_privilege(
    'postgresem_runtime',
    'commerce.orders',
    'INSERT, UPDATE, DELETE'
  ) THEN
    RAISE EXCEPTION 'query runtime has business-data write privileges';
  END IF;
  IF has_table_privilege(
    'postgresem_mutation_runtime',
    'semantic.mutation_audit',
    'SELECT, INSERT, UPDATE, DELETE'
  ) OR has_table_privilege(
    'postgresem_audit_writer',
    'semantic.mutation_audit',
    'SELECT, INSERT, UPDATE, DELETE'
  ) THEN
    RAISE EXCEPTION 'runtime or audit login has direct mutation audit privileges';
  END IF;
END;
$$;
SQL

psql --no-psqlrc -v ON_ERROR_STOP=1 \
  -c "GRANT postgresem_test_bypassrls TO postgresem_order_writer"
if postgresem mutation execute "${TEST_ROOT}/mutations/order-insert.json" \
  --project commerce >/dev/null 2>&1
then
  echo "writer role inherited a BYPASSRLS role" >&2
  exit 1
fi
psql --no-psqlrc -v ON_ERROR_STOP=1 \
  -c "REVOKE postgresem_test_bypassrls FROM postgresem_order_writer"

psql --no-psqlrc -v ON_ERROR_STOP=1 \
  -c "GRANT postgresem_source_owner TO postgresem_order_writer"
if postgresem mutation execute "${TEST_ROOT}/mutations/order-insert.json" \
  --project commerce >/dev/null 2>&1
then
  echo "writer role inherited the target relation owner" >&2
  exit 1
fi
psql --no-psqlrc -v ON_ERROR_STOP=1 \
  -c "REVOKE postgresem_source_owner FROM postgresem_order_writer"

if POSTGRESEM_MUTATION_DATABASE_URL="${POSTGRESEM_MUTATION_DATABASE_URL% sslmode=disable}" \
  postgresem mutation execute "${TEST_ROOT}/mutations/order-insert.json" \
  --project commerce >/dev/null 2>&1
then
  echo "mutation accepted a connection without explicit sslmode" >&2
  exit 1
fi

inserted=$(
  postgresem mutation execute "${TEST_ROOT}/mutations/order-insert.json" --project commerce
)
printf '%s\n' "$inserted" | grep -q '"replayed": false'
printf '%s\n' "$inserted" | grep -q '"affected_rows": 1'
printf '%s\n' "$inserted" | grep -q '"integration-order-1"'
if printf '%s\n' "$inserted" |
  grep -Eq '"source_columns"|"sql"|"statement"|INSERT[[:space:]]+INTO'
then
  echo "mutation response exposed a physical mutation surface" >&2
  exit 1
fi
mutation_id=$(
  printf '%s\n' "$inserted" |
    python3 -c 'import json,sys; print(json.load(sys.stdin)["mutation_id"])'
)

replayed=$(
  postgresem mutation execute "${TEST_ROOT}/mutations/order-insert.json" --project commerce
)
printf '%s\n' "$replayed" | grep -q '"replayed": true'
printf '%s\n' "$replayed" | grep -q "\"mutation_id\": \"$mutation_id\""
if [ "$(
  psql --no-psqlrc --tuples-only --no-align -c \
    "SELECT count(*) FROM commerce.orders WHERE external_id = 'integration-order-1'"
)" != "1" ]; then
  echo "idempotent replay duplicated a row" >&2
  exit 1
fi

if postgresem mutation execute \
  "${TEST_ROOT}/mutations/order-idempotency-conflict.json" \
  --project commerce >/dev/null 2>&1
then
  echo "idempotency key reuse with different content succeeded" >&2
  exit 1
fi

upserted=$(
  postgresem mutation execute "${TEST_ROOT}/mutations/order-upsert.json" --project commerce
)
printf '%s\n' "$upserted" | grep -q '"paid"'
printf '%s\n' "$upserted" | grep -q '"70.50"'

for rejected in partial-batch generated-field invalid-raw-sql; do
  if postgresem mutation execute "${TEST_ROOT}/mutations/${rejected}.json" \
    --project commerce >/dev/null 2>&1
  then
    echo "unsafe mutation succeeded: $rejected" >&2
    exit 1
  fi
done
if [ "$(
  psql --no-psqlrc --tuples-only --no-align -c \
    "SELECT count(*) FROM commerce.orders WHERE external_id IN ('integration-batch-valid', 'integration-batch-invalid', 'integration-generated-field', 'integration-raw-sql')"
)" != "0" ]; then
  echo "rejected mutation left a partial business-data change" >&2
  exit 1
fi

export POSTGRESEM_MUTATION_DB_ROLE=postgresem_tenant_a_writer
export POSTGRESEM_MUTATION_AUTHORITY_ID=tenant-a
tenant_insert=$(
  postgresem mutation execute "${TEST_ROOT}/mutations/tenant-a-insert.json" --project commerce
)
printf '%s\n' "$tenant_insert" | grep -q '"tenant_a"'
tenant_a_mutation_id=$(
  printf '%s\n' "$tenant_insert" |
    python3 -c 'import json,sys; print(json.load(sys.stdin)["mutation_id"])'
)
export POSTGRESEM_MUTATION_DB_ROLE=postgresem_tenant_b_writer
export POSTGRESEM_MUTATION_AUTHORITY_ID=tenant-b
tenant_b_insert=$(
  postgresem mutation execute \
    "${TEST_ROOT}/mutations/tenant-b-same-key-insert.json" \
    --project commerce
)
printf '%s\n' "$tenant_b_insert" | grep -q '"tenant_b"'
tenant_b_mutation_id=$(
  printf '%s\n' "$tenant_b_insert" |
    python3 -c 'import json,sys; print(json.load(sys.stdin)["mutation_id"])'
)
if [ "$tenant_a_mutation_id" = "$tenant_b_mutation_id" ]; then
  echo "idempotency state was shared across mapped writer authorities" >&2
  exit 1
fi
export POSTGRESEM_IDEMPOTENCY_KEY=integration-tenant-a-insert
tenant_b_reconciled=$(postgresem mutation reconcile --project commerce)
printf '%s\n' "$tenant_b_reconciled" |
  grep -q "\"mutation_id\": \"$tenant_b_mutation_id\""
export POSTGRESEM_MUTATION_DB_ROLE=postgresem_tenant_a_writer
export POSTGRESEM_MUTATION_AUTHORITY_ID=tenant-a
tenant_a_reconciled=$(postgresem mutation reconcile --project commerce)
printf '%s\n' "$tenant_a_reconciled" |
  grep -q "\"mutation_id\": \"$tenant_a_mutation_id\""
remapped_reconciled=$(
  POSTGRESEM_MUTATION_DB_ROLE=postgresem_tenant_b_writer \
    postgresem mutation reconcile --project commerce
)
printf '%s\n' "$remapped_reconciled" |
  python3 -c 'import json,sys; assert json.load(sys.stdin)["state"] is None'
if [ "$(
  psql --no-psqlrc --tuples-only --no-align -c \
    "SELECT count(*) FROM semantic.mutation_idempotency WHERE idempotency_key_hash = '$(printf %s integration-tenant-a-insert | sha256sum | awk '{print "sha256:" $1}')'"
)" != "2" ]; then
  echo "idempotency rows were not namespaced by authority" >&2
  exit 1
fi
export POSTGRESEM_MUTATION_DB_ROLE=postgresem_tenant_a_writer
if postgresem mutation execute "${TEST_ROOT}/mutations/tenant-cross-insert.json" \
  --project commerce >/dev/null 2>&1
then
  echo "cross-tenant insert bypassed RLS WITH CHECK" >&2
  exit 1
fi
if [ "$(
  psql --no-psqlrc --tuples-only --no-align -c \
    "SELECT count(*) FROM rls_fixture.orders WHERE external_id = 'integration-tenant-cross-1'"
)" != "0" ]; then
  echo "cross-tenant rejection wrote a row" >&2
  exit 1
fi

export POSTGRESEM_MUTATION_DB_ROLE=postgresem_order_writer
unset POSTGRESEM_MUTATION_AUTHORITY_ID
export POSTGRESEM_MAX_MUTATION_RESULT_BYTES=2
if postgresem mutation execute "${TEST_ROOT}/mutations/result-limit.json" \
  --project commerce >/dev/null 2>&1
then
  echo "oversized mutation result was reported as success" >&2
  exit 1
fi
export POSTGRESEM_MAX_MUTATION_RESULT_BYTES=1048576
if [ "$(
  psql --no-psqlrc --tuples-only --no-align -c \
    "SELECT count(*) FROM commerce.orders WHERE external_id = 'integration-result-limit'"
)" != "0" ]; then
  echo "result-byte rejection committed business data" >&2
  exit 1
fi

psql --no-psqlrc -v ON_ERROR_STOP=1 <<'SQL'
REVOKE EXECUTE ON FUNCTION semantic.finish_mutation(
  uuid, uuid, jsonb, bigint, bigint
) FROM postgresem_mutator;
SQL
if postgresem mutation execute "${TEST_ROOT}/mutations/audit-failure.json" \
  --project commerce >/dev/null 2>&1
then
  echo "mutation succeeded after atomic audit finalization was denied" >&2
  exit 1
fi
psql --no-psqlrc -v ON_ERROR_STOP=1 <<'SQL'
GRANT EXECUTE ON FUNCTION semantic.finish_mutation(
  uuid, uuid, jsonb, bigint, bigint
) TO postgresem_mutator;
SQL
if [ "$(
  psql --no-psqlrc --tuples-only --no-align -c \
    "SELECT count(*) FROM commerce.orders WHERE external_id = 'integration-audit-failure'"
)" != "0" ]; then
  echo "atomic audit failure committed business data" >&2
  exit 1
fi

export POSTGRESEM_IDEMPOTENCY_KEY=integration-order-insert
reconciled=$(
  postgresem mutation reconcile --project commerce
)
printf '%s\n' "$reconciled" | grep -q '"status": "committed"'
printf '%s\n' "$reconciled" | grep -q "\"mutation_id\": \"$mutation_id\""

psql --no-psqlrc -v ON_ERROR_STOP=1 <<'SQL'
DO $$
BEGIN
  IF EXISTS (
    SELECT 1
    FROM semantic.mutation_audit
    WHERE status = 'started'
      OR completed_at IS NULL
      OR lsm_hash !~ '^sha256:[0-9a-f]{64}$'
      OR principal_subject_hash !~ '^sha256:[0-9a-f]{64}$'
  ) THEN
    RAISE EXCEPTION 'mutation audit lifecycle is incomplete';
  END IF;
  IF NOT EXISTS (
    SELECT 1
    FROM semantic.mutation_audit
    WHERE status = 'committed' AND replayed
  ) OR NOT EXISTS (
    SELECT 1
    FROM semantic.mutation_audit
    WHERE status = 'rejected'
      AND error_code IN (
        'MUTATION_IDEMPOTENCY_CONFLICT',
        'MUTATION_FIELD_NOT_WRITABLE',
        'LSM_INVALID_JSON'
      )
  ) OR NOT EXISTS (
    SELECT 1
    FROM semantic.mutation_audit
    WHERE status = 'rolled_back'
      AND error_code IN (
        'MUTATION_DATABASE_REJECTED',
        'MUTATION_RESULT_BYTES_EXCEEDED',
        'MUTATION_ATOMIC_AUDIT_FAILED'
      )
  ) THEN
    RAISE EXCEPTION 'required mutation audit outcomes are missing';
  END IF;
  IF EXISTS (
    SELECT 1
    FROM semantic.mutation_audit
    WHERE row_to_json(mutation_audit)::text LIKE '%integration-order-1%'
       OR row_to_json(mutation_audit)::text LIKE '%70.50%'
  ) THEN
    RAISE EXCEPTION 'mutation values leaked into the audit record';
  END IF;
END;
$$;

DELETE FROM commerce.orders WHERE external_id LIKE 'integration-%';
DELETE FROM rls_fixture.orders WHERE external_id LIKE 'integration-%';
TRUNCATE semantic.mutation_audit, semantic.mutation_idempotency;
SQL

echo "governed mutation integration checks passed"
