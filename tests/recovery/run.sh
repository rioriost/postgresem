#!/bin/sh
set -eu

source_database=$PGDATABASE
n_minus_one_database=postgresem_n_minus_one_test
held_database=postgresem_restore_source_hold
dump_path=/tmp/postgresem-recovery.dump
expected_revision=sha256:806f8687c1e2161f65370e0c433832760c02b6f96f8b8bc6e93fde6295d29da6
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
  export DATABASE_URL="host=$PGHOST port=$PGPORT dbname=$database user=postgresem_runtime password=$POSTGRESEM_RUNTIME_PASSWORD"
  export POSTGRESEM_AUDIT_DATABASE_URL="host=$PGHOST port=$PGPORT dbname=$database user=postgresem_audit_writer password=$POSTGRESEM_AUDIT_WRITER_PASSWORD"
  export POSTGRESEM_DB_ROLE=postgresem_analyst
}

verify_revision() {
  database=$1
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

restore_original_database() {
  if [ "$source_held" = true ]; then
    drop_database "$source_database"
    PGDATABASE=postgres psql --no-psqlrc -v ON_ERROR_STOP=1 \
      -c "ALTER DATABASE $held_database RENAME TO $source_database"
    source_held=false
  fi
}

trap 'drop_database "$n_minus_one_database"; restore_original_database; drop_database "$held_database"; rm -f "$dump_path"' 0

drop_database "$n_minus_one_database"
create_database "$n_minus_one_database"
export PGDATABASE=$n_minus_one_database
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
POSTGRESEM_MIGRATION_MAX_VERSION=0003_guarded_execution_audit \
  sh /migrations/run.sh
psql --no-psqlrc -v ON_ERROR_STOP=1 -f /fixtures/semantic/commerce.sql

if psql --no-psqlrc --tuples-only --no-align -v ON_ERROR_STOP=1 \
  -c "SELECT to_regprocedure('semantic.beta_operational_report(timestamptz)')" |
  grep -q beta_operational_report
then
  echo "N-1 database unexpectedly contains the current reporting function" >&2
  exit 1
fi

verify_revision "$n_minus_one_database"
run_guarded_query "$n_minus_one_database"

unset POSTGRESEM_MIGRATION_MAX_VERSION
sh /migrations/run.sh
migration_count=$(
  psql --no-psqlrc --tuples-only --no-align -v ON_ERROR_STOP=1 \
    -c 'SELECT count(*) FROM semantic.schema_migration'
)
if [ "$migration_count" != "4" ]; then
  echo "N-1 upgrade did not apply the complete migration set" >&2
  exit 1
fi
postgresem report beta --window-hours 1 |
  grep -q '"audit_complete": true'
postgresem report beta --window-hours 1 |
  grep -q '"active_principals": null'
if PGUSER=postgresem_audit_writer \
  PGPASSWORD="$POSTGRESEM_AUDIT_WRITER_PASSWORD" \
  psql --no-psqlrc -v ON_ERROR_STOP=1 \
    -c "SELECT semantic.beta_operational_report(clock_timestamp() - interval '366 days')" \
    >/dev/null 2>&1
then
  echo "database report accepted an unbounded history window" >&2
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
verify_revision "$source_database"
run_guarded_query "$source_database"
postgresem report beta --window-hours 1 |
  grep -q '"validation_compile_p95_under_50_ms"'
restore_original_database

echo "N-1 migration and backup/restore recovery checks passed"
