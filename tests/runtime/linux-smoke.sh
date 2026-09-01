#!/bin/sh
set -eu

expected_arch=${POSTGRESEM_EXPECTED_ARCH:?POSTGRESEM_EXPECTED_ARCH is required}
evidence_file=${POSTGRESEM_EVIDENCE_FILE:-}
database_host=${PGHOST:-db}
database_port=${PGPORT:-5432}
database_name=${PGDATABASE:-postgresem_dev}
runtime_password=${POSTGRESEM_RUNTIME_PASSWORD:?POSTGRESEM_RUNTIME_PASSWORD is required}
audit_password=${POSTGRESEM_AUDIT_WRITER_PASSWORD:?POSTGRESEM_AUDIT_WRITER_PASSWORD is required}
mutation_password=${POSTGRESEM_MUTATION_RUNTIME_PASSWORD:?POSTGRESEM_MUTATION_RUNTIME_PASSWORD is required}
test_root=${POSTGRESEM_TEST_ROOT:-/tests}

case "$(uname -m)" in
  x86_64|amd64) actual_arch=amd64 ;;
  aarch64|arm64) actual_arch=arm64 ;;
  *)
    echo "unsupported Linux runtime architecture: $(uname -m)" >&2
    exit 1
    ;;
esac

if [ "$(uname -s)" != Linux ] || [ "$actual_arch" != "$expected_arch" ]; then
  echo "runtime architecture does not match the expected Linux target" >&2
  exit 1
fi

version=$(postgresem --version)
runtime_url="host=${database_host} port=${database_port} dbname=${database_name} user=postgresem_runtime password=${runtime_password} sslmode=disable"
audit_url="host=${database_host} port=${database_port} dbname=${database_name} user=postgresem_audit_writer password=${audit_password} sslmode=disable"
mutation_url="host=${database_host} port=${database_port} dbname=${database_name} user=postgresem_mutation_runtime password=${mutation_password} sslmode=disable"

if DATABASE_URL="${runtime_url% sslmode=disable}" \
  postgresem catalog scan >/tmp/postgresem-implicit-tls.out 2>&1
then
  echo "catalog scan accepted a connection without explicit sslmode" >&2
  exit 1
fi

DATABASE_URL=$runtime_url postgresem catalog scan >/tmp/postgresem-catalog.json

DATABASE_URL=$runtime_url \
POSTGRESEM_AUDIT_DATABASE_URL=$audit_url \
POSTGRESEM_DB_ROLE=postgresem_analyst \
postgresem query execute \
  "${test_root}/integration/queries/commerce-revenue.json" \
  --project commerce >/tmp/postgresem-query.json
grep -q '"truncated": false' /tmp/postgresem-query.json

POSTGRESEM_MUTATION_DATABASE_URL=$mutation_url \
POSTGRESEM_AUDIT_DATABASE_URL=$audit_url \
POSTGRESEM_MUTATION_DB_ROLE=postgresem_order_writer \
postgresem mutation execute \
  "${test_root}/integration/mutations/order-insert.json" \
  --project commerce >/tmp/postgresem-mutation.json
grep -q '"affected_rows": 1' /tmp/postgresem-mutation.json
grep -q '"replayed": false' /tmp/postgresem-mutation.json
if grep -Eq '"sql"|"statement"|INSERT[[:space:]]+INTO' \
  /tmp/postgresem-mutation.json
then
  echo "runtime mutation response exposed generated SQL" >&2
  exit 1
fi

evidence=$(
  printf \
    '{"architecture":"%s","kernel_architecture":"%s","postgresql":"18","runtime":"%s","tls_mode":"explicit","catalog":true,"query":true,"mutation":true}\n' \
    "$actual_arch" "$(uname -m)" "$version"
)
printf '%s\n' "$evidence"
if [ -n "$evidence_file" ]; then
  printf '%s\n' "$evidence" >"$evidence_file"
fi
