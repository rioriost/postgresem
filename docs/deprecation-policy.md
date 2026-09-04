# Stable deprecation policy

The stable v1 contract inventory is the compatibility boundary for 1.x.

## Change classes

| Change | Stable 1.x rule |
|---|---|
| additive response field | allowed when clients can ignore it and meaning is documented |
| additive CLI command or MCP tool | requires tests, documentation, and stable-manifest refresh |
| stricter rejection closing a security/correctness defect | allowed with release-note justification |
| request field removal/rename or meaning change | requires a new contract version |
| error-code removal or semantic reuse | requires a new error taxonomy version |
| migration edit/reorder | prohibited; add a forward migration |
| removal of a deprecated surface | prohibited before the later of its documented removal version or 12 months after deprecation |

## Current classifications

- **Stable v1:** LSQ v1, LSM v1, Semantic Snapshot v1/v2 loading,
  Snapshot v2 authoring, compiler semantics `0.2.0`, mutation compiler
  semantics `0.1.0`, catalog snapshot v2, catalog diff v1, MCP protocols/tool
  schema/resources, query/mutation audit meanings, and migrations
  `0001`–`0010`.
- **Deprecated but supported:** `postgresem report beta`. Use
  `postgresem report operations`. It will not be removed before `2.0.0` or
  before 12 months have elapsed from the formal `1.0.0` release, whichever is
  later.
- **Review-only, not publication:** `model scaffold` and `model import osi`.
  Their candidate/report outputs are frozen, but they do not write Semantic
  Schema rows.
- **Deferred:** automatic materialized-view routing, pre-aggregation,
  connection pooling, distributed rate limits, runtime OIDC/JWKS discovery,
  general update/delete, and down migrations.

## Process

Any frozen-surface change must:

1. identify the affected contract and compatibility class;
2. add an ADR for a breaking or security-boundary change;
3. add accepted and rejected tests;
4. update compatibility, error, migration, and operator documentation;
5. refresh the stable manifest intentionally with
   `python3 tests/contracts/verify.py --refresh`.

The manifest hash gate is evidence of reviewed change control, not a substitute
for semantic review.
