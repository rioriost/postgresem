#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
output_dir="${1:?usage: run-wren.sh OUTPUT_DIR}"
database_host="${REFERENCE_DATABASE_HOST:-127.0.0.1}"
database_port="${REFERENCE_DATABASE_PORT:-5432}"
database_name="${REFERENCE_DATABASE_NAME:-reference}"
database_user="${REFERENCE_DATABASE_USER:-semantic_user}"
database_password="${REFERENCE_DATABASE_PASSWORD:-semantic-runtime}"
database_image="${REFERENCE_DATABASE_IMAGE:-postgres:18@sha256:4ef4dbc939d61acea57712655ddb4b4ab27419c913f94cca0cd57cb3ea3c2280}"
project="$repo_root/tests/reference-comparison/runtime/wren/project"
environment="$repo_root/tests/reference-comparison/runtime/wren/environment"
expected_file="$repo_root/tests/reference-comparison/runtime/expected.json"
python_version="3.12.13"

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
  uv run \
    --quiet \
    --project "$environment" \
    --frozen \
    --no-dev \
    --python "$python_version" \
    wren "$@"
}

version="$(wren --version)"
engine_version="$(
  uv run \
    --quiet \
    --project "$environment" \
    --frozen \
    --no-dev \
    --python "$python_version" \
    python -c 'from importlib.metadata import version; print(version("wren-core-py"))'
)"
lock_digest="$(
  openssl dgst -sha256 -r "$environment/uv.lock" |
    awk '{print "sha256:" $1}'
)"
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
  --arg engine_version "$engine_version" \
  --arg python_version "$python_version" \
  --arg uv_version "$(uv --version)" \
  --arg lock_digest "$lock_digest" \
  --arg database_image "$database_image" \
  --arg task "commerce-total-revenue" \
  --arg expected "$expected" \
  --arg actual "$actual" \
  '{
    schema_version: "1",
    engine: $engine,
    version: $version,
    dependencies: {"wren-core-py": $engine_version},
    python: $python_version,
    uv: $uv_version,
    lock: $lock_digest,
    database_image: $database_image,
    scope: "oss",
    task: $task,
    expected: {total_revenue: $expected},
    actual: {total_revenue: $actual},
    passed: ($expected == $actual)
  }' > "$output_dir/wren-runtime.json"
