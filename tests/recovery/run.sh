#!/bin/sh
set -eu

source_database=$PGDATABASE
n_minus_one_database=postgresem_n_minus_one_test
legacy_authority_database=postgresem_legacy_authority_test
legacy_v1_database=postgresem_v1_upgrade_test
held_database=postgresem_restore_source_hold
dump_path=/tmp/postgresem-recovery.dump
legacy_v1_revision=sha256:a731347152caed2f8f3dfcecb730aac12c93c839f8cc91e6f81099128f70e58c
current_revision=sha256:dc6fe2f9a25e995dc1bf8a8d156ea245e05e2a9232b2613d9e960dd63b11150f
source_held=false

if [ "$source_database" != postgresem_dev ]; then
  echo "recovery test requires the isolated postgresem_dev database" >&2
  exit 1
fi

drop_database() {
  database=$1
  PGDATABASE=postgres dropdb --if-exists --force "$database"
}

create_database() {
  database=$1
  PGDATABASE=postgres createdb "$database"
}

configure_gateway_urls() {
  database=$1
  export DATABASE_URL="host=$PGHOST port=$PGPORT dbname=$database user=postgresem_runtime password=$POSTGRESEM_RUNTIME_PASSWORD sslmode=disable"
  export POSTGRESEM_AUDIT_DATABASE_URL="host=$PGHOST port=$PGPORT dbname=$database user=postgresem_audit_writer password=$POSTGRESEM_AUDIT_WRITER_PASSWORD sslmode=disable"
  export POSTGRESEM_DB_ROLE=postgresem_analyst
}

configure_mutation_urls() {
  database=$1
  export POSTGRESEM_MUTATION_DATABASE_URL="host=$PGHOST port=$PGPORT dbname=$database user=postgresem_mutation_runtime password=$POSTGRESEM_MUTATION_RUNTIME_PASSWORD sslmode=disable"
  export POSTGRESEM_AUDIT_DATABASE_URL="host=$PGHOST port=$PGPORT dbname=$database user=postgresem_audit_writer password=$POSTGRESEM_AUDIT_WRITER_PASSWORD sslmode=disable"
  export POSTGRESEM_MUTATION_DB_ROLE=postgresem_order_writer
  export POSTGRESEM_MUTATION_AUTHORITY_ID=cli:local-mutation
}

verify_revision() {
  database=$1
  expected_revision=$2
  revision=$(
    PGDATABASE=$database psql --no-psqlrc --tuples-only --no-align \
      -v ON_ERROR_STOP=1 \
      -c "SELECT canonical_hash FROM semantic.revision WHERE status = 'published'"
  )
  if [ "$revision" != "$expected_revision" ]; then
    echo "published semantic revision changed in $database" >&2
    exit 1
  fi
}

run_guarded_query() {
  database=$1
  configure_gateway_urls "$database"
  result=$(
    postgresem query execute /tests/integration/queries/commerce-revenue.json \
      --project commerce
  )
  printf '%s\n' "$result" | grep -q '"200.50"'
  printf '%s\n' "$result" | grep -q '"truncated": false'
}

verify_scale_authoring() {
  database=$1
  PGDATABASE=$database python3 /tests/recovery/scale_authoring.py
}

restore_original_database() {
  if [ "$source_held" = true ]; then
    drop_database "$source_database"
    PGDATABASE=postgres psql --no-psqlrc -v ON_ERROR_STOP=1 \
      -c "ALTER DATABASE $held_database RENAME TO $source_database"
    source_held=false
  fi
}

trap 'drop_database "$n_minus_one_database"; drop_database "$legacy_authority_database"; drop_database "$legacy_v1_database"; restore_original_database; drop_database "$held_database"; rm -f "$dump_path"' 0

drop_database "$n_minus_one_database"
create_database "$n_minus_one_database"
export PGDATABASE=$n_minus_one_database
psql --no-psqlrc -v ON_ERROR_STOP=1 -f /fixtures/postgres/10-commerce.sql
psql --no-psqlrc -v ON_ERROR_STOP=1 -f /fixtures/postgres/20-rls-multitenant.sql
psql --no-psqlrc -v ON_ERROR_STOP=1 -f /fixtures/postgres/30-subscriptions.sql
POSTGRESEM_MIGRATION_MAX_VERSION=0008_mutation_authority_idempotency \
  sh /migrations/run.sh
psql --no-psqlrc -v ON_ERROR_STOP=1 -f /fixtures/semantic/commerce.sql

psql --no-psqlrc -v ON_ERROR_STOP=1 <<'SQL'
DO $$
DECLARE
  v_revision_id uuid;
  v_revision_hash text;
BEGIN
  SELECT revision_id, canonical_hash
  INTO STRICT v_revision_id, v_revision_hash
  FROM semantic.revision AS revision
  JOIN semantic.project AS project
    ON project.project_id = revision.project_id
  WHERE project.semantic_name = 'commerce'
    AND revision.status = 'published';

  PERFORM semantic.claim_mutation(
    'commerce',
    'sha256:' || repeat('3', 64),
    'sha256:' || repeat('4', 64),
    '1',
    'sha256:' || repeat('5', 64),
    v_revision_id,
    v_revision_hash,
    'sha256:' || repeat('6', 64),
    '0.1.0',
    'mcp-http',
    'insert',
    'tenant_orders',
    'sha256:' || repeat('7', 64),
    'sha256:' || repeat('8', 64),
    '[]'::jsonb,
    '{}'::jsonb,
    (
      '{"database_role":"postgresem_tenant_a_writer",'
      '"legacy_authority_hash":"sha256:' || repeat('9', 64) || '"}'
    )::jsonb,
    1,
    0,
    0
  );

  INSERT INTO semantic.mutation_idempotency (
    project, idempotency_key_hash, authority_hash, lsm_hash, revision_id,
    semantic_revision_hash, mutation_id, status, result, affected_rows,
    replay_count, created_at, committed_at, last_replayed_at, database_role,
    authority_scheme
  )
  SELECT
    project, idempotency_key_hash, 'sha256:' || repeat('9', 64), lsm_hash,
    revision_id, semantic_revision_hash, gen_random_uuid(), status, result,
    affected_rows, replay_count, created_at, committed_at, last_replayed_at,
    database_role, 'legacy-v1'
  FROM semantic.mutation_idempotency
  WHERE project = 'commerce'
    AND authority_hash = 'sha256:' || repeat('4', 64)
    AND idempotency_key_hash = 'sha256:' || repeat('3', 64);
END;
$$;
SQL

POSTGRESEM_MIGRATION_MAX_VERSION=0009_mutation_reconcile_precedence \
  sh /migrations/run.sh
if [ "$(
  psql --no-psqlrc --tuples-only --no-align -v ON_ERROR_STOP=1 \
    -c 'SELECT count(*) FROM semantic.schema_migration'
)" != "9" ]; then
  echo "N-1 upgrade did not apply migration 0009" >&2
  exit 1
fi
psql --no-psqlrc -v ON_ERROR_STOP=1 <<'SQL'
DO $$
DECLARE
  v_expected uuid;
  v_actual uuid;
BEGIN
  SELECT mutation_id
  INTO STRICT v_expected
  FROM semantic.mutation_idempotency
  WHERE project = 'commerce'
    AND authority_scheme = 'principal-v1'
    AND authority_hash = 'sha256:' || repeat('4', 64)
    AND idempotency_key_hash = 'sha256:' || repeat('3', 64);

  SELECT (
    semantic.lookup_mutation_idempotency(
      'commerce',
      'sha256:' || repeat('4', 64),
      'sha256:' || repeat('9', 64),
      'sha256:' || repeat('3', 64)
    ) ->> 'mutation_id'
  )::uuid
  INTO STRICT v_actual;

  IF v_actual <> v_expected THEN
    RAISE EXCEPTION 'principal-v1 reconciliation did not take precedence';
  END IF;
END;
$$;
SQL
verify_revision "$n_minus_one_database" "$current_revision"
run_guarded_query "$n_minus_one_database"
postgresem report beta --window-hours 1 |
  grep -q '"audit_complete": true'

unset POSTGRESEM_MIGRATION_MAX_VERSION
sh /migrations/run.sh
if [ "$(
  psql --no-psqlrc --tuples-only --no-align -v ON_ERROR_STOP=1 \
    -c 'SELECT count(*) FROM semantic.schema_migration'
)" != "10" ]; then
  echo "N-1 upgrade did not apply migration 0010" >&2
  exit 1
fi
postgresem report operations --window-hours 1 |
  grep -q '"current": "0010_m10_operational_report"'
postgresem report operations --window-hours 1 |
  grep -q '"query_audit_complete": true'
verify_revision "$n_minus_one_database" "$current_revision"
run_guarded_query "$n_minus_one_database"
verify_scale_authoring "$n_minus_one_database"

drop_database "$legacy_authority_database"
create_database "$legacy_authority_database"
export PGDATABASE=$legacy_authority_database
psql --no-psqlrc -v ON_ERROR_STOP=1 -f /fixtures/postgres/10-commerce.sql
psql --no-psqlrc -v ON_ERROR_STOP=1 -f /fixtures/postgres/20-rls-multitenant.sql
psql --no-psqlrc -v ON_ERROR_STOP=1 -f /fixtures/postgres/30-subscriptions.sql
if POSTGRESEM_MIGRATION_MAX_VERSION=0003_missing \
  sh /migrations/run.sh >/dev/null 2>&1
then
  echo "invalid migration ceiling was accepted" >&2
  exit 1
fi
if psql --no-psqlrc --tuples-only --no-align -v ON_ERROR_STOP=1 \
  -c "SELECT to_regnamespace('semantic')" |
  grep -q semantic
then
  echo "invalid migration ceiling changed the database" >&2
  exit 1
fi
POSTGRESEM_MIGRATION_MAX_VERSION=0007_fanout_anchor_invariants \
  sh /migrations/run.sh
psql --no-psqlrc -v ON_ERROR_STOP=1 -f /fixtures/semantic/commerce.sql

psql --no-psqlrc -v ON_ERROR_STOP=1 <<'SQL'
DO $$
DECLARE
  v_mutation_id uuid;
  v_attempt_id uuid;
  v_revision_id uuid;
  v_revision_hash text;
  v_principal_hash text :=
    'sha256:a4a49a30293115a968b7e784794fc2f78ce6421c6baa11d8701bfcf438f03504';
  v_authority_hash text :=
    'sha256:4063de5cad15a5b21303e73351eedc246758b6f5c9886b43842bf9aee0cd1112';
  v_key_hash text :=
    'sha256:6cc8c4880b70e61c9ab34ce345b1b782251ad69aabb13cc2a34c8ad411faee63';
BEGIN
  SELECT revision_id, canonical_hash
  INTO STRICT v_revision_id, v_revision_hash
  FROM semantic.revision AS revision
  JOIN semantic.project AS project
    ON project.project_id = revision.project_id
  WHERE project.semantic_name = 'commerce'
    AND revision.status = 'published';

  SELECT mutation_id, attempt_id
  INTO STRICT v_mutation_id, v_attempt_id
  FROM semantic.claim_mutation(
    'commerce',
    v_key_hash,
    v_authority_hash,
    '1',
    'sha256:' || repeat('0', 64),
    v_revision_id,
    v_revision_hash,
    v_principal_hash,
    '0.1.0',
    'cli',
    'insert',
    'Orders',
    'sha256:' || repeat('1', 64),
    'sha256:' || repeat('2', 64),
    '[]'::jsonb,
    '{}'::jsonb,
    '{"database_role":"postgresem_order_writer"}'::jsonb,
    0,
    0,
    0
  );

  PERFORM semantic.finish_mutation(
    v_mutation_id,
    v_attempt_id,
    '[]'::jsonb,
    0,
    0
  );
END;
$$;
SQL

if psql --no-psqlrc --tuples-only --no-align -v ON_ERROR_STOP=1 \
  -c "SELECT count(*) FROM pg_catalog.pg_proc WHERE proname = 'lookup_mutation_idempotency' AND pronargs = 4" |
  grep -q '^1$'
then
  echo "pre-0008 database unexpectedly contains authority-scoped idempotency" >&2
  exit 1
fi

verify_revision "$legacy_authority_database" "$current_revision"
run_guarded_query "$legacy_authority_database"

unset POSTGRESEM_MIGRATION_MAX_VERSION
sh /migrations/run.sh
migration_count=$(
  psql --no-psqlrc --tuples-only --no-align -v ON_ERROR_STOP=1 \
    -c 'SELECT count(*) FROM semantic.schema_migration'
)
if [ "$migration_count" != "10" ]; then
  echo "pre-0008 upgrade did not apply the complete migration set" >&2
  exit 1
fi
postgresem report beta --window-hours 1 |
  grep -q '"audit_complete": true'
postgresem report beta --window-hours 1 |
  grep -q '"active_principals": null'
postgresem report operations --window-hours 1 |
  grep -q '"current": "0010_m10_operational_report"'
verify_revision "$legacy_authority_database" "$current_revision"
configure_mutation_urls "$legacy_authority_database"
export POSTGRESEM_IDEMPOTENCY_KEY=recovery-legacy-idempotency
legacy_reconciled=$(postgresem mutation reconcile --project commerce)
if ! printf '%s\n' "$legacy_reconciled" | grep -q '"status": "committed"'; then
  echo "pre-0008 mutation state was not reconciled after upgrade" >&2
  printf '%s\n' "$legacy_reconciled" >&2
  exit 1
fi
TEST_ROOT=/tests/integration sh /tests/integration/mutation.sh

drop_database "$legacy_v1_database"
create_database "$legacy_v1_database"
export PGDATABASE=$legacy_v1_database
psql --no-psqlrc -v ON_ERROR_STOP=1 -f /fixtures/postgres/10-commerce.sql
psql --no-psqlrc -v ON_ERROR_STOP=1 -f /fixtures/postgres/20-rls-multitenant.sql
psql --no-psqlrc -v ON_ERROR_STOP=1 -f /fixtures/postgres/30-subscriptions.sql
POSTGRESEM_MIGRATION_MAX_VERSION=0005_governed_mutation \
  sh /migrations/run.sh
psql --no-psqlrc -v ON_ERROR_STOP=1 -f /fixtures/semantic/commerce.sql
verify_revision "$legacy_v1_database" "$legacy_v1_revision"
run_guarded_query "$legacy_v1_database"
unset POSTGRESEM_MIGRATION_MAX_VERSION
sh /migrations/run.sh
verify_revision "$legacy_v1_database" "$legacy_v1_revision"
postgresem report operations --window-hours 1 |
  grep -q '"current": "0010_m10_operational_report"'

export PGDATABASE=$n_minus_one_database
configure_gateway_urls "$n_minus_one_database"

incomplete_query_id=$(
  PGUSER=postgresem_audit_writer \
    PGPASSWORD="$POSTGRESEM_AUDIT_WRITER_PASSWORD" \
    psql --no-psqlrc --tuples-only --no-align -v ON_ERROR_STOP=1 <<SQL
SELECT semantic.start_query_audit(
  'sha256:recovery-test-principal',
  '1',
  'sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',
  '10000000-0000-0000-0000-000000000002',
  '$current_revision',
  '0.2.0',
  'recovery-test',
  'sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb',
  'sha256:cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc',
  '[]'::jsonb,
  '{}'::jsonb,
  '{}'::jsonb,
  0,
  0
);
SQL
)
postgresem report beta --window-hours 1 |
  grep -q '"incomplete": 1'
postgresem report beta --window-hours 1 |
  grep -q '"audit_complete": false'
postgresem report operations --window-hours 1 |
  grep -q '"query_audit_complete": false'

PGUSER=postgresem_audit_writer \
  PGPASSWORD="$POSTGRESEM_AUDIT_WRITER_PASSWORD" \
  psql --no-psqlrc -v ON_ERROR_STOP=1 \
  -v query_id="$incomplete_query_id" <<'SQL'
SELECT semantic.finish_query_audit(
  :'query_id'::uuid,
  'failed',
  'RECOVERY_CONFIRMED_PROCESS_TERMINATION',
  0,
  0,
  0,
  0,
  false
);
SQL
postgresem report beta --window-hours 1 |
  grep -q '"incomplete": 0'
postgresem report beta --window-hours 1 |
  grep -q '"audit_complete": true'
postgresem report operations --window-hours 1 |
  grep -q '"query_audit_complete": true'

if PGUSER=postgresem_audit_writer \
  PGPASSWORD="$POSTGRESEM_AUDIT_WRITER_PASSWORD" \
  psql --no-psqlrc -v ON_ERROR_STOP=1 \
    -c "SELECT semantic.m10_operational_report(clock_timestamp() - interval '366 days')" \
    >/dev/null 2>&1
then
  echo "M10 operational report accepted an unbounded history window" >&2
  exit 1
fi

export PGDATABASE="$source_database"
rm -f "$dump_path"
pg_dump --format=custom --file="$dump_path" "$source_database"
drop_database "$held_database"
PGDATABASE=postgres psql --no-psqlrc -v ON_ERROR_STOP=1 \
  -c "ALTER DATABASE $source_database RENAME TO $held_database"
source_held=true
create_database "$source_database"
pg_restore --exit-on-error --dbname="$source_database" "$dump_path"

export PGDATABASE="$source_database"
sh /migrations/run.sh
verify_revision "$source_database" "$current_revision"
run_guarded_query "$source_database"
postgresem report beta --window-hours 1 |
  grep -q '"validation_compile_p95_under_50_ms"'
postgresem report operations --window-hours 1 |
  grep -q '"current": "0010_m10_operational_report"'
restore_original_database

echo "M10 N-1 migration, scale authoring, and backup/restore recovery checks passed"
