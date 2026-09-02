# Linux containers

Linux deployments use the repository `Dockerfile`. It is intentionally kept
byte-equivalent to `Containerfile`: `Dockerfile` is the standard Docker and
Podman entry point, while `Containerfile` remains the Apple Container entry
point. CI rejects drift between them.

These are local evaluation paths, not a production orchestration or
availability claim. Use fictional data and separate disposable credentials.

## Docker Compose

Copy `.env.example` to `.env`, replace every placeholder, then run:

```sh
make docker-up
docker compose \
  --env-file .env \
  -f compose.yaml \
  -f compose.linux.yaml \
  exec -T gateway postgresem --version
python3 examples/commerce/mcp_smoke.py -- make docker-mcp
```

`compose.linux.yaml` replaces the Apple Container root/gosu startup workaround.
The Linux gateway container and attached MCP process both run as UID/GID
`10001`.

Stop the stack while preserving PostgreSQL data:

```sh
make docker-down
```

The same Compose files are valid OCI Compose input. Podman installations may
use `podman compose` when a compatible Compose provider is configured, but the
repository's provider-independent Podman path is Quadlet.

## Rootless Podman Quadlet

Requirements:

- Podman 4.9 or newer with Quadlet and rootless systemd user services;
- lingering enabled when the services must survive logout;
- the repository checked out as the user that owns the services.

Build the local image and install the units:

```sh
podman build --file Dockerfile --tag localhost/postgresem:latest .
deploy/quadlet/install.sh
${EDITOR:-vi} "${HOME}/.config/postgresem/postgresem-database.env"
${EDITOR:-vi} "${HOME}/.config/postgresem/postgresem-gateway.env"
systemctl --user daemon-reload
systemctl --user start postgresem-gateway.service
```

Starting the gateway pulls PostgreSQL 18, creates a private network and named
volume, waits for database health, applies forward migrations, publishes the
idempotent commerce fixture, and starts the gateway as UID/GID `10001`.

Check the units and attach the stdio MCP server:

```sh
systemctl --user status \
  postgresem-db.service \
  postgresem-migrate.service \
  postgresem-seed.service \
  postgresem-gateway.service

podman exec -i postgresem-gateway postgresem mcp serve
```

Stop the stack without deleting the named database volume:

```sh
systemctl --user stop postgresem-gateway.service
systemctl --user stop postgresem-seed.service
systemctl --user stop postgresem-migrate.service
systemctl --user stop postgresem-db.service
```

`deploy/quadlet/install.sh` copies only migrations and fictional fixtures into
`~/.local/share/postgresem`; it does not copy `.env`, database dumps, or other
repository content. The generated environment files are mode `0600`. Database
administration credentials are isolated from the gateway environment, which
contains only the runtime, audit-writer, and mutation-runtime credentials.
Replace all placeholder passwords before starting the units.
