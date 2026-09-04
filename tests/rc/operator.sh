#!/bin/sh
set -eu

test_root=${POSTGRESEM_TEST_ROOT:-/tests}
runtime_url="host=${PGHOST} port=${PGPORT} dbname=${PGDATABASE} user=postgresem_runtime password=${POSTGRESEM_RUNTIME_PASSWORD} sslmode=disable"
audit_url="host=${PGHOST} port=${PGPORT} dbname=${PGDATABASE} user=postgresem_audit_writer password=${POSTGRESEM_AUDIT_WRITER_PASSWORD} sslmode=disable"
mutation_url="host=${PGHOST} port=${PGPORT} dbname=${PGDATABASE} user=postgresem_mutation_runtime password=${POSTGRESEM_MUTATION_RUNTIME_PASSWORD} sslmode=disable"
export DATABASE_URL=$runtime_url
export POSTGRESEM_AUDIT_DATABASE_URL=$audit_url
export POSTGRESEM_MUTATION_DATABASE_URL=$mutation_url

cleanup() {
  psql --no-psqlrc -v ON_ERROR_STOP=1 <<'SQL' >/dev/null
DELETE FROM commerce.orders WHERE external_id = 'integration-order-1';
TRUNCATE semantic.mutation_audit, semantic.mutation_idempotency;
SQL
}
trap cleanup 0
cleanup

postgresem contract show >/tmp/rc-contract.json
python3 - /tmp/rc-contract.json <<'PY'
import json
import sys

with open(sys.argv[1], encoding="utf-8") as stream:
    contract = json.load(stream)

assert contract["release"] == "1.0.0"
assert contract["contract_status"] == "stable"
assert contract["contracts"]["lsq"] == ["1"]
assert contract["contracts"]["lsm"] == ["1"]
assert contract["contracts"]["database_migrations"]["current"] == (
    "0010_m10_operational_report"
)
PY

POSTGRESEM_DB_ROLE=postgresem_analyst \
postgresem query execute \
  "$test_root/integration/queries/commerce-revenue.json" \
  --project commerce >/tmp/rc-query.json
grep -q '"truncated": false' /tmp/rc-query.json

POSTGRESEM_MUTATION_DB_ROLE=postgresem_order_writer \
postgresem mutation execute \
  "$test_root/integration/mutations/order-insert.json" \
  --project commerce >/tmp/rc-mutation-first.json
grep -q '"affected_rows": 1' /tmp/rc-mutation-first.json
grep -q '"replayed": false' /tmp/rc-mutation-first.json

POSTGRESEM_MUTATION_DB_ROLE=postgresem_order_writer \
postgresem mutation execute \
  "$test_root/integration/mutations/order-insert.json" \
  --project commerce >/tmp/rc-mutation-replay.json
grep -q '"affected_rows": 1' /tmp/rc-mutation-replay.json
grep -q '"replayed": true' /tmp/rc-mutation-replay.json

postgresem report operations --window-hours 1 >/tmp/rc-operations.json
python3 - /tmp/rc-operations.json <<'PY'
import json
import sys

with open(sys.argv[1], encoding="utf-8") as stream:
    report = json.load(stream)

assert report["objectives"]["query_audit_complete"]
assert report["objectives"]["mutation_audit_complete"]
assert report["queries"]["succeeded"] >= 1
assert report["mutations"]["committed"] >= 1
PY

echo "M11 release-candidate query and ingestion workflow passed"
