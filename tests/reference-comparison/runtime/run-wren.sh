#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
output_dir="${1:?usage: run-wren.sh OUTPUT_DIR}"
database_host="${REFERENCE_DATABASE_HOST:-127.0.0.1}"
database_port="${REFERENCE_DATABASE_PORT:-5432}"
database_name="${REFERENCE_DATABASE_NAME:-reference}"
database_user="${REFERENCE_DATABASE_USER:-semantic_user}"
database_password="${REFERENCE_DATABASE_PASSWORD:-semantic-runtime}"
project="$repo_root/tests/reference-comparison/runtime/wren/project"
expected_file="$repo_root/tests/reference-comparison/runtime/expected.json"

mkdir -p "$output_dir"
connection_file="$output_dir/wren-connection.json"
mdl_file="$output_dir/wren-mdl.json"
query_file="$output_dir/wren-query.json"

cleanup() {
  rm -f -- "$connection_file" "$mdl_file" "$query_file"
}
trap cleanup EXIT

jq -n \
  --arg host "$database_host" \
  --argjson port "$database_port" \
  --arg database "$database_name" \
  --arg user "$database_user" \
  --arg password "$database_password" \
  '{
    datasource: "postgres",
    host: $host,
    port: $port,
    database: $database,
    user: $user,
    password: $password,
    kwargs: {sslmode: "disable"}
  }' > "$connection_file"
chmod 600 "$connection_file"

wren() {
  uvx --quiet --python 3.12 --from 'wrenai[postgres]==0.13.3' wren "$@"
}

version="$(wren --version)"
wren context validate --path "$project" >/dev/null
wren context build --path "$project" --output "$mdl_file" >/dev/null
wren query \
  --mdl "$mdl_file" \
  --connection-file "$connection_file" \
  --sql 'SELECT SUM(amount) AS total_revenue FROM orders' \
  --output json \
  --limit 10 \
  --quiet > "$query_file"

expected="$(jq -r '.expected.total_revenue' "$expected_file")"
actual="$(jq -r '.total_revenue' "$query_file")"
if [[ "$actual" != "$expected" ]]; then
  echo "Wren returned $actual, expected $expected" >&2
  exit 1
fi

jq -n \
  --arg engine "wren-ai" \
  --arg version "$version" \
  --arg task "commerce-total-revenue" \
  --arg expected "$expected" \
  --arg actual "$actual" \
  '{
    schema_version: "1",
    engine: $engine,
    version: $version,
    scope: "oss",
    task: $task,
    expected: {total_revenue: $expected},
    actual: {total_revenue: $actual},
    passed: ($expected == $actual)
  }' > "$output_dir/wren-runtime.json"
