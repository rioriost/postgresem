# M11 release-candidate operator workflow

This workflow proves that one supported installation can operate both guarded
query and governed ingestion without exposing raw SQL as a public input. It
uses fictional fixture data and is not production-pilot evidence.

Run the automated fixture:

```sh
make test-rc-workflow
```

The gate:

1. verifies the frozen `0.9.0` contract manifest;
2. executes a guarded semantic query under the configured read role;
3. executes a governed insert under the separate writer role;
4. repeats the same mutation and requires idempotent replay;
5. requires complete query and mutation audit lifecycles in
   `report operations`;
6. removes the fixture mutation state.

For an approved non-production pilot, record only:

- immutable commit, release tag, and image digest;
- PostgreSQL major version and runtime architecture;
- mapped query and writer role aliases, without credentials or principal data;
- contract manifest version;
- query success/truncation shape;
- mutation affected-row and replay shape;
- aggregate operations-report objectives;
- backup verification and rollback-rehearsal result;
- start/end dates and unresolved P0/P1 defects.

Do not record SQL, LSQ/LSM values, rows, tokens, credentials, private model or
object names, connection strings, or unredacted logs. A maintainer fixture run
does not count as independent production-pilot evidence.
