#!/bin/sh
set -eu

source_database=$PGDATABASE
test_database=postgresem_anchor_concurrency_test
field_race_output=/tmp/postgresem-anchor-field-race.out
publish_race_output=/tmp/postgresem-anchor-publish-race.out
lock_order_output=/tmp/postgresem-anchor-lock-order.out

wait_for_sleep() {
  application_name=$1
  attempt=0
  while [ "$attempt" -lt 50 ]; do
    sleeping=$(
      psql --no-psqlrc --tuples-only --no-align -v ON_ERROR_STOP=1 \
        -v application_name="$application_name" <<'SQL'
SELECT count(*)
FROM pg_catalog.pg_stat_activity
WHERE application_name = :'application_name'
  AND wait_event = 'PgSleep';
SQL
    )
    if [ "$sleeping" = "1" ]; then
      return
    fi
    attempt=$((attempt + 1))
    sleep 0.1
  done
  echo "concurrent test session did not reach its synchronization point" >&2
  exit 1
}

cleanup() {
  PGDATABASE=postgres dropdb --if-exists --force "$test_database" >/dev/null
  rm -f "$field_race_output" "$publish_race_output" "$lock_order_output"
}
trap cleanup 0

PGDATABASE=postgres dropdb --if-exists --force "$test_database" >/dev/null
PGDATABASE=postgres createdb "$test_database"
export PGDATABASE=$test_database
sh /migrations/run.sh >/dev/null

psql --no-psqlrc -v ON_ERROR_STOP=1 <<'SQL'
BEGIN;
INSERT INTO semantic.project (project_id, semantic_name, display_name)
VALUES
  ('00000000-0000-0000-0000-000000000021', 'anchor_field_race', 'Anchor field race'),
  ('00000000-0000-0000-0000-000000000031', 'anchor_publish_race', 'Anchor publish race'),
  ('00000000-0000-0000-0000-000000000041', 'anchor_lock_order', 'Anchor lock order');

INSERT INTO semantic.revision (
  revision_id, project_id, revision_number, status, schema_version,
  canonical_hash, compiler_semantic_version
)
VALUES
  (
    '00000000-0000-0000-0000-000000000022',
    '00000000-0000-0000-0000-000000000021',
    1, 'draft', '2',
    'sha256:cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc',
    '0.2.0'
  ),
  (
    '00000000-0000-0000-0000-000000000032',
    '00000000-0000-0000-0000-000000000031',
    1, 'draft', '2',
    'sha256:dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd',
    '0.2.0'
  ),
  (
    '00000000-0000-0000-0000-000000000042',
    '00000000-0000-0000-0000-000000000041',
    1, 'draft', '2',
    'sha256:ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff',
    '0.2.0'
  );

INSERT INTO semantic.model (
  model_id, revision_id, semantic_name, display_name, model_kind,
  source_database, source_schema, source_relation, source_relation_kind
)
VALUES
  (
    '00000000-0000-0000-0000-000000000023',
    '00000000-0000-0000-0000-000000000022',
    'field_race_orders', 'Field race orders', 'fact',
    current_database(), 'commerce', 'orders', 'table'
  ),
  (
    '00000000-0000-0000-0000-000000000033',
    '00000000-0000-0000-0000-000000000032',
    'publish_race_orders', 'Publish race orders', 'fact',
    current_database(), 'commerce', 'orders', 'table'
  ),
  (
    '00000000-0000-0000-0000-000000000043',
    '00000000-0000-0000-0000-000000000042',
    'lock_order_orders', 'Lock order orders', 'fact',
    current_database(), 'commerce', 'orders', 'table'
  );

INSERT INTO semantic.field (
  field_id, revision_id, model_id, semantic_name, display_name, field_kind,
  logical_type, source_column, nullable
)
VALUES
  (
    '00000000-0000-0000-0000-000000000024',
    '00000000-0000-0000-0000-000000000022',
    '00000000-0000-0000-0000-000000000023',
    'order_id', 'Order ID', 'entity_key', 'integer', 'order_id', false
  ),
  (
    '00000000-0000-0000-0000-000000000025',
    '00000000-0000-0000-0000-000000000022',
    '00000000-0000-0000-0000-000000000023',
    'amount', 'Amount', 'dimension', 'numeric', 'amount', false
  ),
  (
    '00000000-0000-0000-0000-000000000034',
    '00000000-0000-0000-0000-000000000032',
    '00000000-0000-0000-0000-000000000033',
    'order_id', 'Order ID', 'entity_key', 'integer', 'order_id', false
  ),
  (
    '00000000-0000-0000-0000-000000000035',
    '00000000-0000-0000-0000-000000000032',
    '00000000-0000-0000-0000-000000000033',
    'amount', 'Amount', 'dimension', 'numeric', 'amount', false
  ),
  (
    '00000000-0000-0000-0000-000000000044',
    '00000000-0000-0000-0000-000000000042',
    '00000000-0000-0000-0000-000000000043',
    'order_id', 'Order ID', 'entity_key', 'integer', 'order_id', false
  ),
  (
    '00000000-0000-0000-0000-000000000045',
    '00000000-0000-0000-0000-000000000042',
    '00000000-0000-0000-0000-000000000043',
    'amount', 'Amount', 'dimension', 'numeric', 'amount', false
  );
COMMIT;
SQL

PGAPPNAME=postgresem-anchor-field-race \
  psql --no-psqlrc -v ON_ERROR_STOP=1 >"$field_race_output" 2>&1 <<'SQL' &
BEGIN;
UPDATE semantic.field
SET field_kind = 'dimension'
WHERE field_id = '00000000-0000-0000-0000-000000000024';
SELECT pg_catalog.pg_sleep(2);
COMMIT;
SQL
field_race_pid=$!
wait_for_sleep postgresem-anchor-field-race

if psql --no-psqlrc -v ON_ERROR_STOP=1 >/dev/null 2>&1 <<'SQL'
INSERT INTO semantic.metric (
  metric_id, revision_id, model_id, semantic_name, display_name, result_type,
  expression, additivity, aggregation_anchor_field_id
)
VALUES (
  '00000000-0000-0000-0000-000000000026',
  '00000000-0000-0000-0000-000000000022',
  '00000000-0000-0000-0000-000000000023',
  'field_race_revenue', 'Field race revenue', 'numeric',
  '{"version":"1","kind":"aggregation","aggregation":"sum","field":"amount"}',
  'additive',
  '00000000-0000-0000-0000-000000000024'
);
SQL
then
  echo "concurrent field mutation admitted an invalid aggregation anchor" >&2
  exit 1
fi
wait "$field_race_pid"

PGAPPNAME=postgresem-anchor-publish-race \
  psql --no-psqlrc -v ON_ERROR_STOP=1 >"$publish_race_output" 2>&1 <<'SQL' &
BEGIN;
UPDATE semantic.revision
SET status = 'published', published_at = clock_timestamp()
WHERE revision_id = '00000000-0000-0000-0000-000000000032';
SELECT pg_catalog.pg_sleep(2);
COMMIT;
SQL
publish_race_pid=$!
wait_for_sleep postgresem-anchor-publish-race

if psql --no-psqlrc -v ON_ERROR_STOP=1 >/dev/null 2>&1 <<'SQL'
INSERT INTO semantic.metric (
  metric_id, revision_id, model_id, semantic_name, display_name, result_type,
  expression, additivity, aggregation_anchor_field_id
)
VALUES (
  '00000000-0000-0000-0000-000000000036',
  '00000000-0000-0000-0000-000000000032',
  '00000000-0000-0000-0000-000000000033',
  'publish_race_revenue', 'Publish race revenue', 'numeric',
  '{"version":"1","kind":"aggregation","aggregation":"sum","field":"amount"}',
  'additive',
  '00000000-0000-0000-0000-000000000034'
);
SQL
then
  echo "concurrent publication admitted a post-publication metric" >&2
  exit 1
fi
wait "$publish_race_pid"

PGAPPNAME=postgresem-anchor-lock-order \
  psql --no-psqlrc -v ON_ERROR_STOP=1 >"$lock_order_output" 2>&1 <<'SQL' &
BEGIN;
UPDATE semantic.field
SET display_name = 'Amount being authored'
WHERE field_id = '00000000-0000-0000-0000-000000000045';
SELECT pg_catalog.pg_sleep(2);
INSERT INTO semantic.metric (
  metric_id, revision_id, model_id, semantic_name, display_name, result_type,
  expression, additivity, aggregation_anchor_field_id
)
VALUES (
  '00000000-0000-0000-0000-000000000046',
  '00000000-0000-0000-0000-000000000042',
  '00000000-0000-0000-0000-000000000043',
  'lock_order_revenue', 'Lock order revenue', 'numeric',
  '{"version":"1","kind":"aggregation","aggregation":"sum","field":"amount"}',
  'additive',
  '00000000-0000-0000-0000-000000000044'
);
COMMIT;
SQL
lock_order_pid=$!
wait_for_sleep postgresem-anchor-lock-order

psql --no-psqlrc -v ON_ERROR_STOP=1 >/dev/null <<'SQL'
UPDATE semantic.field
SET field_kind = 'dimension'
WHERE field_id = '00000000-0000-0000-0000-000000000044';
SQL

if wait "$lock_order_pid"; then
  echo "inverse lock order admitted an invalid aggregation anchor" >&2
  exit 1
fi
if grep -q 'deadlock detected' "$lock_order_output"; then
  cat "$lock_order_output" >&2
  echo "inverse anchor lock order deadlocked" >&2
  exit 1
fi
if ! grep -Eq \
  'metric aggregation anchor must be a direct entity key|metric_aggregation_anchor_model_fkey' \
  "$lock_order_output"
then
  cat "$lock_order_output" >&2
  echo "inverse lock order did not reject the invalid aggregation anchor" >&2
  exit 1
fi

export PGDATABASE=$source_database
echo "aggregation anchor concurrency checks passed"
