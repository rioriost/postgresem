# ADR 0018: Stable 1.0 contract and external release evidence

- Status: Accepted
- Date: 2026-09-04

## Context

M11 froze the `0.9.0` release-candidate boundary but deliberately kept the
independent security review and two 28-day non-fixture pilots outside
repository self-certification. M12 must promote the reviewed contract to a
stable 1.0 boundary without rewriting the historical RC record or allowing a
tag to imply evidence that has not been accepted.

## Decision

1. `contracts/rc-v1.json` remains immutable historical evidence. The stable
   contract is `contracts/stable-v1.json`, emitted by
   `postgresem contract show` as release `1.0.0` with status `stable`.
2. The stable contract retains the RC versions and migrations `0001` through
   `0010`. No raw SQL, arbitrary DML, request-selected authority, or
   non-PostgreSQL execution surface is added during promotion.
3. Stable contract-bearing changes require compatibility classification,
   tests, documentation, and an intentional stable-manifest refresh. Breaking
   1.x changes require a versioned replacement and the deprecation process,
   except where immediate rejection is required to close a security,
   correctness, or privacy defect.
4. `contracts/release-evidence-v1.json` is the machine-readable release gate
   for the independent security review and exactly two distinct 28-day field
   pilots. The review binds an ancestor commit and the exact stable-manifest
   digest; each pilot binds a full commit or image digest. Pending values are
   explicit and cannot pass the `v1.0.0` workflow.
5. The release workflow validates accepted evidence for `v1.0.0` and every
   later non-prerelease 1.x tag. Development CI can validate the pending
   evidence document without treating it as release approval.
6. Maintainers accept external evidence. CI and automated agents validate
   shape and consistency but cannot assert reviewer independence, field use,
   or governance sustainability.

## Consequences

- Repository-controlled M12 implementation can be complete while publication
  of `v1.0.0` remains fail-closed on external evidence.
- The source version can represent the prepared stable contract without
  claiming that a formal release exists.
- Stable compatibility is reviewable and deterministic, while RC provenance
  remains preserved.
- PostgreSQL remains the semantic, execution, transaction, GRANT, and RLS
  authority.
