# Phase 5 — Clean Up & Dead Code Audit

Status: **Validated**.

Per User Rule 3 (*Surgical Changes: clean up only your own mess; remove imports/variables/functions that YOUR changes made unused; don't remove pre-existing dead code unless asked*), this phase audits the diff against the pre-project baseline (`qumulnms` / `aa817871`), removes orphaned items created during this refactor, aligns stale documentation references, and catalogs pre-existing dead code.

---

## 1. Orphans Removed (Created by This Project)

During the Step 4 migration, rules were migrated from granular node-level inspections to high-level query functions (`find_nested_functions`, `find_unwrapped_multiline_strings`). The old granular AST wrappers were re-wrapped in `AstNode` during Step 2 but became unused when the rules adopted the batch extractors.

| File | Item | Status | Rationale |
| :--- | :--- | :--- | :--- |
| `src/code_lint/ast/python.rs` | `pub fn is_nested_function` | **Deleted** | Orphaned when `flat_scope_enforced` migrated to `find_nested_functions`. Internal `is_nested_function_raw` kept. |
| `src/code_lint/ast/python.rs` | `pub fn is_boolean_literal_collection` | **Deleted** | Orphaned when `no_assertion_packing` Python check adopted `has_boolean_literal_comparison`. Internal `is_boolean_literal_collection_raw` kept. |
| `src/code_lint/ast/python.rs` | `pub fn is_multiline_string_literal` | **Deleted** | Orphaned when `prefer_dedent` migrated to `find_unwrapped_multiline_strings`. Internal `is_multiline_string_literal_raw` kept. |
| `src/code_lint/ast/python.rs` | `pub fn is_docstring` | **Deleted** | Same as above. Internal `is_docstring_raw` kept. |
| `src/code_lint/ast/python.rs` | `pub fn is_enclosed_in_call` | **Deleted** | Same as above. Internal `is_enclosed_in_call_raw` kept. |

Verification: `cargo test`, `cargo clippy --all-targets`, and `cargo fmt` all pass with 0 warnings.

---

## 2. Stale Documentation & Artifact Cleanup

| File | Change | Rationale |
| :--- | :--- | :--- |
| `design_doc.md` (L340) | Replaced `&AstGrep<ast_grep_core::source::StrDoc<SupportLang>>` with `&crate::code_lint::ast::ParsedFile` | Historical code sample aligned with the new signature contract. |
| `tests/architecture.rs` | Rewrote test multiline strings to use `indoc::indoc!` | Conforms to repo's own `prefer-dedent-for-multiline-strings` dogfood rule. |
| `docs/dev/rule_design_guide.md` | Documented Single-Path rule and `rule_test!` path update | Aligns guidance with the elimination of module-root facades. |
| Temporary `baseline_probe` workspace | **Forgotten & removed** via `jj workspace forget` | Zero workspace or repository leak. |

---

## 3. Pre-Existing Dead Code Review & Pragmatic Cleanup

Following the comparative dead-code audit against baseline commit `qumulnms`, each candidate item was evaluated against the remaining the legacy linter rules cataloged in `scratch/legacy_reference/legacy_lints.py` and reviewed with the user.

### A. Removed (Unused & No Short-Term Utility)

| File | Item | Status | Rationale |
| :--- | :--- | :--- | :--- |
| `src/code_lint/ast/python.rs` | `pub fn is_test_function` | **Deleted** | Redundant wrapper. Test detection occurs at the file path level or via batch `collect_test_function_assertion_counts` (which uses internal `is_test_function_raw`). |
| `src/code_lint/ast/python.rs` | `pub fn is_assertion_call` | **Deleted** | Redundant wrapper. Assertion counting uses internal `is_assertion_call_raw`. |
| `src/code_lint/ast/python.rs` | `pub fn extract_keyword_args` | **Deleted** | Redundant wrapper. Decorator kwargs use internal `extract_keyword_args_raw`. |
| `src/core.rs` | `FilterListDefaults::new` | **Deleted** | Unused constructor. All rule implementations construct `FilterListDefaults` via static `const` struct literals. |

### B. Retained for Remaining the legacy linter Rules & Core CLI Contracts

| File | Item | Status | Rationale |
| :--- | :--- | :--- | :--- |
| `src/code_lint/ast/python.rs` | `PythonClassInfo::inherits_from` | **Retained** | Required building block for remaining the legacy linter inheritance rules: `FakeMustInheritProtocolRule`, `RequireExplicitProtocolInheritanceRule`, and `UnusedInterfaceMethodRule`. Tested in `test_extract_classes`. |
| `src/code_lint/ast/python.rs` | `extract_parameters` & `PythonParameterInfo::is_variadic` | **Retained** | Required building block for remaining the legacy linter signature rules: `SignatureConcreteTypeRule`, `SignatureMutableTypeRule`, `SignatureSpecificTypeRule`, and `BannedTypeAnnotationsRule`. Tested in `test_extract_parameters_kinds`. |
| `src/code_lint/ast/python.rs` | `KeywordArg::as_bool` | **Retained** | Parses boolean arguments (`True`/`False`); tested in `test_extract_decorators_keyword_args` and needed for decorator flags. |
| `src/command_lint/rule.rs` | `ParsedArgs::{has_flag, get_option, get_all_options}` | **Retained** | Core data model for CLI argument parsing; fully tested by POSIX clustering and glued-value unit tests. |

### C. Confirmed Actively In-Use

- `diagnostic::ViolationMessage::new`: Actively used across `diagnostic.rs` test suite for direct construction of expected test fixtures.
- `diagnostic::ViolationTemplate::from_static`: Actively used in `core.rs:783` for constructing mock test rules.

---

## 4. Verification Gate

- `cargo fmt --check`: pass (clean)
- `cargo clippy --all-targets`: pass (0 warnings)
- `cargo test`: 508 unit + 6 architecture + 14 CLI tests pass (528 total, 0 failures)
- Architecture test suite: 6/6 pass (enforces acyclicity, downward layering, rule isolation, and single canonical import path per item)
- CLI Snapshot regression: 0 diffs
