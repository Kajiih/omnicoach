# Agent Guidelines

## Verification Scope

Run the narrowest checks that cover what you changed.

- `cargo test` and `cargo clippy --all-targets` are fast; run them as usual.
- [scripts/exemption_mutations.py](scripts/exemption_mutations.py) recompiles the crate once per mutation, so a full run takes about 20 minutes. Run it filtered to what you changed: its argument is a regex matched against mutation labels and `pass` case names.
  - One rule: `python3 scripts/exemption_mutations.py 'callee_before_caller::'`.
  - One exemption: pass part of its mutation label, such as `'signature types'`.
  - A shared helper under `src/code_lint/ast*`: filter to the rules whose mutations patch that helper.
- Run the full mutation suite only for large cross-cutting changes (such as reworking a shared AST module or the mutation registry) or before a release, and ask first.

## Module Placement

Place every item in the module whose abstraction vocabulary states its contract, regardless of who calls it or what it is built on: see [ADR 012](docs/dev/decisions/012_module_placement_by_abstraction.md) (alongside [ADR 006](docs/dev/decisions/006_architectural_dag_and_conformance.md) for component dependencies and [ADR 011](docs/dev/decisions/011_colocated_abstraction_ordering.md) for intra-scope declaration ordering).
