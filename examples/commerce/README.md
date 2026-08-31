# Commerce MCP example

This sample uses only fictional repository fixture data. It contains no
credentials and no raw SQL.

## LSQ files

- `orders-revenue.json`: paid-order revenue
- `revenue-by-month.json`: monthly revenue with a typed date filter
- `revenue-by-region.json`: safe many-to-one relationship dimension
- `active-subscriptions.json`: active subscription MRR by plan

All files use LSQ v1 and can be passed to the smoke client's `--lsq` option.

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

The client initializes MCP `2024-11-05`, verifies `tools/list`, calls all five
tools, lists resources, reads every advertised resource, prints privacy-safe
shape summaries, closes stdin, and exits nonzero on any protocol/tool failure.
Child stderr remains stderr and is prefixed for visibility.

`make mcp` reserves stdout for protocol messages. Do not pipe explanatory text
into its stdin: send one JSON-RPC object per line or use this client.
