#!/usr/bin/env bash
set -euo pipefail
export LC_ALL=C

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
output_dir="${1:?usage: run-malloy.sh OUTPUT_DIR}"
project_source="$repo_root/tests/reference-comparison/runtime/malloy/project"
expected_file="$repo_root/tests/reference-comparison/runtime/expected.json"
database_image="${REFERENCE_DATABASE_IMAGE:-postgres:18@sha256:4ef4dbc939d61acea57712655ddb4b4ab27419c913f94cca0cd57cb3ea3c2280}"
runtime_version="0.0.432"
release_reference="0.0.433"

mkdir -p "$output_dir"
output_dir="$(cd "$output_dir" && pwd)"
query_file="$output_dir/malloy-query.json"
work_root="$(mktemp -d "$output_dir/malloy-project.XXXXXX")"
project="$work_root/project"
cp -R "$project_source" "$project"

cleanup() {
  rm -f -- "$query_file"
  rm -rf -- "$work_root"
}
trap cleanup EXIT

npm ci \
  --prefix "$project" \
  --ignore-scripts \
  --no-audit \
  --no-fund \
  --silent
node "$project/run.mjs" > "$query_file"

expected="$(jq -r '.expected.total_revenue' "$expected_file")"
actual_raw="$(jq -er '.[0].total_revenue | numbers' "$query_file")"
actual="$(printf '%.2f' "$actual_raw")"
if [[ "$actual" != "$expected" ]]; then
  echo "Malloy returned $actual, expected $expected" >&2
  exit 1
fi
lock_digest="$(
  openssl dgst -sha256 -r "$project_source/package-lock.json" |
    awk '{print "sha256:" $1}'
)"

jq -n \
  --arg engine "malloy" \
  --arg version "$runtime_version" \
  --arg release_reference "$release_reference" \
  --arg node_version "$(node --version)" \
  --arg lock_digest "$lock_digest" \
  --arg database_image "$database_image" \
  --arg task "commerce-total-revenue" \
  --arg expected "$expected" \
  --arg actual "$actual" \
  '{
    schema_version: "1",
    engine: $engine,
    version: $version,
    release_reference: $release_reference,
    runtime_boundary: "latest published npm packages; the 0.0.433 source release has no matching npm artifacts",
    node: $node_version,
    lock: $lock_digest,
    database_image: $database_image,
    scope: "oss",
    task: $task,
    expected: {total_revenue: $expected},
    actual: {total_revenue: $actual},
    passed: ($expected == $actual)
  }' > "$output_dir/malloy-runtime.json"
