#!/bin/sh
set -eu

maximum_version=${POSTGRESEM_MIGRATION_MAX_VERSION:-}
maximum_reached=false

if [ -n "$maximum_version" ]; then
  case "$maximum_version" in
    [0-9][0-9][0-9][0-9]_[A-Za-z0-9_]*)
      ;;
    *)
      echo "maximum migration version is invalid: $maximum_version" >&2
      exit 1
      ;;
  esac
  if [ ! -f "/migrations/$maximum_version.sql" ]; then
    echo "maximum migration was not found: $maximum_version" >&2
    exit 1
  fi
fi

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
  else
    psql --no-psqlrc -v ON_ERROR_STOP=1 -f "$migration"
  fi

  if [ -n "$maximum_version" ] && [ "$version" = "$maximum_version" ]; then
    maximum_reached=true
    break
  fi
done

if [ -n "$maximum_version" ] && [ "$maximum_reached" != true ]; then
  echo "maximum migration was not reached: $maximum_version" >&2
  exit 1
fi
