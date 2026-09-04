# Support policy

## Stable scope

`1.0.x` is the stable contract line once `v1.0.0` is formally published.
The source tree may be prepared at version `1.0.0` before publication, but
formal support starts only from the signed release. Stable means the documented
interfaces follow the compatibility policy; it is not a commercial SLA,
regulatory certification, or guaranteed RPO/RTO.

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

- The latest `1.0.x` release receives correctness and security fixes until the
  next supported stable minor is published.
- Each superseded stable minor receives security fixes for 12 months after the
  next stable minor, or 18 months after its initial release, whichever is
  later.
- The final `0.9.x` release receives security fixes for 90 days after 1.0.
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
