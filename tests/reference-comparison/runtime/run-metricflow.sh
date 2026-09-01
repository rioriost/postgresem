#!/usr/bin/env bash
set -euo pipefail
export LC_ALL=C

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
output_dir="${1:?usage: run-metricflow.sh OUTPUT_DIR}"
database_host="${REFERENCE_DATABASE_HOST:-127.0.0.1}"
database_port="${REFERENCE_DATABASE_PORT:-5432}"
database_name="${REFERENCE_DATABASE_NAME:-reference}"
database_user="${REFERENCE_DATABASE_USER:-semantic_user}"
database_password="${REFERENCE_DATABASE_PASSWORD:-semantic-runtime}"
project_source="$repo_root/tests/reference-comparison/runtime/metricflow/project"
expected_file="$repo_root/tests/reference-comparison/runtime/expected.json"
bundle='dbt-metricflow[dbt-postgres]==0.14.0'
metricflow_version='0.212.0'
dbt_core_version='1.12.3'
dbt_postgres_version='1.11.0'

mkdir -p "$output_dir"
output_dir="$(cd "$output_dir" && pwd)"
profiles_file="$output_dir/profiles.yml"
result_file="$output_dir/metricflow-result.csv"
work_root="$(mktemp -d "$output_dir/metricflow-project.XXXXXX")"
project="$work_root/project"
cp -R "$project_source" "$project"

cleanup() {
  rm -f -- "$profiles_file" "$result_file"
  rm -rf -- "$work_root"
}
trap cleanup EXIT

jq -n \
  --arg host "$database_host" \
  --argjson port "$database_port" \
  --arg user "$database_user" \
  --arg password "$database_password" \
  --arg database "$database_name" \
  '{
    config: {
      send_anonymous_usage_stats: false
    },
    postgresem_reference: {
      target: "local",
      outputs: {
        local: {
          type: "postgres",
          host: $host,
          port: $port,
          user: $user,
          password: $password,
          dbname: $database,
          schema: "commerce",
          threads: 1,
          sslmode: "disable"
        }
      }
    }
  }' > "$profiles_file"
chmod 600 "$profiles_file"

export DBT_PROFILES_DIR="$output_dir"
export DBT_LOG_PATH="$output_dir/metricflow-logs"

dbt() {
  uvx \
    --quiet \
    --python 3.12 \
    --from "$bundle" \
    --with "metricflow==$metricflow_version" \
    --with "dbt-core==$dbt_core_version" \
    --with "dbt-postgres==$dbt_postgres_version" \
    dbt "$@"
}

mf() {
  uvx \
    --quiet \
    --python 3.12 \
    --from "$bundle" \
    --with "metricflow==$metricflow_version" \
    --with "dbt-core==$dbt_core_version" \
    --with "dbt-postgres==$dbt_postgres_version" \
    mf "$@"
}

dbt parse --project-dir "$project" --quiet
(
  cd "$project"
  mf validate-configs
  mf query \
    --metrics total_revenue \
    --decimals 2 \
    --csv "$result_file" \
    --quiet
)

expected="$(jq -r '.expected.total_revenue' "$expected_file")"
actual_raw="$(tail -n 1 "$result_file" | tr -d '\r')"
actual="$(printf '%.2f' "$actual_raw")"
if [[ "$actual" != "$expected" ]]; then
  echo "MetricFlow returned $actual, expected $expected" >&2
  exit 1
fi

bundle_version="$(mf --version)"
jq -n \
  --arg engine "metricflow" \
  --arg version "$metricflow_version" \
  --arg bundle "$bundle_version" \
  --arg dbt_core_version "$dbt_core_version" \
  --arg dbt_postgres_version "$dbt_postgres_version" \
  --arg task "commerce-total-revenue" \
  --arg expected "$expected" \
  --arg actual "$actual" \
  '{
    schema_version: "1",
    engine: $engine,
    version: $version,
    bundle: $bundle,
    dependencies: {
      "dbt-core": $dbt_core_version,
      "dbt-postgres": $dbt_postgres_version
    },
    scope: "oss",
    task: $task,
    expected: {total_revenue: $expected},
    actual: {total_revenue: $actual},
    passed: ($expected == $actual)
  }' > "$output_dir/metricflow-runtime.json"
