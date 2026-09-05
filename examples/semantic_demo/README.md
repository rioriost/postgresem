# Meaning Lab: one business question, two execution paths

This is the single application sample for PostgreSQL Semantic Gateway 1.0.
It uses real PostgreSQL reads and governed writes, not canned result rows.
The accompanying CLI requests and `smoke.py` exercise the same published models.

**The claim is reusable, enforced business meaning, not that SQL cannot be
correct.** A knowledgeable SQL author can implement every demonstrated read.
postgresem lets consumers choose published metrics without independently
reimplementing status filters, relationship grain, authorization, and write
retry rules.

## Run

Use Python 3.9+, Git, Make, and Apple Container with `container-compose` on
Apple silicon macOS, Docker Engine with Compose v2 on Linux, or an installed
rootless Podman Quadlet stack. No Python packages or OpenAI key are required.

From the repository root:

```sh
cp .env.example .env  # only if .env does not already exist
chmod 600 .env
# Replace password placeholders with separate local-only credentials.
make web-demo
```

Open <http://127.0.0.1:8765>. The command starts the existing local fixture
stack, then a loopback Python server. Explicit runtime choices:

```sh
make web-demo DEMO_RUNTIME=apple
make web-demo DEMO_RUNTIME=docker
make web-demo DEMO_RUNTIME=podman
```

The UI starts in **English** on every page load. Use the **Language** selector
in the top bar to switch to **日本語** or back to **English**. Switching only
changes presentation: current scenario/mode selections, fetched results, and
in-flight operations are retained, without another API call or write.
UI guidance, confirmations, status messages, and accessibility labels switch
language. SQL, LSQ/LSM, source values, IDs, raw API diagnostics/traces, and
original OpenAI output remain unchanged; planner prompts are not translated.

For Podman, first follow [the Quadlet setup](../../docs/linux-containers.md#rootless-podman-quadlet).
Its database and gateway credentials live in the installed systemd environment
files; repository `.env` is used only for optional OpenAI settings in that path.
To attach without rebuilding/restarting containers:

```sh
python3 examples/semantic_demo/server.py --runtime docker --no-start
```

`Ctrl-C` stops only the Web server. Use `make dev-down`, `make docker-down`,
or stop the Quadlet user services to stop the stack. Database data is retained.
Never delete an existing volume to resolve a migration failure: retain a backup
and use the [upgrade/recovery procedure](../../docs/operations.md).

## Walk through the business case

Imagine a finance agent preparing the revenue report and recording a newly
paid order. Physical types alone do not establish revenue recognition or the
grain at which a monetary value should be aggregated.

| Question | Representative schema-only mistake | Published meaning | Initial direct / semantic / independent answer |
|---|---|---|---|
| Recognized order revenue | Sum amounts for paid, pending, and cancelled orders | `revenue` applies `status = paid` | 545.50 / 200.50 / 200.50 |
| Recognized revenue of orders containing SKU-RED | Join matching items and sum the repeated order amount | `revenue` stays anchored to `order_id`; each matching order contributes once | 320.50 / 200.50 / 200.50 |
| Current MRR | Sum active and inactive subscription rows | `mrr` applies `active = true` | 627.00 / 128.00 / 128.00 |

Amounts are in one fictional fixture currency. The SKU question is **whole-order
revenue for orders containing the SKU**, not revenue allocated to that SKU.
The duplicate RED item is intentional: order 1 has two matching item rows,
but it remains one 120.00 order.

The screen shows the question, authored business rule, actual direct SQL,
typed LSQ, model description, validation, semantic lineage, immutable revision,
query/audit ID, and live source ledger. Decimal arithmetic over that ledger
independently derives the expected answer. It does not reuse the compiler's
result or the baseline SQL aggregate as its oracle.

Only `fixture-order-1` through `fixture-order-4`, the optional
`meaning-lab-paid-order`, and subscription IDs 1, 2, 3 participate. Unrelated
inserts do not change the comparison. Missing/out-of-bounds fixtures fail
explicitly rather than returning sample answers.

### Record, replay, reconcile

The write button asks for confirmation before inserting a real, fictional
**45.00 paid order** through `validate_semantic_mutation` and
`mutate_semantic_model`. It immediately repeats the identical request, reads
`reconcile_semantic_mutation`, and attempts a changed-amount retry with the
same key. That last request must be rejected with
`MUTATION_IDEMPOTENCY_CONFLICT`.

The first insertion changes recognized revenue from 200.50 to 245.50. Its
baseline total becomes 590.50. The new order has no items, so SKU-RED revenue
and MRR remain unchanged. Repeated button presses reuse
`meaning-lab-paid-order-v1`: no additional order is inserted and the expected
revenue delta is zero. The UI displays actual/expected deltas, mutation IDs,
replay flags, and reconciliation evidence.

Do not manually remove the row without its corresponding idempotency state:
replaying a committed operation does not recreate manually deleted data. The
demo reports this inconsistency instead of calling it a successful write.
An interrupted request may already have committed; restart and retry only the
same operation/key.

### Check the boundary

The boundary button demonstrates hidden/unknown metric rejection, raw-SQL
rejection, and two separately configured tenant readers. Both direct SQL and
postgresem see **250.00 for tenant A** and **999.00 for tenant B**, not 1249.00.
The query deliberately scopes all three fixture external IDs; PostgreSQL RLS
removes the other tenant's rows on both paths. There is no caller-supplied
tenant predicate masquerading as RLS, and no browser role selector.

## Optional live OpenAI planner

Add these literal settings to repository `.env`, then restart `make web-demo`:

```dotenv
POSTGRESEM_DEMO_OPENAI=1
OPENAI_API_KEY=your-local-api-key
OPENAI_MODEL=gpt-4.1-mini
```

Select the live planner mode before running a comparison. Calls incur OpenAI
API usage charges. The two independent planning calls use the same configured
model, question, and instruction, with different contexts: a reviewed physical
fixture schema versus the actual public `describe_semantic_model` response.
No conversation or previous answer is shared between them.

Both sides choose among **bounded, reviewed candidate plans**. The SQL choices
include a correct status-filtered/EXISTS/active-only query; an actual model may
select it. Both-correct and wrong semantic choices are displayed honestly.
Candidate position and a small authored workload make this an instructional
experiment, **not a model accuracy benchmark or a general text-to-SQL agent**.
The default deterministic mode is explicitly an authored comparison, never
presented as evidence that a live agent failed.

Only fixed fictional questions, candidate plans, the reviewed fixture schema,
and public metadata for the bundled immutable fixture revision are sent to
OpenAI. No ledger rows, query results, PostgreSQL credentials, browser input,
or production metadata are sent. The API key stays on the Python server and
is used only in the authorization header to the fixed HTTPS OpenAI endpoint.
Environment-configured proxies and redirects are disabled. The planner refuses
other publication hashes, unapproved outputs, truncated replies, and upstream
errors; it never silently executes a default after a failed model call.

The `.env` loader reads only these three literal settings; it does not execute
shell substitutions or interpolate variables. Existing process environment
settings take precedence. Never use this sample with production databases or
production API credentials. Do not commit `.env`.

## Execution and trust boundary

```text
Browser -> loopback Python -> fixed scenario dispatch
                           -> fixed read-only SQL probe -> PostgreSQL
                           -> stdio MCP -> postgresem -> PostgreSQL
                           -> optional bounded OpenAI planner (metadata only)
```

The direct probe is demonstration infrastructure, **not a gateway SQL tool**.
SQL is selected exclusively from server-owned definitions. No HTTP request or
model response can supply SQL, a connection string, credentials, or a database
role. The probe logs in as `postgresem_runtime`, switches to the same approved
reader role used by MCP, sets `search_path = pg_catalog`, applies timeouts, and
executes in a read-only transaction. It never uses the database administrator.
The gateway's public no-raw-SQL contract remains unchanged.

Writes use the existing separate mutation runtime/approved writer. Two other
fixed MCP sessions use tenant A/B reader roles with mutation tools disabled.
This is a local operator demonstration of those identities, not a multi-user
authentication application. HTTP/JWT deployment and operational disaster
recovery are covered in their dedicated guides, not simulated in this UI.

Host/Origin checks, JSON-only bounded requests, CSP, and text-only DOM rendering
protect the loopback UI. Do not reverse-proxy or expose it remotely. Source
rows and fixed physical SQL are intentionally visible to the local demo user;
this is not the disclosure policy of public MCP responses.

Workflows serialize the demo's own writes. Ledger fingerprints before/after
and publication identities detect observed concurrent changes and reject
comparison rather than scoring inconsistent inputs. Separate PostgreSQL/MCP
connections do **not** share an exported snapshot: run without other writers;
the fingerprint check is not proof against a change-and-revert between reads.

## CLI and qualification

Reusable LSQ/LSM documents live in `requests/`. For example:

```sh
python3 examples/semantic_demo/smoke.py \
  --lsq examples/semantic_demo/requests/revenue-by-month.json \
  --lsm examples/semantic_demo/requests/order-insert.json \
  -- make docker-mcp
make test-web-demo
make test-semantic-demo DEMO_RUNTIME=docker  # existing running fixture stack
```

On Apple Container replace `make docker-mcp` with `make mcp`. `smoke.py` is the
command-line companion to this sample, not a second scenario. It writes its
own fixed **pending** order and does not change the Meaning Lab comparison.

Unit coverage uses test-only fake transports. `e2e.py` uses the real browser
HTTP routes, MCP processes, PostgreSQL probe, and persisted rows; it also runs
the correct direct-SQL candidates, all available semantic choices, writes and
repeats the mutation, and checks tenant/rejection behavior. CI runs this
against the Docker Compose stack without an API key. No runtime fake-result
fallback exists.

The optional `node examples/semantic_demo/browser_test.cjs` regression runs
the language switch in Chromium with Playwright installed in the development
environment. It uses an isolated HTTP server and test-only API responses,
never PostgreSQL or OpenAI. It covers English defaults, both languages,
preserved selections/results, in-flight switching, confirmations, errors,
mobile layout, and the absence of extra requests when switching languages.
Playwright is not a runtime dependency of the demo.
