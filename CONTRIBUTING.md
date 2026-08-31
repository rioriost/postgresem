# Contributing

The project is in its foundation phase. Discuss changes that alter LSQ,
semantic schema, authorization, query semantics, or compatibility in an ADR
before implementation.

Before submitting a change:

```sh
make fmt
make check
make test
```

Changes to compiler behavior must include tests for accepted input and safe
rejection. Do not add raw SQL strings to public API contracts.

