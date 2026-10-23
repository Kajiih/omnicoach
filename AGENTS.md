# Agent Guidelines

## Verification Scope

Run the narrowest checks that cover what you changed.

- `cargo test` and `cargo clippy --all-targets` are fast; run them as usual.
- When adding or modifying a rule or AST helper, verify branch and test adequacy scoped to what you changed:
  - Diff-scoped or file-scoped mutation testing: `cargo mutants --cap-lints=true --in-diff -- --lib` or `cargo mutants --cap-lints=true -f <path> -- --lib`.
  - Single-build branch coverage across the library: `cargo +nightly llvm-cov --lib --branch`.

## Architecture and Module Placement

- Treat adding or updating a rule as an opportunity to improve the shared abstractions and component boundaries, never as a narrow patch: when an existing `ast`, `semantic`, or `policy` helper bakes in one caller's policy or lacks a general fact, refactor the abstraction rather than bolting on flags or callbacks ([ADR 012](docs/dev/decisions/012_module_placement_by_abstraction.md) P6–P7).
- Place every item in the module whose abstraction vocabulary states its contract, regardless of who calls it or what it is built on: see [ADR 012](docs/dev/decisions/012_module_placement_by_abstraction.md) (alongside [ADR 006](docs/dev/decisions/006_architectural_dag_and_conformance.md) for component dependencies and [ADR 011](docs/dev/decisions/011_colocated_abstraction_ordering.md) for intra-scope declaration ordering).
