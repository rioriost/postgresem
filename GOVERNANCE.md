# Governance

## Current ownership

The project currently has one repository maintainer, `@rioriost`. This
single-maintainer state is explicit and remains a sustainability risk; it is
not hidden behind an implied foundation or team. The repository, signed
release automation, append-only migrations, ADRs, contract manifests, and
operator procedures provide continuity artifacts, but they do not replace a
second human maintainer.

The maintainer owns release decisions, contract-version decisions, security
coordination, issue triage, and acceptance of external evidence. Automated
agents and CI may provide analysis and reproducible evidence but cannot approve
their own security review, production pilot, or governance status.

## Decision process

- Public contract, authorization, migration, audit, or trust-boundary changes
  require an accepted ADR.
- Stable contract changes require compatibility classification, tests,
  documentation, and an intentional refresh of `contracts/stable-v1.json`.
- `contracts/rc-v1.json` is immutable historical release-candidate evidence.
- Applied migrations are append-only.
- Security-sensitive changes require private coordination until disclosure is
  safe.
- Release tags are created only from a clean commit whose required CI and
  release gates pass. `v1.0.0` additionally requires accepted external
  evidence in `contracts/release-evidence-v1.json`.

## Contributions and maintainership

Contributors retain credit under the repository license. A contributor may be
considered for maintainership after sustained review-quality contributions
across compiler correctness, PostgreSQL authorization, operations, and
security boundaries. Adding a maintainer requires a public governance change
that states permissions and responsibilities.

## Release cadence

There is no calendar SLA. Patch releases are driven by security or correctness
needs. Minor releases require an implementation-plan milestone and migration
or compatibility notes. The 1.0 release requires the M12 repository gates,
accepted independent review and field evidence, a current maintainer, and this
sustainability statement.
