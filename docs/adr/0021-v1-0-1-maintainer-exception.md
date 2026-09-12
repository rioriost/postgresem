# ADR 0021: Maintainer-approved evidence exception for v1.0.1

- Status: Accepted
- Date: 2026-09-12
- Decision owner: `rioriost`
- Scope: the exact release tag `v1.0.1` only

## Context and decision

After inspecting the macOS 27 release draft and being asked explicitly whether
v1.0.1 should waive the same independent external review, reviewed container
image, and two 28-day non-fixture pilots as v1.0.0, the project maintainer
replied: 「免除を適用します。」 This records that new human decision.

Supersede only those three external-evidence prerequisites of ADR 0018 for
v1.0.1. They are waived, not completed. No image review or field pilot is
claimed. Automated source review does not establish independence. ADR 0020
and its v1.0.0 evidence remain historical and unchanged.

Accept the automated delta review in
`docs/security-reviews/2026-09-12-v1-0-1-delta.md`, binding source commit
`d56b8fffffde89961f71562a42d2255fb913feb6` and stable-manifest SHA-256
`2f43961ee31df5bb12f89aa974b6d625b8821756dfbc07ac11e6554ee8e53015`.
Runtime source, dependencies, schemas, migrations and public behavior are
unchanged from v1.0.0 except the release label. The reviewed packaging change
imports digest-pinned, signed and notarized macOS 27 executables; it retains
native smoke tests, tagged metadata packaging, Linux qualification, checksum
signatures, container SBOM/provenance and image signing.

The evidence validator must bind both review and decision to real immutable
ancestor commits and unchanged document contents. Only these subsequent
files may differ from the reviewed source:

- `CHANGELOG.md`
- `contracts/release-evidence-v1.json`
- `contracts/stable-v1.json` (only the evidence-validator artifact digest)
- `tests/contracts/verify_release_evidence.py`
- `tests/contracts/test_verify_release_evidence.py`
- `docs/adr/0021-v1-0-1-maintainer-exception.md`
- `docs/security-reviews/2026-09-12-v1-0-1-delta.md`
- `docs/beta-checklist.md`
- `docs/releases/1.0.1-macos27.md`

No later tag inherits this exception. Runtime, dependency, CI, packaging,
input-binary identity or public-contract changes beyond the reviewed baseline
require another decision and review. Preserve fail-closed checks, GRANT/RLS,
no-raw-SQL, audit and mutation controls. Known unresolved P0/P1 findings remain
blockers. Approval does not itself establish publication or completed tests.
