# M5 external evidence collection

Repository tests establish implementation behavior, but they cannot establish
independent review or real-world operation. M5 remains incomplete until both
external gates below are supported by reviewable evidence.

## Gate 1: two non-fixture databases for at least 28 days

Each database needs its own evidence record. The two records may come from the
same organization, but they must describe distinct non-fixture PostgreSQL
databases and real operator or user activity. Fixtures, maintainer-only runs,
CI, and automated demo traffic do not count.

For each deployment:

1. Assign a stable, non-sensitive pseudonym.
2. Record the PostgreSQL version and immutable postgresem full commit or image
   digest. A release tag may be described in the evidence, but the
   machine-readable gate records its resolved commit or image digest rather
   than trusting the tag name alone.
3. Record a UTC start date before the first qualifying audited query.
4. Capture at least four weekly checkpoints covering at least 28 continuous
   days.
5. At each checkpoint, retain a restricted copy of:
   - `postgresem report beta --window-hours 168`;
   - process availability and PostgreSQL health observations;
   - backup age and any restore exercise;
   - correctness checks against independently known answers;
   - explicit rejection, timeout, truncation, and incomplete-audit outcomes;
   - user-confirmed repeated use and value.
6. At the end of the period, run
   `postgresem report beta --window-hours 672` and record any coverage gaps.
7. Record every suspected P0/P1 security or correctness defect through the
   private security or incident process before publishing a redacted summary.
8. Submit one
   [M5 field evidence issue](https://github.com/rioriost/postgresem/issues/new?template=m5_field_evidence.yml)
   per database.

The aggregate report deliberately omits availability, backup freshness,
database health, and end-user outcomes. Those observations must be collected
separately. A zero-query week is not evidence of repeated operation.

### Local evidence handling

Store evidence outside the source repository with restrictive permissions.
For example:

```sh
umask 077
mkdir -p evidence/week-1
postgresem report beta --window-hours 168 \
  > evidence/week-1/beta-report.json
shasum -a 256 evidence/week-1/beta-report.json \
  > evidence/week-1/SHA256SUMS
```

Do not commit reports, credentials, connection strings, logs, SQL or LSQ text,
private object names, query results, or source data. The public issue should
contain stable pseudonyms and redacted aggregate conclusions only.

## Gate 2: independent security review

The reviewer must be independent from implementation of the reviewed changes.
Automated scanning, maintainer self-review, and an AI-assisted pre-review are
useful preparation but do not satisfy this gate.

The review record must identify:

- reviewer and organization, including relevant project relationships;
- immutable commit, release tag, and image digest in scope;
- SHA-256 digest of the reviewed `contracts/stable-v1.json`;
- review and retest dates;
- trust boundaries, code/configuration scope, methodology, and exclusions;
- findings and severity;
- private remediation references and public commits/releases when disclosure
  is safe;
- reviewer retest evidence;
- accepted residual risk.

Suspected vulnerabilities are reported through [SECURITY.md](../SECURITY.md),
not a public issue. After coordinated remediation, submit the redacted
[M5 security review evidence issue](https://github.com/rioriost/postgresem/issues/new?template=m5_security_review_evidence.yml).

M5 requires all P0/P1 security findings to be resolved and retested. Lower
severity residual risk must be explicitly accepted and documented.

## Maintainer acceptance

Maintainers verify dates, immutable versions, distinct databases, weekly
coverage, defect disposition, reviewer independence, remediation links, and
retest evidence. Ambiguous, retrospective, fixture-derived, or
success-shaped-without-data submissions are not accepted.

The accepted issue URLs are then recorded in
[the beta checklist](beta-checklist.md). Until that update is merged, the
external gates remain outstanding.
