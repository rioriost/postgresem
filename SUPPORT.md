# Support policy

## Release-candidate scope

`0.9.x` is the supported release-candidate line before 1.0. It is intended for
approved non-production pilots and release qualification, not for workloads
that require a production SLA, regulatory certification, or guaranteed RPO/RTO.

The repository supports:

- PostgreSQL 16, 17, and 18 for migrations and guarded query/mutation
  integration;
- native Linux amd64 and arm64 runtime images and binaries;
- macOS amd64 and arm64 command-line binaries;
- MCP stdio `2024-11-05` and authenticated loopback MCP HTTP `2026-07-28`;
- forward upgrades from the documented previous release and same-name restore
  rehearsal.

The exact contract versions are emitted by:

```sh
postgresem contract show
```

## Support windows

- The latest `0.9.x` release receives correctness and security fixes until 1.0
  is published.
- After 1.0, the final `0.9.x` release receives security fixes for 90 days.
- Older preview lines receive no fixes except when required to provide a safe
  upgrade path to the supported line.
- Support periods are best-effort open-source maintenance commitments, not
  response-time or resolution-time SLAs.

## Supported changes and assistance

Supported reports include reproducible defects in frozen public contracts,
supported PostgreSQL/platform combinations, release artifacts, installer
verification, upgrade/recovery fixtures, and documented operator workflows.

Production architecture, capacity planning, encryption/key management,
replication, backup retention, HA, RPO/RTO, identity-provider operation,
reverse-proxy operation, and regulatory controls remain operator-owned.

Report security issues through private vulnerability reporting as described in
[`SECURITY.md`](SECURITY.md). Use public issues for non-sensitive defects and
include the version, platform, PostgreSQL version, minimal fictional-data
reproduction, and expected/actual public output.
