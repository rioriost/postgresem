.PHONY: doctor dev-up dev-down mcp docker-up docker-down docker-mcp web-demo backup verify-backup upgrade-local report-beta report-operations fmt check test test-contracts test-install test-web-demo test-reference-comparison test-db test-execution test-mcp test-mutation test-performance test-recovery test-rc-workflow preview-check beta-check rc-check

doctor:
	cargo run --quiet -p postgresem -- doctor

dev-up:
	@test -f .env || (echo "copy .env.example to .env and set local passwords" >&2; exit 1)
	container-compose up --env-file .env -d --build db migrate seed gateway

mcp:
	@test -f .env || (echo "copy .env.example to .env and set local passwords" >&2; exit 1)
	@container-compose up --env-file .env -d --build gateway </dev/null 1>&2
	@container exec -i --user postgresem postgresem-gateway postgresem mcp serve

docker-up:
	@test -f .env || (echo "copy .env.example to .env and set local passwords" >&2; exit 1)
	docker compose --env-file .env -f compose.yaml -f compose.linux.yaml \
		up --detach --build gateway

docker-mcp:
	@test -f .env || (echo "copy .env.example to .env and set local passwords" >&2; exit 1)
	@docker compose --env-file .env -f compose.yaml -f compose.linux.yaml \
		up --detach --build gateway </dev/null 1>&2
	@docker compose --env-file .env -f compose.yaml -f compose.linux.yaml \
		exec -T gateway postgresem mcp serve

docker-down:
	docker compose --env-file .env -f compose.yaml -f compose.linux.yaml down

web-demo:
	@test -f .env || (echo "copy .env.example to .env and set local passwords" >&2; exit 1)
	python3 examples/web_demo/server.py -- make mcp

backup:
	scripts/backup.sh $(BACKUP_ROOT)

verify-backup:
	@test -n "$(BACKUP_DIR)" || (echo "set BACKUP_DIR to a backup directory" >&2; exit 1)
	scripts/verify-backup.sh "$(BACKUP_DIR)"

upgrade-local:
	@test -n "$(BACKUP_DIR)" || (echo "set BACKUP_DIR to a verified pre-upgrade backup directory" >&2; exit 1)
	scripts/upgrade-local.sh "$(BACKUP_DIR)"

report-beta:
	@test -f .env || (echo "copy .env.example to .env and set local passwords" >&2; exit 1)
	@container-compose up --env-file .env -d --build gateway </dev/null 1>&2
	@container exec --user postgresem postgresem-gateway postgresem report beta \
		--audit-database-url-env MCP_AUDIT_DATABASE_URL

report-operations:
	@test -f .env || (echo "copy .env.example to .env and set local passwords" >&2; exit 1)
	@container-compose up --env-file .env -d --build gateway </dev/null 1>&2
	@container exec --user postgresem postgresem-gateway postgresem report operations \
		--audit-database-url-env MCP_AUDIT_DATABASE_URL

dev-down:
	container-compose down --env-file .env

fmt:
	cargo fmt --all --check

check:
	cargo clippy --workspace --all-targets --all-features -- -D warnings

test:
	cargo test --workspace --all-features

test-contracts:
	@output="$$(mktemp)"; \
	trap 'rm -f "$$output"' EXIT; \
	cargo run --quiet -p postgresem -- contract show >"$$output"; \
	python3 tests/contracts/verify.py --actual-manifest "$$output"

test-install:
	tests/install/security.sh
	tests/install/success.sh

test-web-demo:
	python3 examples/web_demo/test_server.py

test-reference-comparison:
	tests/reference-comparison/run.sh

test-db:
	@test -f .env || (echo "copy .env.example to .env and set local passwords" >&2; exit 1)
	container-compose up --env-file .env -d integration-test
	@attempt=0; \
	while [ $$attempt -lt 30 ]; do \
		logs="$$(container logs postgresem-integration-test 2>&1)"; \
		if printf '%s\n' "$$logs" | grep -q "all database integration checks passed"; then \
			printf '%s\n' "$$logs"; \
			exit 0; \
		fi; \
		if container inspect postgresem-integration-test 2>/dev/null | grep -q '"state" : "stopped"'; then \
			printf '%s\n' "$$logs" >&2; \
			exit 1; \
		fi; \
		attempt=$$((attempt + 1)); \
		sleep 1; \
	done; \
	echo "integration test timed out" >&2; \
	exit 1

test-execution:
	@test -f .env || (echo "copy .env.example to .env and set local passwords" >&2; exit 1)
	container-compose up --env-file .env -d --build execution-test
	@attempt=0; \
	while [ $$attempt -lt 60 ]; do \
		logs="$$(container logs postgresem-execution-test 2>&1)"; \
		if printf '%s\n' "$$logs" | grep -q "guarded execution integration checks passed"; then \
			printf '%s\n' "$$logs"; \
			exit 0; \
		fi; \
		if container inspect postgresem-execution-test 2>/dev/null | grep -q '"state" : "stopped"'; then \
			printf '%s\n' "$$logs" >&2; \
			exit 1; \
		fi; \
		attempt=$$((attempt + 1)); \
		sleep 1; \
	done; \
	echo "execution integration test timed out" >&2; \
	exit 1

test-mcp:
	@test -f .env || (echo "copy .env.example to .env and set local passwords" >&2; exit 1)
	container-compose up --env-file .env -d --build mcp-test
	@attempt=0; \
	while [ $$attempt -lt 60 ]; do \
		logs="$$(container logs postgresem-mcp-test 2>&1)"; \
		if printf '%s\n' "$$logs" | grep -q "MCP stdio integration checks passed"; then \
			printf '%s\n' "$$logs"; \
			exit 0; \
		fi; \
		if container inspect postgresem-mcp-test 2>/dev/null | grep -q '"state" : "stopped"'; then \
			printf '%s\n' "$$logs" >&2; \
			exit 1; \
		fi; \
		attempt=$$((attempt + 1)); \
		sleep 1; \
	done; \
	echo "MCP integration test timed out" >&2; \
	exit 1

test-mutation:
	@test -f .env || (echo "copy .env.example to .env and set local passwords" >&2; exit 1)
	container-compose up --env-file .env -d --build mutation-test
	@attempt=0; \
	while [ $$attempt -lt 60 ]; do \
		logs="$$(container logs postgresem-mutation-test 2>&1)"; \
		if printf '%s\n' "$$logs" | grep -q "governed mutation integration checks passed"; then \
			printf '%s\n' "$$logs"; \
			exit 0; \
		fi; \
		if container inspect postgresem-mutation-test 2>/dev/null | grep -q '"state" : "stopped"'; then \
			printf '%s\n' "$$logs" >&2; \
			exit 1; \
		fi; \
		attempt=$$((attempt + 1)); \
		sleep 1; \
	done; \
	echo "mutation integration test timed out" >&2; \
	exit 1

test-performance:
	@test -f .env || (echo "copy .env.example to .env and set local passwords" >&2; exit 1)
	container-compose up --env-file .env -d --build performance-test
	@attempt=0; \
	while [ $$attempt -lt 120 ]; do \
		logs="$$(container logs postgresem-performance-test 2>&1)"; \
		if printf '%s\n' "$$logs" | grep -q "M10 scale baseline checks passed"; then \
			printf '%s\n' "$$logs"; \
			exit 0; \
		fi; \
		if container inspect postgresem-performance-test 2>/dev/null | grep -q '"state" : "stopped"'; then \
			printf '%s\n' "$$logs" >&2; \
			exit 1; \
		fi; \
		attempt=$$((attempt + 1)); \
		sleep 1; \
	done; \
	echo "M10 scale baseline test timed out" >&2; \
	exit 1

test-recovery:
	@test -f .env || (echo "copy .env.example to .env and set local passwords" >&2; exit 1)
	container-compose up --env-file .env -d --build recovery-test
	@attempt=0; \
	while [ $$attempt -lt 180 ]; do \
		logs="$$(container logs postgresem-recovery-test 2>&1)"; \
		if printf '%s\n' "$$logs" | grep -q "M11 N-1 migration and recovery checks passed"; then \
			printf '%s\n' "$$logs"; \
			exit 0; \
		fi; \
		if container inspect postgresem-recovery-test 2>/dev/null | grep -q '"state" : "stopped"'; then \
			printf '%s\n' "$$logs" >&2; \
			exit 1; \
		fi; \
		attempt=$$((attempt + 1)); \
		sleep 1; \
	done; \
	echo "recovery integration test timed out" >&2; \
	exit 1

test-rc-workflow:
	@test -f .env || (echo "copy .env.example to .env and set local passwords" >&2; exit 1)
	container-compose up --env-file .env -d --build rc-test
	@attempt=0; \
	while [ $$attempt -lt 120 ]; do \
		logs="$$(container logs postgresem-rc-test 2>&1)"; \
		if printf '%s\n' "$$logs" | grep -q "M11 release-candidate query and ingestion workflow passed"; then \
			printf '%s\n' "$$logs"; \
			exit 0; \
		fi; \
		if container inspect postgresem-rc-test 2>/dev/null | grep -q '"state" : "stopped"'; then \
			printf '%s\n' "$$logs" >&2; \
			exit 1; \
		fi; \
		attempt=$$((attempt + 1)); \
		sleep 1; \
	done; \
	echo "M11 release-candidate workflow test timed out" >&2; \
	exit 1

preview-check: fmt check test test-contracts test-reference-comparison test-db test-execution test-mcp test-mutation test-performance

beta-check: preview-check test-install test-web-demo test-recovery

rc-check: beta-check test-rc-workflow
