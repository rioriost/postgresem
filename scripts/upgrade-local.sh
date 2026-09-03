#!/bin/sh
set -eu

backup_dir=${1:?usage: scripts/upgrade-local.sh BACKUP_DIRECTORY}
database_container=${POSTGRESEM_DATABASE_CONTAINER:-postgresem-db}
database=postgresem_dev
environment_file=${POSTGRESEM_ENV_FILE:-.env}
case "$environment_file" in
  /*) ;;
  *) environment_file=./$environment_file ;;
esac
before_revisions=
after_revisions=
canary_image=postgresem-local-upgrade-canary

cleanup() {
  rm -f "$before_revisions" "$after_revisions"
  container image delete "$canary_image" >/dev/null 2>&1 || true
}

container_psql() {
  container exec --user postgres "$database_container" \
    psql --no-psqlrc --tuples-only --no-align \
      --set ON_ERROR_STOP=1 --dbname="$database" "$@"
}

build_conninfo() {
  CONNINFO_HOST=$database_ip \
  CONNINFO_DATABASE=$database \
  CONNINFO_USER=$1 \
  CONNINFO_SECRET=$2 \
    python3 -c '
import os

secret = os.environ["CONNINFO_SECRET"]
quoted = "'"'"'" + secret.replace("\\", "\\\\").replace("'"'"'", "\\'"'"'") + "'"'"'"
print(
    " ".join(
        [
            "host=" + os.environ["CONNINFO_HOST"],
            "port=5432",
            "dbname=" + os.environ["CONNINFO_DATABASE"],
            "user=" + os.environ["CONNINFO_USER"],
            "password=" + quoted,
            "sslmode=disable",
        ]
    )
)
'
}

command -v container >/dev/null 2>&1 || {
  echo "Apple Container CLI is required" >&2
  exit 1
}
command -v python3 >/dev/null 2>&1 || {
  echo "Python 3 is required" >&2
  exit 1
}
if [ ! -f "$environment_file" ]; then
  echo "local upgrade environment file is missing: $environment_file" >&2
  exit 1
fi

scripts/verify-backup.sh "$backup_dir"

current_migrations=$(
  container_psql \
    --command='SELECT version FROM semantic.schema_migration ORDER BY version'
)
if [ "$current_migrations" != "$(cat "$backup_dir/migrations.txt")" ]; then
  echo "verified backup does not match the current pre-upgrade migrations" >&2
  exit 1
fi

before_revisions=$(mktemp)
after_revisions=$(mktemp)
trap cleanup 0
container_psql \
  --command="SELECT canonical_hash FROM semantic.revision WHERE status = 'published' ORDER BY published_at" \
  >"$before_revisions"
if ! cmp -s "$before_revisions" "$backup_dir/published-revisions.txt"; then
  echo "verified backup does not match the current published revisions" >&2
  exit 1
fi

incomplete=$(
  container_psql --command="
    SELECT
      (SELECT count(*) FROM semantic.query_audit WHERE status = 'started')
      +
      (SELECT count(*) FROM semantic.mutation_audit WHERE status = 'started')"
)
if [ "$incomplete" != "0" ]; then
  echo "upgrade blocked: incomplete query or mutation audit rows remain" >&2
  exit 1
fi

for migration in migrations/[0-9][0-9][0-9][0-9]_*.sql; do
  version=$(basename "$migration" .sql)
  applied=$(
    container_psql \
      --command="SELECT 1 FROM semantic.schema_migration WHERE version = '$version'"
  )
  if [ "$applied" != "1" ]; then
    container exec --interactive --user postgres "$database_container" \
      psql --no-psqlrc --set ON_ERROR_STOP=1 --dbname="$database" \
      <"$migration"
  fi
done

container_psql \
  --command="SELECT canonical_hash FROM semantic.revision WHERE status = 'published' ORDER BY published_at" \
  >"$after_revisions"
if ! cmp -s "$before_revisions" "$after_revisions"; then
  echo "upgrade changed a published semantic revision" >&2
  exit 1
fi

container build --file Containerfile --tag "$canary_image" .
database_ip=$(
  container inspect "$database_container" |
    python3 -c 'import json, sys; print(json.load(sys.stdin)[0]["status"]["networks"][0]["ipv4Address"].split("/")[0])'
)

set -a
. "$environment_file"
set +a
runtime_url=$(build_conninfo postgresem_runtime "$POSTGRESEM_RUNTIME_PASSWORD")
audit_url=$(build_conninfo postgresem_audit_writer "$POSTGRESEM_AUDIT_WRITER_PASSWORD")

container run --rm --network default \
  --env UPGRADE_RUNTIME_URL="$runtime_url" \
  --env UPGRADE_AUDIT_URL="$audit_url" \
  --env POSTGRESEM_RUNTIME_PASSWORD \
  --env POSTGRESEM_AUDIT_WRITER_PASSWORD \
  --env POSTGRESEM_DB_ROLE=postgresem_analyst \
  --volume "$PWD/tests/integration/queries:/queries:ro" \
  "$canary_image" \
  query execute /queries/commerce-revenue.json \
    --project commerce \
    --database-url-env UPGRADE_RUNTIME_URL \
    --audit-database-url-env UPGRADE_AUDIT_URL \
    --db-role-env POSTGRESEM_DB_ROLE \
  >/dev/null

container run --rm --network default \
  --env UPGRADE_AUDIT_URL="$audit_url" \
  --env POSTGRESEM_AUDIT_WRITER_PASSWORD \
  "$canary_image" \
  report operations \
    --audit-database-url-env UPGRADE_AUDIT_URL \
    --window-hours 1 \
  >/dev/null

echo "Local upgrade migrations, revision preservation, canary, and operations report passed"
