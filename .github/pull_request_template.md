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
- [ ] Read-only execution, role mapping, RLS, audit, and redaction guarantees are preserved or explicitly reviewed
- [ ] Workflow permissions and use of `GITHUB_TOKEN` remain least-privilege

## Compatibility and release

- [ ] PostgreSQL 16, 17, and 18 behavior was considered
- [ ] Linux amd64/arm64 and macOS amd64/arm64 behavior was considered
- [ ] LSQ, Semantic Schema, MCP, CLI, migration, and archive compatibility impacts are documented
- [ ] Breaking preview changes include migration or upgrade guidance

## Reviewer notes

<!-- Call out areas needing focused review, limitations, and follow-up work. -->
