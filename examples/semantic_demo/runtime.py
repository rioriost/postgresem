"""Local container plumbing; credentials never travel through browser requests."""

import hashlib
import json
from pathlib import Path
import re
import shutil
import subprocess
import sys

from scenarios import ORDER_IDS, ORDER_SCOPE, SCENARIOS
from smoke import McpClient, SmokeFailure

ROOT = Path(__file__).resolve().parents[2]
ROLES = {
    "analyst": "postgresem_analyst",
    "tenant_a": "postgresem_tenant_a",
    "tenant_b": "postgresem_tenant_b",
}

SNAPSHOT_SQL = f"""
SELECT json_build_object(
  'orders', (SELECT json_agg(r ORDER BY order_id) FROM (
    SELECT o.order_id, o.external_id, o.status, o.amount::text
    FROM commerce.orders o WHERE {ORDER_SCOPE}
  ) r),
  'items', (SELECT json_agg(r ORDER BY order_item_id) FROM (
    SELECT i.order_item_id, i.order_id, i.sku, i.quantity
    FROM commerce.order_item i JOIN commerce.orders o USING (order_id)
    WHERE {ORDER_SCOPE} ORDER BY i.order_item_id LIMIT 101
  ) r),
  'subscriptions', (SELECT json_agg(r ORDER BY subscription_id) FROM (
    SELECT subscription_id, plan, active, monthly_amount::text
    FROM billing.subscriptions WHERE subscription_id IN (1, 2, 3)
  ) r)
)
"""


class ContainerRuntime:
    def __init__(self, name="auto", *, container_prefix="postgresem"):
        if name == "auto":
            name = "apple" if sys.platform == "darwin" and shutil.which("container") else "docker"
        if name not in {"apple", "docker", "podman"}:
            raise SmokeFailure("unsupported demo container runtime")
        if not re.fullmatch(r"[a-z0-9][a-z0-9-]{0,48}", container_prefix):
            raise SmokeFailure("invalid demo container prefix")
        self.name = name
        self.container_prefix = container_prefix

    def compose(self):
        return ["docker", "compose", "-f", "compose.yaml", "-f", "compose.linux.yaml"]

    def start(self):
        if self.container_prefix != "postgresem":
            raise SmokeFailure("custom qualification containers must already be running")
        if self.name == "podman":
            command = ["systemctl", "--user", "start", "postgresem-gateway.service"]
        elif self.name == "apple":
            command = [
                "container-compose", "up", "--env-file", ".env",
                "-d", "--build", "db", "migrate", "seed", "gateway",
            ]
        else:
            command = self.compose() + ["up", "--detach", "--build", "gateway"]
        result = subprocess.run(command, cwd=ROOT, check=False)
        if result.returncode:
            raise SmokeFailure("could not start the local demo stack")

    def exec(self, service, command, *, environment=None):
        environment = environment or {}
        if self.name == "docker":
            prefix = self.compose() + ["exec", "-T"]
        else:
            prefix = ["container" if self.name == "apple" else "podman", "exec", "-i"]
        if service == "gateway":
            prefix += ["--user", "postgresem"]
        for key, value in environment.items():
            prefix += ["--env", f"{key}={value}"]
        target = service if self.name == "docker" else f"{self.container_prefix}-{service}"
        return prefix + [target] + command

    def client(self, profile):
        role = ROLES[profile]
        server = ["postgresem", "mcp", "serve"]
        if profile != "analyst":
            server = ["env", "-u", "POSTGRESEM_MCP_MUTATION_URL_ENV"] + server
        command = self.exec("gateway", server, environment={
            "POSTGRESEM_DEMO_READER": role,
            "POSTGRESEM_MCP_DB_ROLE_ENV": "POSTGRESEM_DEMO_READER",
        })
        client = McpClient(command, 60)
        try:
            initialized = client.request("initialize", {
                "protocolVersion": "2024-11-05", "capabilities": {},
                "clientInfo": {"name": "postgresem-meaning-lab", "version": "1"},
            })
            if (initialized.get("protocolVersion") != "2024-11-05"
                    or initialized.get("serverInfo", {}).get("name") != "postgresem"):
                raise SmokeFailure("unexpected demo MCP initialization response")
            client.notify("notifications/initialized", {})
        except SmokeFailure:
            client.abort()
            raise
        return client


class DatabaseProbe:
    def __init__(self, runtime):
        self.runtime = runtime

    def _read(self, expression, profile="analyst"):
        role = ROLES[profile]
        sql = f"""
BEGIN ISOLATION LEVEL REPEATABLE READ READ ONLY;
SET LOCAL ROLE "{role}";
SET LOCAL search_path = pg_catalog;
SET LOCAL statement_timeout = '5s';
SET LOCAL lock_timeout = '2s';
SELECT json_build_object(
  'role', current_user,
  'read_only', current_setting('transaction_read_only'),
  'data', ({expression})
);
ROLLBACK;
"""
        # Constant shell program: the password is expanded inside the DB container,
        # never placed in argv, SQL, captured output, or the host/browser environment.
        command = self.runtime.exec("db", [
            "sh", "-ec",
            'PGPASSWORD="$POSTGRESEM_RUNTIME_PASSWORD" PGCONNECT_TIMEOUT=5 '
            'exec psql -XAtq -v ON_ERROR_STOP=1 -h 127.0.0.1 -p 5432 '
            '-U postgresem_runtime -d postgresem_dev',
        ])
        try:
            result = subprocess.run(
                command, cwd=ROOT, input=sql, text=True, capture_output=True,
                timeout=20, check=False,
            )
        except (OSError, subprocess.TimeoutExpired) as error:
            raise SmokeFailure("demo PostgreSQL probe unavailable") from error
        if result.returncode or len(result.stdout) > 131_072:
            raise SmokeFailure("demo PostgreSQL read failed")
        try:
            envelope = json.loads(result.stdout)
        except json.JSONDecodeError as error:
            raise SmokeFailure("invalid demo PostgreSQL response") from error
        if (not isinstance(envelope, dict) or envelope.get("role") != role
                or envelope.get("read_only") != "on" or "data" not in envelope):
            raise SmokeFailure("demo PostgreSQL authority mismatch")
        return envelope

    def snapshot(self):
        source = self._read(SNAPSHOT_SQL)["data"]
        if (not isinstance(source, dict)
                or not all(isinstance(source.get(key), list)
                           for key in ("orders", "items", "subscriptions"))
                or not 4 <= len(source["orders"]) <= 5
                or not 1 <= len(source["items"]) <= 100
                or len(source["subscriptions"]) != 3):
            raise SmokeFailure("demonstration fixtures are missing or outside demo bounds")
        if (not set(ORDER_IDS[:4]).issubset({row.get("external_id") for row in source["orders"]})
                or {row.get("subscription_id") for row in source["subscriptions"]} != {1, 2, 3}):
            raise SmokeFailure("required demonstration fixture identities are missing")
        source["fingerprint"] = hashlib.sha256(
            json.dumps(source, sort_keys=True, separators=(",", ":")).encode()
        ).hexdigest()
        return source

    def baseline(self, scenario_id, choice):
        return self._read(SCENARIOS[scenario_id]["baseline"][choice])

    def tenant(self, profile):
        return self._read(
            "SELECT sum(amount)::text FROM rls_fixture.orders "
            "WHERE external_id IN ('fixture-a-1', 'fixture-a-2', 'fixture-b-1')",
            profile,
        )
