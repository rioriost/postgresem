#!/bin/sh
set -eu

export DATABASE_URL="host=${PGHOST} port=${PGPORT} dbname=${PGDATABASE} user=postgresem_runtime password=${POSTGRESEM_RUNTIME_PASSWORD} sslmode=disable"
export POSTGRESEM_AUDIT_DATABASE_URL="host=${PGHOST} port=${PGPORT} dbname=${PGDATABASE} user=postgresem_audit_writer password=${POSTGRESEM_AUDIT_WRITER_PASSWORD} sslmode=disable"
export POSTGRESEM_MAX_RESULT_BYTES=1048576

psql --no-psqlrc -v ON_ERROR_STOP=1 <<'SQL'
TRUNCATE semantic.query_audit;
DO $$
BEGIN
  IF pg_has_role(
    'postgresem_runtime',
    'postgresem_source_owner',
    'MEMBER'
  ) OR pg_has_role(
    'postgresem_runtime',
    'postgresem_test_superuser',
    'MEMBER'
  ) OR pg_has_role(
    'postgresem_runtime',
    'postgresem_test_bypassrls',
    'MEMBER'
  ) THEN
    RAISE EXCEPTION 'runtime login can SET ROLE to an unsafe role';
  END IF;
END;
$$;
SQL

export POSTGRESEM_DB_ROLE=postgresem_analyst
if DATABASE_URL="${DATABASE_URL% sslmode=disable}" \
  postgresem query execute /tests/queries/commerce-revenue.json \
  --project commerce >/dev/null 2>&1
then
  echo "execution accepted a connection without an explicit sslmode" >&2
  exit 1
fi
if DATABASE_URL="${DATABASE_URL%sslmode=disable}sslmode=require" \
  postgresem query execute /tests/queries/commerce-revenue.json \
  --project commerce >/dev/null 2>&1
then
  echo "sslmode=require unexpectedly downgraded to plaintext" >&2
  exit 1
fi

psql --no-psqlrc -v ON_ERROR_STOP=1 <<'SQL'
ALTER ROLE postgresem_runtime RESET search_path;
DROP SCHEMA IF EXISTS postgresem_attacker CASCADE;
CREATE SCHEMA postgresem_attacker;
CREATE FUNCTION postgresem_attacker.date_trunc(text, timestamptz)
RETURNS timestamptz
LANGUAGE plpgsql
AS $$
BEGIN
  RAISE EXCEPTION 'untrusted search_path function executed';
END;
$$;
GRANT USAGE ON SCHEMA postgresem_attacker TO postgresem_analyst;
ALTER ROLE postgresem_runtime
  SET search_path = postgresem_attacker, pg_catalog;
SQL
monthly_revenue=$(
  postgresem query execute /tests/queries/monthly-revenue.json --project commerce
)
printf '%s\n' "$monthly_revenue" | grep -q '"semantic_revision": "sha256:'
psql --no-psqlrc -v ON_ERROR_STOP=1 <<'SQL'
ALTER ROLE postgresem_runtime RESET search_path;
DROP SCHEMA postgresem_attacker CASCADE;
SQL

commerce=$(
  postgresem query execute /tests/queries/commerce-revenue.json --project commerce
)
printf '%s\n' "$commerce" | grep -q '"semantic_revision": "sha256:'
printf '%s\n' "$commerce" | grep -q '"200.50"'
printf '%s\n' "$commerce" | grep -q '"truncated": false'

typed_order=$(
  postgresem query execute /tests/queries/typed-order.json --project commerce
)
printf '%s\n' "$typed_order" | grep -q '1,'
printf '%s\n' "$typed_order" | grep -q '"2026-01-15T10:00:00+00:00"'
printf '%s\n' "$typed_order" | grep -q '"120.00"'

typed_subscription=$(
  postgresem query execute /tests/queries/typed-subscription.json --project commerce
)
printf '%s\n' "$typed_subscription" | grep -q '101,'
printf '%s\n' "$typed_subscription" | grep -q '"2026-01-01"'
printf '%s\n' "$typed_subscription" | grep -q 'true'

export POSTGRESEM_DB_ROLE=postgresem_tenant_a
tenant_a=$(
  postgresem query execute /tests/queries/tenant-revenue.json --project commerce
)
printf '%s\n' "$tenant_a" | grep -q '"250.00"'
if printf '%s\n' "$tenant_a" | grep -q '"999.00"'; then
  echo "tenant A execution leaked tenant B rows" >&2
  exit 1
fi

export POSTGRESEM_DB_ROLE=postgresem_tenant_b
tenant_b=$(
  postgresem query execute /tests/queries/tenant-revenue.json --project commerce
)
printf '%s\n' "$tenant_b" | grep -q '"999.00"'
if printf '%s\n' "$tenant_b" | grep -q '"250.00"'; then
  echo "tenant B execution leaked tenant A rows" >&2
  exit 1
fi

for unsafe_role in \
  postgresem_source_owner \
  postgresem_test_superuser \
  postgresem_test_bypassrls
do
  export POSTGRESEM_DB_ROLE=$unsafe_role
  if postgresem query execute /tests/queries/commerce-revenue.json \
    --project commerce >/dev/null 2>&1
  then
    echo "unsafe mapped role was accepted: $unsafe_role" >&2
    exit 1
  fi
done

export POSTGRESEM_DB_ROLE=postgresem_analyst
export POSTGRESEM_MAX_RESULT_BYTES=2
limited=$(
  postgresem query execute /tests/queries/order-statuses.json --project commerce
)
printf '%s\n' "$limited" | grep -q '"rows": \[\]'
printf '%s\n' "$limited" | grep -q '"truncated": true'
export POSTGRESEM_MAX_RESULT_BYTES=1048576

psql --no-psqlrc -v ON_ERROR_STOP=1 <<'SQL'
DO $$
BEGIN
  IF has_table_privilege(
    'postgresem_audit_writer',
    'semantic.query_audit',
    'SELECT, INSERT, UPDATE, DELETE'
  ) THEN
    RAISE EXCEPTION 'audit writer retained direct query audit table privileges';
  END IF;
  IF NOT has_function_privilege(
    'postgresem_audit_writer',
    'semantic.start_query_audit(text,text,text,uuid,text,text,text,text,text,jsonb,jsonb,jsonb,bigint,bigint)',
    'EXECUTE'
  ) OR NOT has_function_privilege(
    'postgresem_audit_writer',
    'semantic.finish_query_audit(uuid,text,text,bigint,bigint,bigint,bigint,boolean)',
    'EXECUTE'
  ) THEN
    RAISE EXCEPTION 'audit writer does not have the audit lifecycle functions';
  END IF;
  IF (SELECT count(*) FROM semantic.query_audit) <> 10 THEN
    RAISE EXCEPTION 'unexpected guarded execution audit count';
  END IF;
  IF EXISTS (
    SELECT 1
    FROM semantic.query_audit
    WHERE status = 'started'
      OR completed_at IS NULL
      OR canonical_lsq_hash !~ '^sha256:[0-9a-f]{64}$'
      OR semantic_revision_hash !~ '^sha256:[0-9a-f]{64}$'
      OR generated_sql_hash !~ '^sha256:[0-9a-f]{64}$'
      OR compiler_query_hash !~ '^sha256:[0-9a-f]{64}$'
      OR jsonb_array_length(parameter_types) = 0
      OR lineage = '{}'::jsonb
  ) THEN
    RAISE EXCEPTION 'guarded execution audit fields or lifecycle are invalid';
  END IF;
  IF (
    SELECT count(*)
    FROM semantic.query_audit
    WHERE status = 'succeeded'
  ) <> 7 THEN
    RAISE EXCEPTION 'unexpected successful guarded execution audit count';
  END IF;
  IF (
    SELECT count(*)
    FROM semantic.query_audit
    WHERE status = 'failed'
  ) <> 3 THEN
    RAISE EXCEPTION 'unexpected failed guarded execution audit count';
  END IF;
  IF NOT EXISTS (
    SELECT 1
    FROM semantic.query_audit
    WHERE status = 'succeeded'
      AND policy_context->>'database_role' = 'postgresem_analyst'
      AND canonical_lsq_hash =
        'sha256:6811641914fa00468a5e8dcc52dd725815974bdcc1791b927a8381d07a3c7c8b'
      AND semantic_revision_hash =
        'sha256:806f8687c1e2161f65370e0c433832760c02b6f96f8b8bc6e93fde6295d29da6'
      AND generated_sql_hash =
        'sha256:b18e050abd61f49a7f0960ffb58c5648c7af8e1d29c4c046e8d9cfadb280ec8f'
  ) THEN
    RAISE EXCEPTION 'commerce execution audit hashes did not match compiler output';
  END IF;
  IF NOT EXISTS (
    SELECT 1
    FROM semantic.query_audit
    WHERE status = 'succeeded'
      AND truncated
      AND byte_count = 2
  ) THEN
    RAISE EXCEPTION 'result byte limit was not audited';
  END IF;
END;
$$;
SQL

psql --no-psqlrc -v ON_ERROR_STOP=1 <<'SQL'
REVOKE EXECUTE ON FUNCTION semantic.start_query_audit(
  text, text, text, uuid, text, text, text, text, text, jsonb, jsonb, jsonb, bigint, bigint
) FROM postgresem_auditor;
SQL
before_count=$(
  psql --no-psqlrc --tuples-only --no-align -c \
    'SELECT count(*) FROM semantic.query_audit'
)
if postgresem query execute /tests/queries/commerce-revenue.json \
  --project commerce >/dev/null 2>&1
then
  echo "execution continued after mandatory started audit failure" >&2
  exit 1
fi
after_count=$(
  psql --no-psqlrc --tuples-only --no-align -c \
    'SELECT count(*) FROM semantic.query_audit'
)
if [ "$before_count" != "$after_count" ]; then
  echo "failed started audit unexpectedly wrote a row" >&2
  exit 1
fi
psql --no-psqlrc -v ON_ERROR_STOP=1 <<'SQL'
GRANT EXECUTE ON FUNCTION semantic.start_query_audit(
  text, text, text, uuid, text, text, text, text, text, jsonb, jsonb, jsonb, bigint, bigint
) TO postgresem_auditor;
REVOKE EXECUTE ON FUNCTION semantic.finish_query_audit(
  uuid, text, text, bigint, bigint, bigint, bigint, boolean
) FROM postgresem_auditor;
SQL

terminal_failure=$(
  postgresem query execute /tests/queries/commerce-revenue.json \
    --project commerce 2>&1 || true
)
if printf '%s\n' "$terminal_failure" | grep -q '"schema_version"'; then
  echo "terminal audit failure returned a success-shaped result" >&2
  exit 1
fi

psql --no-psqlrc -v ON_ERROR_STOP=1 <<'SQL'
GRANT EXECUTE ON FUNCTION semantic.finish_query_audit(
  uuid, text, text, bigint, bigint, bigint, bigint, boolean
) TO postgresem_auditor;
UPDATE semantic.query_audit
SET
  status = 'failed',
  error_code = 'INTEGRATION_TERMINAL_AUDIT_FAILURE',
  completed_at = clock_timestamp()
WHERE status = 'started';
SQL

echo "guarded execution integration checks passed"
