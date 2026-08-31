#!/bin/sh
set -eu

psql --no-psqlrc -v ON_ERROR_STOP=1 <<'SQL'
CREATE SCHEMA IF NOT EXISTS semantic AUTHORIZATION postgresem_owner;
CREATE TABLE IF NOT EXISTS semantic.schema_migration (
  version text PRIMARY KEY,
  applied_at timestamptz NOT NULL DEFAULT clock_timestamp()
);
ALTER TABLE semantic.schema_migration OWNER TO postgresem_owner;
SQL

for migration in /migrations/[0-9][0-9][0-9][0-9]_*.sql; do
  version=$(basename "$migration" .sql)
  applied=$(
    psql --no-psqlrc --tuples-only --no-align \
      -v ON_ERROR_STOP=1 \
      -v version="$version" <<'SQL'
SELECT 1
FROM semantic.schema_migration
WHERE version = :'version';
SQL
  )

  if [ "$applied" = "1" ]; then
    echo "migration $version already applied"
    continue
  fi

  psql --no-psqlrc -v ON_ERROR_STOP=1 -f "$migration"
done
