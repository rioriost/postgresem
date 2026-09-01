# Beta security review checklist

Repository maintainers use this checklist before requesting an independent
review. Completion is not a substitute for an external assessment.

## Identity and authorization

- request bodies cannot select principal, role, credentials, project, or
  policy context;
- runtime role is not superuser, `BYPASSRLS`, or a source owner;
- tenant isolation and hidden-object indistinguishability tests pass;
- audit writer cannot read source data or directly modify audit tables.

## Query and protocol handling

- all public query paths accept LSQ, not raw SQL;
- malformed JSON, unknown fields, deep filters, large `IN`, and invalid typed
  literals fail closed;
- generated identifiers come only from the published catalog;
- parameters remain bound values;
- byte, row, statement, lock, and request limits are enforced;
- timeout or protocol desynchronization never returns a success-shaped result.

## Web demo

- binds only to `127.0.0.1`;
- rejects a nonmatching or duplicate `Host` header;
- accepts only checked-in example IDs;
- does not return credentials, generated SQL, physical database names, or raw
  private diagnostics;
- marks an MCP framing/timeout failure fatal until restart.

## Migration and recovery

- a typo in the migration ceiling changes no schema;
- current binary works on the N-1 schema;
- upgrade preserves the published revision;
- restore uses the original database name or publishes an intentional new
  revision;
- backup files and globals are encrypted and access controlled;
- no ad hoc down migration is used during rollback.

## Supply chain

- tag equals workspace version;
- archives are reproducible and checksummed;
- future signed releases verify the expected GitHub workflow identity and OIDC
  issuer;
- image digest, SBOM, provenance, and signature refer to the same manifest;
- every GitHub Actions dependency is pinned to an immutable commit SHA;
- no long-lived signing key is stored in repository secrets.

## Independent review evidence

Record reviewer identity/organization, scope, commit/tag, dates, findings,
severity, remediation commits, retest evidence, and accepted residual risk.
Never publish exploit details before coordinated remediation.

Follow the [M5 external evidence process](m5-external-evidence.md). Report
suspected vulnerabilities privately through [SECURITY.md](../SECURITY.md);
use the public evidence form only after coordinated remediation.

An internal pre-review is preparation only. It does not establish reviewer
independence and must not be recorded as completion of the external gate.
