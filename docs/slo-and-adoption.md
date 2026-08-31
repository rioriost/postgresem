# Beta SLO and adoption reporting

Migration `0004_beta_operational_report` adds a restricted aggregate report.
The audit writer can execute the report function but still cannot select,
insert, update, or delete `semantic.query_audit` directly.

Run:

```sh
postgresem report beta --window-hours 24
```

The JSON result includes:

- terminal status counts and incomplete audit rows;
- truncation count;
- counts of active principal hashes and semantic revisions;
- validation-plus-compilation p50/p95;
- database p95, reported separately;
- bounded error-code counts;
- audit-completeness and 50 ms compiler objective evaluations.

It does not contain principal hashes, model names, LSQ documents, generated
SQL, parameters, connection data, or result rows. Postgresem does not transmit
the report externally. The database function limits reports to the previous
365 days, rounds the cutoff down to an hour, and suppresses active-principal
and error-code breakdowns when fewer than ten queries are present.

The report is evidence for a deployment window, not an availability promise.
Operators must separately measure process availability, queueing, PostgreSQL
health, backup age, restore duration, and end-user outcomes.

For field evaluation, record:

- time from installation to first successful audited query;
- supported-query success rate and explicit rejection rate;
- timeout and truncation rates;
- repeated weekly use;
- user-confirmed time saved or correctness improvement;
- P0/P1 security and correctness defects.

Do not infer adoption from fixture, maintainer, CI, or automated demo traffic.
