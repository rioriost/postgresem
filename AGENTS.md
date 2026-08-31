# Agent Instructions

- Follow the implementation plan and accepted ADRs.
- Preserve the no-raw-SQL public contract.
- Prefer deterministic, typed compiler logic and fail closed on ambiguity.
- Add rejection tests for unsupported or unsafe input.
- Keep the compiler crate free of database, transport, and logging I/O.
- Do not weaken PostgreSQL GRANT or RLS enforcement.

