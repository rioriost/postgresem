# ADR 0012: Linux amd64/arm64 runtime and release evidence

- Status: Accepted
- Date: 2026-09-01

## Context

Cross-compilation and a multi-architecture image manifest do not prove that a
released artifact starts or behaves correctly on its target architecture. M6
requires Linux amd64 and arm64 as supported runtime targets while retaining the
macOS arm64 maintainer path.

## Decision

1. Linux amd64 and arm64 are release-blocking runtime targets for the binary
   archive and OCI image.
2. Native GitHub-hosted runners execute the built release binary on both
   architectures. The gate covers version/doctor startup, explicit TLS-mode
   rejection, archive layout, and installer-compatible packaging.
3. Native runners build and start the runtime image on both architectures.
   Database contract coverage remains the PostgreSQL 16-18 matrix; a separate
   architecture smoke matrix covers catalog/query and governed mutation on
   PostgreSQL 18.
4. Release jobs test packaged archives before upload and test each
   architecture-specific image before publishing the multi-architecture
   manifest.
5. Evidence records the runner architecture, binary target, image platform,
   PostgreSQL version, and smoke commands. Artifact generation alone is not
   accepted as runtime evidence.

## Consequences

The PostgreSQL-version and CPU-architecture matrices remain separate to avoid
an unnecessary full cross product. Linux support claims require native
execution on both architectures.

