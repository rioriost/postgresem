#!/bin/sh
set -eu

export DATABASE_URL="host=${PGHOST} port=${PGPORT} dbname=${PGDATABASE} user=postgresem_runtime password=${POSTGRESEM_RUNTIME_PASSWORD} sslmode=disable"
export POSTGRESEM_AUDIT_DATABASE_URL="host=${PGHOST} port=${PGPORT} dbname=${PGDATABASE} user=postgresem_audit_writer password=${POSTGRESEM_AUDIT_WRITER_PASSWORD} sslmode=disable"
export POSTGRESEM_MAX_RESULT_BYTES=1048576
TEST_ROOT=${TEST_ROOT:-/tests}

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

psql --no-psqlrc -v ON_ERROR_STOP=1 <<'SQL'
DROP VIEW IF EXISTS commerce.catalog_security_view;
DROP AGGREGATE IF EXISTS commerce.catalog_security_total(bigint);
DROP FUNCTION IF EXISTS commerce.catalog_security_add(bigint, bigint);
DROP FUNCTION IF EXISTS commerce.catalog_security_filter(bigint);
DROP SEQUENCE IF EXISTS commerce.catalog_empty_acl_sequence;
CREATE SEQUENCE commerce.catalog_empty_acl_sequence;
REVOKE ALL ON SEQUENCE commerce.catalog_empty_acl_sequence
  FROM PUBLIC, postgres;
CREATE FUNCTION commerce.catalog_security_add(state bigint, value bigint)
RETURNS bigint
LANGUAGE sql
IMMUTABLE
AS 'SELECT state + value';
CREATE AGGREGATE commerce.catalog_security_total(bigint) (
  SFUNC = commerce.catalog_security_add,
  STYPE = bigint,
  INITCOND = '0',
  PARALLEL = SAFE
);
CREATE FUNCTION commerce.catalog_security_filter(value bigint)
RETURNS boolean
LANGUAGE sql
STABLE
SECURITY DEFINER
SET search_path = pg_catalog
AS 'SELECT value > 0';
ALTER FUNCTION commerce.catalog_security_filter(bigint)
  OWNER TO postgresem_source_owner;
CREATE VIEW commerce.catalog_security_view
WITH (security_invoker = on, security_barrier = 1)
AS
SELECT amount
FROM commerce.orders
WHERE commerce.catalog_security_filter(order_id);
ALTER VIEW commerce.catalog_security_view OWNER TO postgresem_analyst;
SQL
postgresem catalog scan > /tmp/catalog-view-before.json
python3 - /tmp/catalog-view-before.json <<'PY'
import json
import sys

with open(sys.argv[1], encoding="utf-8") as stream:
    snapshot = json.load(stream)
view = next(
    relation["view"]
    for relation in snapshot["relations"]
    if relation["schema"] == "commerce"
    and relation["name"] == "catalog_security_view"
)
if not view["security_invoker"] or not view["security_barrier"]:
    raise SystemExit("catalog scan did not normalize PostgreSQL view booleans")
function = next(
    function
    for function in snapshot["functions"]
    if function["schema"] == "commerce"
    and function["name"] == "catalog_security_filter"
)
if view["owner_authorization"] is not None:
    raise SystemExit("security-invoker view unexpectedly bound owner authority")
if function["owner_authorization"] is None:
    raise SystemExit("security-definer function omitted owner authority")
aggregate = next(
    function
    for function in snapshot["functions"]
    if function["schema"] == "commerce"
    and function["name"] == "catalog_security_total"
)
if aggregate["kind"] != "aggregate":
    raise SystemExit("catalog scan omitted user-defined aggregate evidence")
PY
psql --no-psqlrc -v ON_ERROR_STOP=1 <<'SQL'
DROP AGGREGATE commerce.catalog_security_total(bigint);
CREATE AGGREGATE commerce.catalog_security_total(bigint) (
  SFUNC = commerce.catalog_security_add,
  STYPE = bigint,
  INITCOND = '0',
  PARALLEL = SAFE
);
SQL
postgresem catalog scan > /tmp/catalog-aggregate-recreated.json
python3 - \
  /tmp/catalog-view-before.json \
  /tmp/catalog-aggregate-recreated.json <<'PY'
import json
import sys

with open(sys.argv[1], encoding="utf-8") as stream:
    before = json.load(stream)
with open(sys.argv[2], encoding="utf-8") as stream:
    after = json.load(stream)
if before["fingerprint"] != after["fingerprint"]:
    raise SystemExit("identical aggregate recreation changed catalog fingerprint")
PY
psql --no-psqlrc -v ON_ERROR_STOP=1 <<'SQL'
DROP AGGREGATE commerce.catalog_security_total(bigint);
CREATE AGGREGATE commerce.catalog_security_total(bigint) (
  SFUNC = commerce.catalog_security_add,
  STYPE = bigint,
  INITCOND = '0',
  PARALLEL = UNSAFE
);
SQL
postgresem catalog scan > /tmp/catalog-aggregate-parallel.json
if postgresem catalog diff \
  --from /tmp/catalog-view-before.json \
  --to /tmp/catalog-aggregate-parallel.json \
  --fail-on-breaking >/tmp/catalog-aggregate-parallel-diff.json 2>/dev/null
then
  echo "catalog diff missed aggregate parallel-safety drift" >&2
  exit 1
fi
python3 - /tmp/catalog-aggregate-parallel-diff.json <<'PY'
import json
import sys

with open(sys.argv[1], encoding="utf-8") as stream:
    diff = json.load(stream)
if not any(
    change["path"].startswith(
        "/functions/commerce/catalog_security_total/"
    )
    and change["compatibility"] == "breaking"
    for change in diff["changes"]
):
    raise SystemExit("aggregate parallel-safety drift was not breaking")
PY
psql --no-psqlrc -v ON_ERROR_STOP=1 <<'SQL'
DROP AGGREGATE commerce.catalog_security_total(bigint);
CREATE AGGREGATE commerce.catalog_security_total(bigint) (
  SFUNC = commerce.catalog_security_add,
  STYPE = bigint,
  INITCOND = '0',
  PARALLEL = SAFE
);
SQL
psql --no-psqlrc -v ON_ERROR_STOP=1 <<'SQL'
ALTER VIEW commerce.catalog_security_view
  RESET (security_invoker, security_barrier);
SQL
postgresem catalog scan > /tmp/catalog-view-after.json
if postgresem catalog diff \
  --from /tmp/catalog-view-before.json \
  --to /tmp/catalog-view-after.json \
  --fail-on-breaking >/tmp/catalog-view-diff.json 2>/dev/null
then
  echo "catalog diff missed view security option drift" >&2
  exit 1
fi
python3 - /tmp/catalog-view-diff.json <<'PY'
import json
import sys

with open(sys.argv[1], encoding="utf-8") as stream:
    diff = json.load(stream)
view_changes = [
    change
    for change in diff["changes"]
    if change["path"] == "/relations/commerce/catalog_security_view/view"
]
if diff["compatibility"] != "breaking" or len(view_changes) != 1:
    raise SystemExit("view security drift was not classified as breaking")
PY
psql --no-psqlrc -v ON_ERROR_STOP=1 \
  -c 'ALTER ROLE postgresem_analyst BYPASSRLS'
postgresem catalog scan > /tmp/catalog-view-owner-after.json
if postgresem catalog diff \
  --from /tmp/catalog-view-after.json \
  --to /tmp/catalog-view-owner-after.json \
  --fail-on-breaking >/tmp/catalog-view-owner-diff.json 2>/dev/null
then
  echo "catalog diff missed security-definer view owner authorization drift" >&2
  exit 1
fi
python3 - /tmp/catalog-view-owner-diff.json <<'PY'
import json
import sys

with open(sys.argv[1], encoding="utf-8") as stream:
    diff = json.load(stream)
if not any(
    change["path"] == "/relations/commerce/catalog_security_view/view"
    and change["compatibility"] == "breaking"
    for change in diff["changes"]
):
    raise SystemExit("view owner authorization drift was not classified as breaking")
if not any(
    change["path"] == "/role_graph_fingerprint"
    and change["compatibility"] == "breaking"
    for change in diff["changes"]
):
    raise SystemExit("non-scanner role authorization drift was not classified as breaking")
PY
psql --no-psqlrc -v ON_ERROR_STOP=1 <<'SQL'
ALTER ROLE postgresem_analyst NOBYPASSRLS;
ALTER SEQUENCE commerce.catalog_empty_acl_sequence
  OWNER TO postgresem_analyst;
SQL
postgresem catalog scan > /tmp/catalog-object-acl-after.json
if postgresem catalog diff \
  --from /tmp/catalog-view-after.json \
  --to /tmp/catalog-object-acl-after.json \
  --fail-on-breaking >/tmp/catalog-object-acl-diff.json 2>/dev/null
then
  echo "catalog diff missed normalized object ACL drift" >&2
  exit 1
fi
python3 - /tmp/catalog-object-acl-diff.json <<'PY'
import json
import sys

with open(sys.argv[1], encoding="utf-8") as stream:
    diff = json.load(stream)
if not any(
    change["path"] == "/object_privilege_fingerprint"
    and change["compatibility"] == "breaking"
    for change in diff["changes"]
):
    raise SystemExit("object ACL drift was not classified as breaking")
PY
psql --no-psqlrc -v ON_ERROR_STOP=1 <<'SQL'
ALTER SEQUENCE commerce.catalog_empty_acl_sequence OWNER TO postgres;
CREATE OR REPLACE FUNCTION commerce.catalog_security_filter(value bigint)
RETURNS boolean
LANGUAGE sql
STABLE
SECURITY DEFINER
SET search_path = pg_catalog
AS 'SELECT value >= 0';
SQL
postgresem catalog scan > /tmp/catalog-function-after.json
if postgresem catalog diff \
  --from /tmp/catalog-view-after.json \
  --to /tmp/catalog-function-after.json \
  --fail-on-breaking >/tmp/catalog-function-diff.json 2>/dev/null
then
  echo "catalog diff missed executable function drift" >&2
  exit 1
fi
python3 - /tmp/catalog-function-diff.json <<'PY'
import json
import sys

with open(sys.argv[1], encoding="utf-8") as stream:
    diff = json.load(stream)
if not any(
    change["path"].startswith(
        "/functions/commerce/catalog_security_filter/"
    )
    and change["compatibility"] == "breaking"
    for change in diff["changes"]
):
    raise SystemExit("function definition drift was not classified as breaking")
PY
psql --no-psqlrc -v ON_ERROR_STOP=1 <<'SQL'
DROP VIEW commerce.catalog_security_view;
DROP AGGREGATE commerce.catalog_security_total(bigint);
DROP FUNCTION commerce.catalog_security_add(bigint, bigint);
DROP FUNCTION commerce.catalog_security_filter(bigint);
DROP SEQUENCE commerce.catalog_empty_acl_sequence;
SQL

export POSTGRESEM_DB_ROLE=postgresem_analyst
if DATABASE_URL="${DATABASE_URL% sslmode=disable}" \
  postgresem query execute "${TEST_ROOT}/queries/commerce-revenue.json" \
  --project commerce >/dev/null 2>&1
then
  echo "execution accepted a connection without an explicit sslmode" >&2
  exit 1
fi
if DATABASE_URL="${DATABASE_URL%sslmode=disable}sslmode=require" \
  postgresem query execute "${TEST_ROOT}/queries/commerce-revenue.json" \
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
  postgresem query execute "${TEST_ROOT}/queries/monthly-revenue.json" --project commerce
)
printf '%s\n' "$monthly_revenue" | grep -q '"semantic_revision": "sha256:'
psql --no-psqlrc -v ON_ERROR_STOP=1 <<'SQL'
ALTER ROLE postgresem_runtime RESET search_path;
DROP SCHEMA postgresem_attacker CASCADE;
SQL

commerce=$(
  postgresem query execute "${TEST_ROOT}/queries/commerce-revenue.json" --project commerce
)
printf '%s\n' "$commerce" | grep -q '"semantic_revision": "sha256:'
printf '%s\n' "$commerce" | grep -q '"200.50"'
printf '%s\n' "$commerce" | grep -q '"truncated": false'

fanout=$(
  postgresem query execute "${TEST_ROOT}/queries/fanout-revenue-by-sku.json" \
    --project commerce
)
priority_fanout=$(
  postgresem query execute \
    "${TEST_ROOT}/queries/fanout-priority-revenue-by-sku.json" \
    --project commerce
)
python3 - "$fanout" "$priority_fanout" <<'PY'
import json
import sys

fanout = json.loads(sys.argv[1])
priority = json.loads(sys.argv[2])
if fanout["rows"] != [
    ["SKU-BLUE", "120.00", 1],
    ["SKU-GREEN", "80.50", 1],
    ["SKU-RED", "200.50", 3],
    [None, None, 1],
]:
    raise SystemExit(f"fan-out-safe result mismatch: {fanout['rows']!r}")
if priority["rows"] != [
    ["SKU-BLUE", "120.00"],
    ["SKU-RED", "120.00"],
]:
    raise SystemExit(f"multi-branch fan-out result mismatch: {priority['rows']!r}")
anchors = fanout["lineage"].get("aggregation_anchors", {})
if anchors != [
    {"metric": "revenue", "field": "order_id"},
    {"metric": "order_count", "field": "order_id"},
]:
    raise SystemExit(f"aggregation anchor lineage mismatch: {anchors!r}")
PY

typed_order=$(
  postgresem query execute "${TEST_ROOT}/queries/typed-order.json" --project commerce
)
printf '%s\n' "$typed_order" | grep -q '1,'
printf '%s\n' "$typed_order" | grep -q '"2026-01-15T10:00:00+00:00"'
printf '%s\n' "$typed_order" | grep -q '"120.00"'

typed_subscription=$(
  postgresem query execute "${TEST_ROOT}/queries/typed-subscription.json" --project commerce
)
printf '%s\n' "$typed_subscription" | grep -q '101,'
printf '%s\n' "$typed_subscription" | grep -q '"2026-01-01"'
printf '%s\n' "$typed_subscription" | grep -q 'true'

export POSTGRESEM_DB_ROLE=postgresem_tenant_a
tenant_a=$(
  postgresem query execute "${TEST_ROOT}/queries/tenant-revenue.json" --project commerce
)
printf '%s\n' "$tenant_a" | grep -q '"250.00"'
if printf '%s\n' "$tenant_a" | grep -q '"999.00"'; then
  echo "tenant A execution leaked tenant B rows" >&2
  exit 1
fi
tenant_a_fanout=$(
  postgresem query execute "${TEST_ROOT}/queries/tenant-revenue-by-sku.json" \
    --project commerce
)
python3 - "$tenant_a_fanout" <<'PY'
import json
import sys

result = json.loads(sys.argv[1])
if result["rows"] != [["SKU-BLUE", "150.00", 1], ["SKU-RED", "100.00", 1]]:
    raise SystemExit(f"tenant A fan-out RLS mismatch: {result['rows']!r}")
PY

export POSTGRESEM_DB_ROLE=postgresem_tenant_b
tenant_b=$(
  postgresem query execute "${TEST_ROOT}/queries/tenant-revenue.json" --project commerce
)
printf '%s\n' "$tenant_b" | grep -q '"999.00"'
if printf '%s\n' "$tenant_b" | grep -q '"250.00"'; then
  echo "tenant B execution leaked tenant A rows" >&2
  exit 1
fi
tenant_b_fanout=$(
  postgresem query execute "${TEST_ROOT}/queries/tenant-revenue-by-sku.json" \
    --project commerce
)
python3 - "$tenant_b_fanout" <<'PY'
import json
import sys

result = json.loads(sys.argv[1])
if result["rows"] != [["SKU-RED", "999.00", 1]]:
    raise SystemExit(f"tenant B fan-out RLS mismatch: {result['rows']!r}")
PY

for unsafe_role in \
  postgresem_source_owner \
  postgresem_test_superuser \
  postgresem_test_bypassrls
do
  export POSTGRESEM_DB_ROLE=$unsafe_role
  if postgresem query execute "${TEST_ROOT}/queries/commerce-revenue.json" \
    --project commerce >/dev/null 2>&1
  then
    echo "unsafe mapped role was accepted: $unsafe_role" >&2
    exit 1
  fi
done

export POSTGRESEM_DB_ROLE=postgresem_analyst
psql --no-psqlrc -v ON_ERROR_STOP=1 \
  -c "GRANT postgresem_test_bypassrls TO postgresem_analyst"
if postgresem query execute "${TEST_ROOT}/queries/commerce-revenue.json" \
  --project commerce >/dev/null 2>&1
then
  echo "mapped query role inherited a BYPASSRLS role" >&2
  exit 1
fi
psql --no-psqlrc -v ON_ERROR_STOP=1 \
  -c "REVOKE postgresem_test_bypassrls FROM postgresem_analyst"

psql --no-psqlrc -v ON_ERROR_STOP=1 \
  -c "GRANT postgresem_source_owner TO postgresem_analyst"
if postgresem query execute "${TEST_ROOT}/queries/commerce-revenue.json" \
  --project commerce >/dev/null 2>&1
then
  echo "mapped query role inherited a source relation owner" >&2
  exit 1
fi
psql --no-psqlrc -v ON_ERROR_STOP=1 \
  -c "REVOKE postgresem_source_owner FROM postgresem_analyst"

export POSTGRESEM_MAX_RESULT_BYTES=2
limited=$(
  postgresem query execute "${TEST_ROOT}/queries/order-statuses.json" --project commerce
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
  IF (SELECT count(*) FROM semantic.query_audit) <> 16 THEN
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
  ) <> 11 THEN
    RAISE EXCEPTION 'unexpected successful guarded execution audit count';
  END IF;
  IF (
    SELECT count(*)
    FROM semantic.query_audit
    WHERE status = 'failed'
  ) <> 5 THEN
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
        'sha256:dc6fe2f9a25e995dc1bf8a8d156ea245e05e2a9232b2613d9e960dd63b11150f'
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
if postgresem query execute "${TEST_ROOT}/queries/commerce-revenue.json" \
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
  postgresem query execute "${TEST_ROOT}/queries/commerce-revenue.json" \
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
