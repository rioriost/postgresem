# 30-minute read-only commerce pilot on macOS

This developer-preview path uses Apple Container, `container-compose`, the
repository's fictional commerce fixture, and MCP stdio. It does not require or
accept production credentials.

## 1. Clone and enter the repository

```sh
git clone https://github.com/rioriost/postgresem.git
cd postgresem
```

Use macOS on Apple silicon with Rust 1.85 or later, Python 3.9 or later, Apple
Container 1.0.0, and `container-compose` 1.1.0. Confirm the host tools before
continuing:

```sh
rustc --version
python3 --version
container --version
container-compose --version
```

## 2. Create local-only credentials

Copy the template, then replace every placeholder password and connection URL.
The following script creates unrelated random values for this disposable local
stack:

```sh
cp .env.example .env
python3 - <<'PY'
from pathlib import Path
import secrets

path = Path(".env")
values = {}
for line in path.read_text().splitlines():
    if line and not line.startswith("#") and "=" in line:
        key, value = line.split("=", 1)
        values[key] = value

superuser = secrets.token_hex(24)
runtime = secrets.token_hex(24)
audit = secrets.token_hex(24)
values.update({
    "POSTGRES_SUPERUSER_PASSWORD": superuser,
    "POSTGRESEM_RUNTIME_PASSWORD": runtime,
    "POSTGRESEM_AUDIT_WRITER_PASSWORD": audit,
    "DATABASE_URL":
        f"postgresql://postgresem_runtime:{runtime}@127.0.0.1:55432/postgresem_dev",
    "POSTGRESEM_AUDIT_DATABASE_URL":
        f"postgresql://postgresem_audit_writer:{audit}@127.0.0.1:55432/postgresem_dev",
})

out = []
for line in path.read_text().splitlines():
    if line and not line.startswith("#") and "=" in line:
        key = line.split("=", 1)[0]
        line = f"{key}={values[key]}"
    out.append(line)
path.write_text("\n".join(out) + "\n")
PY
```

Do not reuse these values elsewhere. `.env` is ignored by Git, but still treat
it as a secret-bearing local file.

## 3. Check and start the stack

```sh
make doctor
make dev-up
```

`make doctor` should report `postgresem 0.3.0-beta.1`, `macos/aarch64`, and
Apple Container as available. `make dev-up` builds the gateway, starts
PostgreSQL 18 on `127.0.0.1:55432`, applies migrations in order, publishes the
idempotent commerce revision, and leaves `postgresem-db` and
`postgresem-gateway` running.

Verify the containers and migrations:

```sh
container list --all | grep 'postgresem-\(db\|gateway\)'
container exec postgresem-db \
  psql --no-psqlrc -U postgres -d postgresem_dev -Atc \
  'SELECT version FROM semantic.schema_migration ORDER BY version'
```

Expected migration rows are `0001_semantic_schema`,
`0002_published_snapshot_v1`, `0003_guarded_execution_audit`, and
`0004_beta_operational_report`. Hashes, container addresses, and timestamps are
intentionally not fixed.

## 4. Attach to MCP and run the pilot

`make mcp` attaches the terminal to a line-delimited JSON-RPC stdio server. It
does not open a prompt or an HTTP port. The sample client launches that exact
command, exercises every tool, lists and reads every resource, and closes stdin
so the server exits:

```sh
python3 examples/commerce/mcp_smoke.py -- make mcp
```

Expected summary shapes include:

```text
initialize: protocol=2024-11-05 server=postgresem/<version>
tools/list: 5 tools
list_semantic_models: models=[orders, subscriptions, tenant_orders]
describe_semantic_model: model=orders fields=<count> metrics=<count>
validate_semantic_query: valid=True hash=sha256:<64 hex>
explain_semantic_query: models=[orders] effective_limit=10
query_semantic_model: columns=[revenue:numeric] rows=<count> truncated=False
resources/list: 5 resources
PASS: MCP stdio commerce smoke completed
```

The exact semantic revision, normalized LSQ hash, query ID, timings, and row
values can change. Numeric results are represented as JSON strings.

The client reads `orders-revenue.json` by default. Try another supplied LSQ:

```sh
python3 examples/commerce/mcp_smoke.py \
  --lsq examples/commerce/revenue-by-month.json -- make mcp
```

### Requests sent by the client

The wire format is one compact JSON object per line. These abbreviated examples
show the required request shapes:

```json
{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"commerce-smoke","version":"1"}}}
{"jsonrpc":"2.0","method":"notifications/initialized","params":{}}
{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}
{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"list_semantic_models","arguments":{"schema_version":"1"}}}
{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"describe_semantic_model","arguments":{"schema_version":"1","model":"orders"}}}
{"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"validate_semantic_query","arguments":{"schema_version":"1","lsq":{"schema_version":"1","model":"orders","metrics":[{"metric":"revenue"}],"limit":10}}}}
{"jsonrpc":"2.0","id":6,"method":"tools/call","params":{"name":"query_semantic_model","arguments":{"schema_version":"1","lsq":{"schema_version":"1","model":"orders","metrics":[{"metric":"revenue"}],"limit":10}}}}
{"jsonrpc":"2.0","id":7,"method":"tools/call","params":{"name":"explain_semantic_query","arguments":{"schema_version":"1","lsq":{"schema_version":"1","model":"orders","metrics":[{"metric":"revenue"}],"limit":10}}}}
{"jsonrpc":"2.0","id":8,"method":"resources/list","params":{}}
{"jsonrpc":"2.0","id":9,"method":"resources/read","params":{"uri":"semantic://projects/commerce/revisions/current"}}
```

Tool results contain `content[0].text`, matching `structuredContent`, plus
`isError`. Resource reads contain JSON text in `contents[0].text`. Validation
and explanation expose semantic output schema and public lineage, never
generated SQL.

## 5. Inspect the audit safely

The preview does not provide an audit-reader login. For this local fixture only,
use the container's PostgreSQL administrator and select the non-payload
columns:

```sh
container exec postgresem-db \
  psql --no-psqlrc -U postgres -d postgresem_dev -P pager=off -c "
    BEGIN READ ONLY;
    SELECT query_id, status, error_code, config_profile,
           validation_duration_ms, compile_duration_ms,
           database_duration_ms, serialization_duration_ms,
           row_count, byte_count, truncated, started_at, completed_at
    FROM semantic.query_audit
    ORDER BY started_at DESC
    LIMIT 10;
    COMMIT;"
```

Expect at least one `succeeded` row with `config_profile = 'mcp-stdio'`. Avoid
selecting or copying `lineage`, `policy_context`, hashes, or principal hashes
unless your review specifically requires them.

## 6. Shut down

```sh
make dev-down
```

Shutdown stops the compose services but retains the named PostgreSQL volume.
That persistence is convenience, not a tested backup. See
[operations](operations.md) before reusing or upgrading a pilot.
