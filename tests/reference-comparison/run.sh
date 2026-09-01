#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
tmp_dir="$(mktemp -d)"
cleanup() {
  rm -f -- \
    "$tmp_dir/osi-snapshot.json" \
    "$tmp_dir/compiled-query.json" \
    "$tmp_dir/catalog-diff.json"
  rmdir -- "$tmp_dir"
}
trap cleanup EXIT

cd "$repo_root"

cargo test --quiet -p postgresem-compiler --test m0_evals
cargo test --quiet -p postgresem osi

cargo run --quiet -p postgresem -- \
  model import osi \
  --from fixtures/interoperability/osi-commerce.yaml \
  --catalog fixtures/interoperability/osi-commerce-catalog.json \
  --snapshot-only > "$tmp_dir/osi-snapshot.json"

cargo run --quiet -p postgresem -- \
  query compile \
  examples/commerce/orders-revenue.json \
  --snapshot "$tmp_dir/osi-snapshot.json" > "$tmp_dir/compiled-query.json"

jq -e '
  .output_schema == [{"name":"revenue","data_type":"numeric"}]
  and .lineage.models == ["orders"]
  and .lineage.source_columns == ["commerce.orders.amount"]
' "$tmp_dir/compiled-query.json" >/dev/null

cargo run --quiet -p postgresem -- \
  catalog diff \
  --from fixtures/interoperability/osi-commerce-catalog.json \
  --to fixtures/interoperability/osi-commerce-catalog.json \
  --fail-on-breaking > "$tmp_dir/catalog-diff.json"

jq -e '
  .compatibility == "compatible"
  and .summary == {
    "total": 0,
    "compatible": 0,
    "review_required": 0,
    "breaking": 0
  }
' "$tmp_dir/catalog-diff.json" >/dev/null

echo "M7 reference comparison fixtures passed"
