"""Fixed-scope runtime and probe tests without subprocess or database I/O."""

from copy import deepcopy
import json
import os
import subprocess
import unittest
from unittest.mock import Mock, patch

from runtime import ContainerRuntime, DatabaseProbe, ROLES, SNAPSHOT_SQL
from scenarios import ORDER_IDS, SCENARIOS
from smoke import McpClient, SmokeFailure


def source_rows():
    return {
        "orders": [
            {"order_id": index, "external_id": external_id, "status": "paid", "amount": "1.00"}
            for index, external_id in enumerate(ORDER_IDS[:4], 1)
        ],
        "items": [{"order_item_id": 1, "order_id": 1, "sku": "SKU-RED", "quantity": 1}],
        "subscriptions": [
            {"subscription_id": index, "plan": "unit", "active": True, "monthly_amount": "1.00"}
            for index in (1, 2, 3)
        ],
    }


class RuntimeTests(unittest.TestCase):
    def test_only_supported_runtimes_and_safe_prefixes_are_accepted(self):
        for name in ("shell", "DOCKER", "", "docker;echo"):
            with self.subTest(name=name):
                with self.assertRaises(SmokeFailure):
                    ContainerRuntime(name)
        for prefix in ("", "../outside", "-option", "name;command", "A", "x" * 50):
            with self.subTest(prefix=prefix):
                with self.assertRaises(SmokeFailure):
                    ContainerRuntime("docker", container_prefix=prefix)

    def test_auto_selection_is_platform_and_runtime_availability_based(self):
        for platform, available, expected in (
            ("darwin", "/usr/bin/container", "apple"),
            ("darwin", None, "docker"), ("linux", "/usr/bin/container", "docker"),
        ):
            with self.subTest(platform=platform, available=available):
                with patch("runtime.sys.platform", platform), patch("runtime.shutil.which", return_value=available):
                    self.assertEqual(ContainerRuntime().name, expected)

    def test_gateway_process_is_nonroot_and_each_reader_role_is_fixed(self):
        with patch("runtime.McpClient") as factory:
            for runtime in ("docker", "apple", "podman"):
                for profile, role in ROLES.items():
                    with self.subTest(runtime=runtime, profile=profile):
                        client = factory.return_value
                        client.reset_mock()
                        client.request.return_value = {
                            "protocolVersion": "2024-11-05", "serverInfo": {"name": "postgresem"},
                        }
                        result = ContainerRuntime(runtime).client(profile)
                        self.assertIs(result, client)
                        command, timeout = factory.call_args.args
                        self.assertEqual(timeout, 60)
                        self.assertIn("--user", command)
                        self.assertEqual(command[command.index("--user") + 1], "postgresem")
                        self.assertIn(f"POSTGRESEM_DEMO_READER={role}", command)
                        self.assertIn("POSTGRESEM_MCP_DB_ROLE_ENV=POSTGRESEM_DEMO_READER", command)
                        self.assertEqual(command[-3:], ["postgresem", "mcp", "serve"])
                        if profile != "analyst":
                            self.assertEqual(command[-6:-3], ["env", "-u", "POSTGRESEM_MCP_MUTATION_URL_ENV"])
                        else:
                            self.assertNotIn("POSTGRESEM_MCP_MUTATION_URL_ENV", command)
                        client.request.assert_called_once()
                        self.assertEqual(client.request.call_args.args[0], "initialize")
                        client.notify.assert_called_once_with("notifications/initialized", {})
                        client.abort.assert_not_called()

    def test_unknown_role_is_rejected_before_starting_a_client(self):
        with patch("runtime.McpClient") as factory:
            for profile in ("postgres", "admin", "tenant_c", "postgresem_analyst"):
                with self.subTest(profile=profile):
                    with self.assertRaises(KeyError):
                        ContainerRuntime("docker").client(profile)
            factory.assert_not_called()

    def test_failed_initialization_aborts_client(self):
        with patch("runtime.McpClient") as factory:
            client = factory.return_value
            for response in (
                {}, {"protocolVersion": "other", "serverInfo": {"name": "postgresem"}},
                {"protocolVersion": "2024-11-05", "serverInfo": {"name": "unknown"}},
            ):
                with self.subTest(response=response):
                    client.reset_mock()
                    client.request.return_value = response
                    with self.assertRaises(SmokeFailure):
                        ContainerRuntime("docker").client("analyst")
                    client.abort.assert_called_once_with()
                    client.notify.assert_not_called()
            client.reset_mock()
            client.request.side_effect = SmokeFailure("connection lost")
            with self.assertRaises(SmokeFailure):
                ContainerRuntime("docker").client("tenant_a")
            client.abort.assert_called_once_with()

    def test_qualification_containers_cannot_be_started_accidentally(self):
        with patch("runtime.subprocess.run") as run:
            with self.assertRaises(SmokeFailure):
                ContainerRuntime("podman", container_prefix="unit-qualification").start()
            run.assert_not_called()

    def test_custom_prefix_targets_only_qualification_containers(self):
        for runtime, binary in (("apple", "container"), ("podman", "podman")):
            with self.subTest(runtime=runtime):
                selected = ContainerRuntime(runtime, container_prefix="unit-qualification")
                command = selected.exec("db", ["psql", "--version"])
                self.assertEqual(command, [
                    binary, "exec", "-i", "unit-qualification-db", "psql", "--version",
                ])
                command = selected.exec("gateway", ["postgresem", "mcp", "serve"])
                self.assertIn("unit-qualification-gateway", command)
                self.assertEqual(command[command.index("--user") + 1], "postgresem")


class McpWriteTests(unittest.TestCase):
    def test_os_write_and_flush_failures_become_sanitized_smoke_failures(self):
        for stage in ("write", "flush"):
            for error in (OSError("private-stream-secret"), BrokenPipeError("private-stream-secret")):
                with self.subTest(stage=stage, error=type(error).__name__):
                    client = object.__new__(McpClient)
                    client.process = Mock()
                    client.process.poll.return_value = None
                    getattr(client.process.stdin, stage).side_effect = error
                    with self.assertRaises(SmokeFailure) as caught:
                        client._write({"jsonrpc": "2.0", "method": "tools/call", "id": 1})
                    self.assertNotIn("private", str(caught.exception))
                    self.assertIn("MCP connection", str(caught.exception))
                    if stage == "write":
                        client.process.stdin.flush.assert_not_called()

    def test_already_exited_mcp_is_rejected_before_writing(self):
        client = object.__new__(McpClient)
        client.process = Mock()
        client.process.poll.return_value = 1
        client.process.returncode = 1
        with self.assertRaises(SmokeFailure):
            client._write({"jsonrpc": "2.0", "id": 1})
        client.process.stdin.write.assert_not_called()


class ProbeTests(unittest.TestCase):
    def setUp(self):
        patcher = patch("runtime.subprocess.run")
        self.run = patcher.start()
        self.addCleanup(patcher.stop)
        self.runtime = Mock()
        self.runtime.exec.return_value = ["unit-runtime", "exec", "db"]
        self.probe = DatabaseProbe(self.runtime)
        self.response("1.00")

    def response(self, data, *, role=ROLES["analyst"], read_only="on"):
        self.run.return_value = Mock(
            returncode=0, stdout=json.dumps({"role": role, "read_only": read_only, "data": data}),
            stderr="",
        )

    def test_probe_uses_runtime_login_read_only_transaction_fixed_role_and_timeouts(self):
        secret = "unit-secret-never-place-in-argv"
        with patch.dict(os.environ, {"POSTGRESEM_RUNTIME_PASSWORD": secret}):
            result = self.probe.baseline("recognized-revenue", "B")
        self.assertEqual(result["role"], ROLES["analyst"])
        service, command = self.runtime.exec.call_args.args
        self.assertEqual(service, "db")
        self.assertEqual(command[:2], ["sh", "-ec"])
        script = command[2]
        self.assertIn('PGPASSWORD="$POSTGRESEM_RUNTIME_PASSWORD"', script)
        self.assertIn("-U postgresem_runtime", script)
        self.assertNotIn("-U postgres ", script)
        self.assertIn("PGCONNECT_TIMEOUT=5", script)
        self.assertIn("-XAtq -v ON_ERROR_STOP=1", script)
        self.assertNotIn(secret, repr(self.runtime.exec.call_args))
        call = self.run.call_args
        self.assertEqual(call.args[0], ["unit-runtime", "exec", "db"])
        self.assertNotIn("shell", call.kwargs)
        self.assertTrue(call.kwargs["capture_output"])
        self.assertTrue(call.kwargs["text"])
        self.assertEqual(call.kwargs["timeout"], 20)
        sql = call.kwargs["input"]
        self.assertIn("BEGIN ISOLATION LEVEL REPEATABLE READ READ ONLY;", sql)
        self.assertIn('SET LOCAL ROLE "postgresem_analyst";', sql)
        self.assertIn("SET LOCAL search_path = pg_catalog;", sql)
        self.assertIn("SET LOCAL statement_timeout = '5s';", sql)
        self.assertIn("SET LOCAL lock_timeout = '2s';", sql)
        self.assertIn(SCENARIOS["recognized-revenue"]["baseline"]["B"], sql)
        self.assertIn("ROLLBACK;", sql)
        self.assertNotIn(secret, sql)
        self.assertNotIn("SET ROLE postgres", sql)

    def test_every_baseline_is_selected_from_fixed_candidates(self):
        with patch.object(self.probe, "_read", return_value={"data": "1.00"}) as read:
            for scenario_id, scenario in SCENARIOS.items():
                for choice, sql in scenario["baseline"].items():
                    with self.subTest(scenario=scenario_id, choice=choice):
                        self.probe.baseline(scenario_id, choice)
                        read.assert_called_with(sql)
                        self.assertTrue(sql.startswith("SELECT "))
            read.reset_mock()
            for scenario_id, choice in (
                ("SELECT 1", "A"), ("recognized-revenue", "DROP TABLE orders"),
                ("unknown", "A"), ("recognized-revenue", "C"),
            ):
                with self.subTest(scenario=scenario_id, choice=choice):
                    with self.assertRaises(KeyError):
                        self.probe.baseline(scenario_id, choice)
            read.assert_not_called()
        self.run.assert_not_called()

    def test_snapshot_sql_is_constant_and_bounded_to_demonstration_identities(self):
        self.assertTrue(SNAPSHOT_SQL.lstrip().startswith("SELECT "))
        for external_id in ORDER_IDS:
            self.assertIn("'" + external_id + "'", SNAPSHOT_SQL)
        self.assertIn("subscription_id IN (1, 2, 3)", SNAPSHOT_SQL)
        self.assertIn("LIMIT 101", SNAPSHOT_SQL)
        self.response(source_rows())
        self.probe.snapshot()
        self.assertIn(SNAPSHOT_SQL, self.run.call_args.kwargs["input"])

    def test_tenant_probe_uses_only_fixed_login_roles_and_fixture_scope(self):
        for profile in ("tenant_a", "tenant_b"):
            with self.subTest(profile=profile):
                self.response("250.00", role=ROLES[profile])
                self.probe.tenant(profile)
                sql = self.run.call_args.kwargs["input"]
                self.assertIn(f'SET LOCAL ROLE "{ROLES[profile]}";', sql)
                self.assertIn("FROM rls_fixture.orders", sql)
                self.assertIn("('fixture-a-1', 'fixture-a-2', 'fixture-b-1')", sql)
                self.assertNotIn("row_security = off", sql)
        self.run.reset_mock()
        with self.assertRaises(KeyError):
            self.probe.tenant("postgres")
        self.run.assert_not_called()

    def test_read_only_and_role_attestations_are_required(self):
        bad = [
            {"role": "postgres", "read_only": "on", "data": "1"},
            {"role": ROLES["analyst"], "read_only": "off", "data": "1"},
            {"role": ROLES["analyst"], "read_only": True, "data": "1"},
            {"role": ROLES["analyst"], "read_only": "on"},
            {}, [], None,
        ]
        for value in bad:
            with self.subTest(value=value):
                self.run.return_value.stdout = json.dumps(value)
                with self.assertRaisesRegex(SmokeFailure, "authority mismatch"):
                    self.probe.baseline("recognized-revenue", "A")

    def test_process_and_decode_failures_do_not_expose_sql_credentials_or_stderr(self):
        for error in (
            OSError("password=private-secret"),
            subprocess.TimeoutExpired(["private-command"], 20, output="private-output"),
        ):
            with self.subTest(error=type(error).__name__):
                self.run.side_effect = error
                with self.assertRaises(SmokeFailure) as caught:
                    self.probe.baseline("recognized-revenue", "A")
                self.assertNotIn("private", str(caught.exception))
        self.run.side_effect = None
        for returncode, stdout in ((1, "private-secret"), (0, "private-invalid-json"), (0, "x" * 131_073)):
            with self.subTest(returncode=returncode, length=len(stdout)):
                self.run.return_value = Mock(returncode=returncode, stdout=stdout, stderr="private-password")
                with self.assertRaises(SmokeFailure) as caught:
                    self.probe.baseline("recognized-revenue", "A")
                self.assertNotIn("private", str(caught.exception))

    def test_snapshot_fingerprint_is_deterministic_and_changes_with_source(self):
        source = source_rows()
        self.response(source)
        first = self.probe.snapshot()
        self.response({key: source[key] for key in reversed(source)})
        reordered_keys = self.probe.snapshot()
        self.assertEqual(first["fingerprint"], reordered_keys["fingerprint"])
        self.assertEqual(len(first["fingerprint"]), 64)
        int(first["fingerprint"], 16)
        source["items"].append({
            "order_item_id": 2, "order_id": 1, "sku": "SKU-RED", "quantity": 1,
        })
        self.response(source)
        self.assertNotEqual(first["fingerprint"], self.probe.snapshot()["fingerprint"])
        self.assertNotIn("fingerprint", source)

    def test_snapshot_rejects_missing_or_unbounded_fixture_shapes(self):
        variants = [None, {}, {"orders": [], "items": [], "subscriptions": []}]
        for field, value in (
            ("orders", []), ("orders", source_rows()["orders"] * 2),
            ("items", []), ("items", [{}] * 101),
            ("subscriptions", []), ("subscriptions", [{}] * 4), ("items", None),
        ):
            source = source_rows()
            source[field] = value
            variants.append(source)
        missing_order = source_rows()
        missing_order["orders"][0]["external_id"] = "not-a-fixture"
        variants.append(missing_order)
        duplicate_subscription = source_rows()
        duplicate_subscription["subscriptions"][0]["subscription_id"] = 2
        variants.append(duplicate_subscription)
        for source in variants:
            with self.subTest(source=source):
                self.response(deepcopy(source))
                with self.assertRaises(SmokeFailure):
                    self.probe.snapshot()


if __name__ == "__main__":
    unittest.main()
