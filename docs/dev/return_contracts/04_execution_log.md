# Return contracts: 04 Execution log

> **Status: VALIDATED** (lead, 2026-10-13; implemented). Scope: `positional-tuple-return`, `non-boolean-predicate`, `unexplained-return`; `undocumented-tuple-return` removed.

## What was built

- **Shared abstraction** (`src/code_lint/ast.rs`): `ReturnShape` (`Unit`, `Boolean`, `PositionalTuple { arity }`, `Unknown`, `Other`), `FunctionReturn` (name, name node, declared type, shape, `declared_type_text()`), `collect_function_returns(file)` (language-dispatched, imposed signatures excluded once) and the language-neutral `is_predicate_name`.
- **Python** (`src/code_lint/ast/python/returns.rs`, new): `PythonBodyResults` from one own-body walk (value returns, yields, tuple-display arity), `PythonFunctionSignature::{return_type_union, body_results, return_shape}`, and the Python collector. `PythonReturnTypeUnion` gained `is_nullable`, `is_boolean`, `is_opaque`, `positional_tuple_arity`; branches carry their import-resolved constructor. `PythonDocstring::has_condition_summary` shares a summary tokenizer with `has_result_verb_summary`. `PythonFunctionSignature::is_property_getter` added.
- **Rust** (`src/code_lint/ast/rust.rs`): `RustFunction` gained `name_node` and `return_shape()`; the shared `Result` envelope now accepts aliases ending in `Result` (`io::Result`, `CargoResult`), which `nullable-collection-return` also benefits from. Shared type helpers moved to the module's trailing helper section (`uncolocated-helper`).
- **Removed**: `undocumented-tuple-return`, `PythonFunctionSignature::returns_positional_tuple`, `is_positional_tuple_return_type` and the tuple-only body walker; their mutations in `scripts/exemptions/docstrings.py`.
- **Rules**: `positional-tuple-return` and `non-boolean-predicate` are filters over `collect_function_returns` with no language branch; `unexplained-return` composes the union queries, `is_predicate_name` and docstring queries.

## Deviations, with reasons

| # | Deviation | Reason |
| :-- | :--- | :--- |
| X1 | One design directory for three rules instead of one per rule. | The batch was designed together around one abstraction (`03_design.md` D1); per-rule documents would repeat it three times. |
| X2 | Rust 1-tuples and `()` have no mutation (`Unmutated`). | Two guards (`return_shape` reports tuples of two or more elements; `min-elements`) mean no single edit flags them. |

## Findings during dogfooding

- In `require-explanation` mode a Rust `///` doc comment directly above a function counts as the explanation, so the 25 tuple-returning functions in `src/` (including `nontrivial_metric`, `extract_path_type_generics`, `split_entry_line`, `parse_bracketed_rules`) passed without change. The rule now defaults to `ban` (`03_design.md` D5), which flagged all 25; all 25 were refactored to return named structs or enums, and the framework gap is recorded in [ROADMAP.md](../../../ROADMAP.md) ("Doc comments count as `require-explanation` explanations").
- `run_code_rule` in [src/test_utils.rs](../../../src/test_utils.rs) now runs every `rule_test!` case in `ban` mode so rule suites cannot depend on `require-explanation` comment filtering.

## Tests & exemption mutations

- `returns.rs` unit tests: 17 `return_shape` cases.
- `positional-tuple-return`: 13 `pass`, 12 `fail`; `non-boolean-predicate`: 10 `pass`, 10 `fail`; `unexplained-return`: 11 `pass`, 6 `fail`.
- 23 mutations in `scripts/exemptions/return_contracts.py`, all killed; 3 `Unmutated` entries.
- `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings -D clippy::pedantic`, `cargo test`, `RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --document-private-items` and self-dogfooding pass.
