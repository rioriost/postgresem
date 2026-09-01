## Summary

<!-- Explain the problem, the chosen approach, and any compatibility impact. -->

## Validation

- [ ] `cargo fmt --all --check`
- [ ] `cargo clippy --workspace --all-targets --all-features -- -D warnings`
- [ ] `cargo test --workspace --all-features`
- [ ] Relevant JSON schemas parse successfully
- [ ] Relevant container and PostgreSQL integration checks pass

## Security

- [ ] No secrets, credentials, connection strings, private metadata, SQL, or customer data are included
- [ ] Public inputs fail closed and do not add a raw-SQL path
- [ ] The read-only query boundary and any separate mutation role/RLS/audit/idempotency guarantees are preserved or explicitly reviewed
- [ ] Workflow permissions and use of `GITHUB_TOKEN` remain least-privilege

## Compatibility and release

- [ ] PostgreSQL 16, 17, and 18 behavior was considered
- [ ] Linux amd64/arm64 and macOS amd64/arm64 behavior was considered
- [ ] A claimed Linux architecture is executed in CI or explicitly documented as an evidence gap; cross-build alone is not treated as runtime support
- [ ] LSQ, Semantic Schema, MCP, CLI, migration, and archive compatibility impacts are documented
- [ ] Breaking preview changes include migration or upgrade guidance

## Reviewer notes

<!-- Call out areas needing focused review, limitations, and follow-up work. -->
