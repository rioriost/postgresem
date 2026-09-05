# ADR 0020: Maintainer-approved release evidence exception for v1.0.0

- Status: Accepted
- Date: 2026-09-05
- Decision owner: `rioriost`
- Scope: the exact release tag `v1.0.0` only

## Context

ADR 0018 normally requires an independent external security review, a reviewed
container image, and two distinct non-fixture database pilots lasting at least
28 days. Those requirements have not been completed. The remotely supplied
automated source review and its post-fix evidence are useful evidence, but
they are not an independent third-party review and do not cover a container
image. No qualifying pilots were performed.

On 2026-09-05, project owner and maintainer `rioriost` expressly approved in
the release implementation conversation both skipping the impractical two
28-day pilots and accepting this automated source review/remediation as a
maintainer-approved source review for `v1.0.0`, waiving independent third-party
review and reviewed-container-image prerequisites. This ADR records that
human decision; repository automation does not make the risk-acceptance
decision.

## Decision

1. Supersede only the external-evidence prerequisites in ADR 0018 and the
   associated M5/M11/M12 release checklists for the exact tag `v1.0.0`.
   The ordinary independent-review, reviewed-image, and two-pilot gates remain
   the default for every later stable release. No later tag inherits this
   exception; changing its scope or source baseline requires a new explicit
   maintainer decision and review.
2. Accept the automated source report and post-fix evidence only for corrected
   source commit `c8a2ca7a6a635de975d8e8b2324b652ac037075c` and its reviewed
   `contracts/stable-v1.json` SHA-256
   `c3d58c9fd3670836da7f86c73c478dcee4a087601cb083a927e3f7617e4f18a2`.
   The immutable evidence is the
   [source review and remediation report](https://github.com/rioriost/postgresem/blob/2797160ee431ee12722d339e23def6d8c8e7fbd5/docs/security-reviews/2026-09-05-381fe57.md).
   Its original review of `381fe57` and original hashes remain historical;
   neither the report nor those identities are rewritten. ADR 0019's
   writer-role-bound reconciliation and JWK-only dependency decision remain
   unchanged.
3. Preserve the existing strict ordinary evidence shape. Add a separate strict
   accepted shape with these values:
   - `schema_version`: `"1"`; `release`: `"1.0.0"`; `status`: `"accepted"`.
   - `source_security_review.kind`: `"automated-source-review"`.
   - `source_security_review.evidence_url`: the immutable report URL above.
   - `source_security_review.reviewed_commit`: the corrected full commit above.
   - `source_security_review.reviewed_contract_digest`:
     `sha256:c3d58c9fd3670836da7f86c73c478dcee4a087601cb083a927e3f7617e4f18a2`.
   - `source_security_review.reviewed_image_digest`: `null`, because no image
     review was performed.
   - `source_security_review.retest_completed_at`: `"2026-09-05"`;
     `source_security_review.unresolved_p0_p1`: `0`, referring to the recorded
     source remediation/retest, not unmeasured field operation.
   - `field_pilots`: `[]`, because no pilots were completed.
   - `maintainer_exception.scope`: `"v1.0.0-only"`; `maintainer`: `"rioriost"`;
     `accepted_at`: `"2026-09-05"`.
   - `maintainer_exception.decision_url`: a genuine immutable, full
     commit-pinned GitHub URL for this ADR, registered after its policy commit
     exists. A relative link to this ADR is documentation, not a substitute
     for the required immutable URL in release evidence.
   - `maintainer_exception.waived_requirements`:
     `["independent_external_reviewer", "reviewed_container_image",
     "two_28_day_non_fixture_pilots"]`.
   Unknown, mixed, incomplete, or broadened exception records must fail closed.
4. Bind acceptance to the reviewed source, not to unreviewed release-gate
   changes. The gate loads and hashes the stable manifest from the corrected
   source commit and requires the current public contract to match that
   reviewed manifest. Generated stable inventory updates for the permitted
   release-only changes do not make their new hashes independently reviewed.
   Reject every subsequent file change except the exact validator-enforced
   allowlist: release-evidence validator/tests, generated stable inventory,
   release-evidence record, changelog, specifically enumerated policy/release
   documents, and the existing documentation-only evidence binding.
   This is an exact path allowlist, not blanket permission for a directory.
   Reviewed source, dependency, migration, packaging, and CI changes are not
   allowed under this acceptance; they require a new decision/review.
5. Both the review and decision URLs must identify genuine immutable GitHub
   commits that are ancestors of the release commit. Their document contents
   must be unchanged in the release tree. Do not fabricate dates, pilot data,
   reviewer independence, image digests, commit hashes, or immutable URLs to
   make validation succeed.
6. Do not change runtime PostgreSQL GRANT/RLS enforcement, the no-raw-SQL public
   contract, audit lifecycle, mutation controls, CI, native Linux qualification,
   installer trust, signing, SBOM, or provenance requirements. Known unresolved
   P0/P1 source findings remain release blockers. Potential future supply-chain
   findings remain subject to continuing monitoring and vulnerability response.

## Consequences and accepted limitations

- Source review status is **accepted under the v1.0.0 maintainer exception**,
  not independently or externally reviewed.
- Independent third-party review and reviewed-image prerequisites are
  **waived for v1.0.0; not completed**. Build, smoke, signing, and provenance
  checks are still required and are not evidence of an image security review.
- Both field pilots are **waived for v1.0.0; not completed**. Field-period
  P0/P1 outcomes are **not measured / accepted limitation**, not zero defects.
- Real-world adoption, long-duration operational behavior, and independent
  security assurance remain unestablished. Historical M5 goals and incomplete
  evidence are not retrospectively marked complete.
- Approval of this exception does not mean that `v1.0.0` has been published.
  Publication still requires valid immutable evidence registration, all
  unchanged technical qualification gates, and successful release automation.
- See the [beta checklist](../beta-checklist.md),
  [ordinary external evidence process](../m5-external-evidence.md), and
  [stable release checklist](../m12-stable-release-checklist.md).
