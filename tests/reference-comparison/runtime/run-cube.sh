#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
output_dir="${1:?usage: run-cube.sh OUTPUT_DIR}"
database_host="${REFERENCE_DATABASE_HOST:-127.0.0.1}"
database_port="${REFERENCE_DATABASE_PORT:-5432}"
database_name="${REFERENCE_DATABASE_NAME:-reference}"
database_user="${REFERENCE_DATABASE_USER:-semantic_user}"
database_password="${REFERENCE_DATABASE_PASSWORD:-semantic-runtime}"
database_image="${REFERENCE_DATABASE_IMAGE:-postgres:18@sha256:4ef4dbc939d61acea57712655ddb4b4ab27419c913f94cca0cd57cb3ea3c2280}"
model_dir="$repo_root/tests/reference-comparison/runtime/cube/model"
expected_file="$repo_root/tests/reference-comparison/runtime/expected.json"
image="cubejs/cube:v1.7.31@sha256:88ea48a11489bfc396c9c8e387a445f6425447c0735352abb4c1d39edb97113d"
container_name="postgresem-reference-cube-${GITHUB_RUN_ID:-$$}"

mkdir -p "$output_dir"

cleanup() {
  docker rm -f "$container_name" >/dev/null 2>&1 || true
}
trap cleanup EXIT

docker pull --quiet "$image" >/dev/null
api_secret="$(openssl rand -hex 24)"
docker run --detach --rm \
  --name "$container_name" \
  --network host \
  --volume "$model_dir:/cube/conf/model:ro" \
  --env CUBEJS_DB_TYPE=postgres \
  --env CUBEJS_DB_HOST="$database_host" \
  --env CUBEJS_DB_PORT="$database_port" \
  --env CUBEJS_DB_NAME="$database_name" \
  --env CUBEJS_DB_USER="$database_user" \
  --env CUBEJS_DB_PASS="$database_password" \
  --env CUBEJS_API_SECRET="$api_secret" \
  --env CUBEJS_DEV_MODE=true \
  --env CUBEJS_SCHEMA_PATH=model \
  "$image" >/dev/null

ready=false
for _ in $(seq 1 90); do
  if curl --fail --silent http://127.0.0.1:4000/readyz >/dev/null 2>&1; then
    ready=true
    break
  fi
  sleep 2
done
if [[ "$ready" != true ]]; then
  docker logs "$container_name" >&2
  exit 1
fi

response="$(
  curl --fail --silent --show-error \
    --request POST \
    --header 'Content-Type: application/json' \
    --data '{"query":{"measures":["orders.total_revenue"]}}' \
    http://127.0.0.1:4000/cubejs-api/v1/load
)"
expected="$(jq -r '.expected.total_revenue' "$expected_file")"
actual="$(jq -r '.data[0]["orders.total_revenue"]' <<<"$response")"
if [[ "$actual" != "$expected" ]]; then
  echo "Cube returned $actual, expected $expected" >&2
  exit 1
fi

image_digest="$(
  docker image inspect \
    --format '{{index .RepoDigests 0}}' \
    "$image"
)"
jq -n \
  --arg engine "cube-core" \
  --arg version "1.7.31" \
  --arg image "$image_digest" \
  --arg database_image "$database_image" \
  --arg task "commerce-total-revenue" \
  --arg expected "$expected" \
  --arg actual "$actual" \
  '{
    schema_version: "1",
    engine: $engine,
    version: $version,
    image: $image,
    database_image: $database_image,
    scope: "oss",
    task: $task,
    expected: {total_revenue: $expected},
    actual: {total_revenue: $actual},
    passed: ($expected == $actual)
  }' > "$output_dir/cube-runtime.json"
