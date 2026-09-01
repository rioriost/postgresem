# Commerce MCP example

This sample uses only fictional repository fixture data. It contains no
credentials and no raw SQL.

## Contract files

- `orders-revenue.json`: paid-order revenue
- `revenue-by-month.json`: monthly revenue with a typed date filter
- `revenue-by-region.json`: safe many-to-one relationship dimension
- `active-subscriptions.json`: active subscription MRR by plan
- `order-insert.json`: bounded LSM v1 order insert
- `order-upsert.json`: approved idempotent LSM v1 order upsert

Query files use LSQ v1 and can be passed with `--lsq`. Mutation files use LSM
v1; pass an insert request to the smoke client with `--lsm`.

## Run

Python 3.9 or later is required for the smoke client. Start the stack first:

```sh
cp .env.example .env
# Replace every placeholder with local-only development values.
make dev-up
```

The stdlib-only executable client accepts the MCP server command after `--` and
launches it without a shell:

```sh
python3 examples/commerce/mcp_smoke.py -- make mcp
```

Alternative command arguments are passed exactly, for example:

```sh
python3 examples/commerce/mcp_smoke.py -- \
  container exec -i --user postgresem \
  postgresem-gateway postgresem mcp serve
```

Choose an LSQ and model description target:

```sh
python3 examples/commerce/mcp_smoke.py \
  --lsq examples/commerce/active-subscriptions.json \
  --model subscriptions -- make mcp
```

The client initializes MCP `2024-11-05`, verifies `tools/list`, calls the five
query/discovery tools and the two governed-mutation tools, lists resources,
reads every advertised resource, prints privacy-safe shape summaries, closes
stdin, and exits nonzero on any protocol/tool failure. Repeating the same LSM
demonstrates idempotent replay rather than inserting a duplicate row. Child
stderr remains stderr and is prefixed for visibility.

Run an approved upsert directly from the source checkout:

```sh
set -a
. ./.env
set +a
export POSTGRESEM_MUTATION_DATABASE_URL="host=127.0.0.1 port=55432 dbname=postgresem_dev user=postgresem_mutation_runtime password=${POSTGRESEM_MUTATION_RUNTIME_PASSWORD} sslmode=disable"
export POSTGRESEM_AUDIT_DATABASE_URL="host=127.0.0.1 port=55432 dbname=postgresem_dev user=postgresem_audit_writer password=${POSTGRESEM_AUDIT_WRITER_PASSWORD} sslmode=disable"
export POSTGRESEM_MUTATION_DB_ROLE=postgresem_order_writer
cargo run -p postgresem -- mutation execute \
  examples/commerce/order-upsert.json --project commerce
```

For a browser demonstration built on the same stdio MCP boundary, see the
[local commerce Web demo](../web_demo/README.md).

`make mcp` reserves stdout for protocol messages. Do not pipe explanatory text
into its stdin: send one JSON-RPC object per line or use this client.
