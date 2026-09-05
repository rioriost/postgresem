# ADR 0019: Role-bound reconciliation and JWK-only dependencies

- Status: Accepted
- Date: 2026-09-05

## Context

The pre-release review of commit `381fe57` found that principal-v1
reconciliation checked stable authority but not the stored writer role.
ADR 0015 already requires both. The dependency audit also reported
RUSTSEC-2026-0009 in `time 0.3.36`. No runtime path to the affected RFC2822
parser was identified; the application uses JWK keys, not PEM decoding,
and has no direct use of the pinned `time` dependency.

## Decision

1. Migration `0011_mutation_reconcile_writer_role` adds the configured writer
   role to the internal lookup function. The shared executor supplies that
   role for CLI, stdio, and HTTP reconciliation; request bodies cannot set it.
2. Choose the principal-v1 record before checking its writer role. A role
   mismatch returns no state and must not fall back to a matching legacy row.
   Legacy-only lookup requires its existing authority hash and stored role.
   Stable-authority lookup continues to work after audit-HMAC rotation.
3. Remove the four-argument overload rather than retaining an unguarded
   compatibility path. Apply all migrations before deploying the new binary.
   New reconciliation against an old schema, and old reconciliation against
   the new schema, fail closed. Query execution and mutation claim/replay
   retain their existing interfaces and permissions.
4. Keep the function SECURITY DEFINER with `pg_catalog` search path,
   qualified relations, PUBLIC execution revoked, and only the existing
   auditor grant. Do not broaden database roles, table grants, or RLS.
5. Disable jsonwebtoken's unused default PEM feature and remove the unused
   direct `time` pin. This removes `simple_asn1`, `time`, and the vulnerable
   parser from the dependency graph without raising the declared Rust MSRV
   or changing the supported JWK authentication surface.
6. Classify this as stricter rejection restoring an accepted security
   boundary, permitted by ADR 0018 and the deprecation policy. Refresh the
   stable manifest intentionally; preserve the historical RC manifest and
   the original review's commit/digests. This correction does not establish
   external review acceptance.

## Consequences

Reconciliation returns the same empty state for an absent key, foreign
authority, or remapped writer role. It still returns only mutation metadata,
not stored result values. Old binaries cannot use the removed reconciliation
overload after migration; rollback must not reintroduce that overload.

The new stable-manifest digest needs its own review/evidence binding before
release acceptance. The pending external-evidence gate remains unchanged.
