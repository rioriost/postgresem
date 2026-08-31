#!/bin/sh
set -eu

backup_dir=${1:?usage: scripts/verify-backup.sh BACKUP_DIRECTORY}

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

for file in MANIFEST database.dump globals.sql migrations.txt published-revisions.txt; do
  if [ ! -f "$backup_dir/$file" ]; then
    echo "backup file is missing: $file" >&2
    exit 1
  fi
done

expected_database=$(
  awk -F= '$1 == "database_dump_sha256" { print $2 }' "$backup_dir/MANIFEST"
)
expected_globals=$(
  awk -F= '$1 == "globals_dump_sha256" { print $2 }' "$backup_dir/MANIFEST"
)

if [ "$(checksum "$backup_dir/database.dump")" != "$expected_database" ]; then
  echo "database dump checksum mismatch" >&2
  exit 1
fi
if [ "$(checksum "$backup_dir/globals.sql")" != "$expected_globals" ]; then
  echo "globals dump checksum mismatch" >&2
  exit 1
fi

container exec -i --user postgres \
  "${POSTGRESEM_DATABASE_CONTAINER:-postgresem-db}" \
  pg_restore --list <"$backup_dir/database.dump" >/dev/null

echo "Backup files and archive structure verified"
