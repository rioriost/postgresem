.PHONY: doctor dev-up dev-down mcp fmt check test test-db test-execution test-mcp

doctor:
	cargo run --quiet -p postgresem -- doctor

dev-up:
	@test -f .env || (echo "copy .env.example to .env and set local passwords" >&2; exit 1)
	container-compose up --env-file .env -d --build db migrate seed gateway

mcp:
	@test -f .env || (echo "copy .env.example to .env and set local passwords" >&2; exit 1)
	@container-compose up --env-file .env -d --build gateway </dev/null 1>&2
	@container exec -i postgresem-gateway postgresem mcp serve

dev-down:
	container-compose down --env-file .env

fmt:
	cargo fmt --all --check

check:
	cargo clippy --workspace --all-targets --all-features -- -D warnings

test:
	cargo test --workspace --all-features

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
