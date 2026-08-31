#!/bin/sh
set -eu

output_root=${1:-backups}
container_name=${POSTGRESEM_DATABASE_CONTAINER:-postgresem-db}
timestamp=$(date -u +%Y%m%dT%H%M%SZ)
backup_dir=
created=false
complete=false

cleanup() {
  if [ "$created" = true ] && [ "$complete" != true ] && [ -d "$backup_dir" ]; then
    rm -rf "$backup_dir"
  fi
}

checksum() {
  if command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "$1" | awk '{ print $1 }'
  elif command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | awk '{ print $1 }'
  else
    echo "shasum or sha256sum is required" >&2
    exit 1
  fi
}

trap cleanup 0
umask 077
mkdir -p "$output_root"
backup_dir=$(mktemp -d "$output_root/postgresem-$timestamp.XXXXXX")
created=true

command -v container >/dev/null 2>&1 || {
  echo "Apple Container CLI is required" >&2
  exit 1
}

container exec --user postgres "$container_name" \
  pg_dump --format=custom --dbname=postgresem_dev \
  >"$backup_dir/database.dump"
container exec --user postgres "$container_name" \
  pg_dumpall --globals-only \
  >"$backup_dir/globals.sql"
container exec --user postgres "$container_name" \
  psql --no-psqlrc --tuples-only --no-align \
    --dbname=postgresem_dev \
    --command='SELECT version FROM semantic.schema_migration ORDER BY version' \
  >"$backup_dir/migrations.txt"
container exec --user postgres "$container_name" \
  psql --no-psqlrc --tuples-only --no-align \
    --dbname=postgresem_dev \
    --command="SELECT canonical_hash FROM semantic.revision WHERE status = 'published' ORDER BY published_at" \
  >"$backup_dir/published-revisions.txt"

database_checksum=$(checksum "$backup_dir/database.dump")
globals_checksum=$(checksum "$backup_dir/globals.sql")

cat >"$backup_dir/MANIFEST" <<EOF
format=postgresem-local-backup-v1
created_at=$timestamp
database=postgresem_dev
database_dump_sha256=$database_checksum
globals_dump_sha256=$globals_checksum
source_database_name_must_be_preserved=true
EOF

complete=true
printf 'Created local reference backup: %s\n' "$backup_dir"
printf 'This backup contains every local pilot database row and cluster role.\n'
printf 'Protect globals.sql: it contains role metadata and password verifiers.\n'
