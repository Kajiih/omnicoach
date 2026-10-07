# Phase 4 — Execution Log (P3 Dedicated AST Migration)

> **Status**: IN PROGRESS

---

## Slice 0 — Toolchain Update (`rustc 1.99.0`)

- **Action**: Ran `rustup update stable` to update the local Rust toolchain from `1.90.0` to `1.99.0 (b940084d7 2026-09-28)` and `cargo 1.99.0 (5f94df478 2026-08-27)`.
- **Verification**: Confirmed via `cargo info` that crates.io `ruff_python_parser = "=0.0.16"` (`rust-version = 1.97`), `ruff_python_ast = "=0.0.16"`, `ruff_text_size = "=0.0.2"`, and `ra_ap_syntax = "=0.0.357"` (`rust-version = 1.98`) are compatible with the local toolchain and `.github/workflows/release.yml` (`cargo publish --locked`).

---

## Slice 1 — First-Class `crate::diagnostic::Language` Enum (Decision D3) — DONE

- **Goal**: Replace `ast_grep_language::SupportLang` outside `src/code_lint/ast.rs` and `src/command_lint/command.rs` with `crate::diagnostic::Language { Python, Rust }`, and remove `support_lang_name` in favor of `Language::as_str` / `Display`.
- **Changes** (57 files, +490 / −547):
  - `src/diagnostic.rs`: `Language` with `as_str()` (configuration keys, Markdown fences), `from_path(&Path)`, `Display` (`Python` / `Rust`, used by the catalog renderer) and `strum::VariantArray` (`Language::VARIANTS`).
  - Mechanical `SupportLang` → `Language` rename in rules, semantic engines, runner, rule declarations, catalog, taxonomy, `test_utils` (including `rule_test!`, which now names `$crate::diagnostic::Language`) and `tests/registry.rs`.
  - `ParsedFile` stores `lang: Language`. Transitional bridges `to_support_lang` (private) and `from_support_lang` (`pub(in crate::code_lint::ast)`) live in `ast.rs` until `AstGrep` leaves `ParsedFile` (Slices 2 and 5).
- **Deviations from the plan** (all simplifications):
  1. `detect_language` is deleted instead of retyped: its 7 call sites use `Language::from_path`, so one concern has one API.
  2. `SUPPORTED_LANGUAGES` is deleted in favor of `Language::VARIANTS`. It only existed because `SupportLang` has 28 variants; keeping a second list would let it drift when a language is added.
  3. `dispatch_lang!` drops its `$fallback` argument and is exhaustive over `Language`: a new language becomes a compile error at every dispatch site instead of a silent `false` / `None` / `Vec::new()`. Its `pub(crate) use` re-export is removed (the macro is in textual scope for `ast/*`, and rustc 1.99 reports the path import as unused).
  4. Exhaustive matches replace the now-unreachable wildcard and panic arms in `nullable_collection_return::check_file`, `packed_assertion::check_file`, `RegisteredRule::new` (`filter_map` → `map`) and `test_utils::dummy_filename` (a test-time panic becomes a compile-time error).
  5. `Language` derives only what is used: no `serde`, no `PartialOrd` / `Ord`.
- **Toolchain follow-up (Slice 0)**: rustc 1.99 clippy lints on pre-existing code, fixed without behavior change:
  - `question_mark`: `extract_valid_field_root` (`format_strings.rs`) and `parse_directive_prefix` (`suppression.rs`).
  - `collapsible_match`: `summarize_rust_node` (`rust.rs`).
  - `assert_is_empty`: 6 test assertions (`runner.rs` ×3, `edit_of_described_commit.rs` ×2, `architecture_conformance.rs` ×1) now use `assert_eq!` against an empty value, so a failure prints the unexpected content.
- **Remaining `SupportLang`** (all expected):
  - `code_lint/ast.rs`: the bridge and `SourceDoc` (Slices 2 and 5).
  - `code_lint/ast/statements.rs`: raw-CST tests (Slices 3 and 5).
  - `command_lint/command.rs`: Bash; `command_lint` is out of scope (see `ROADMAP.md`).
  - `bin/ast_dumper.rs`: carried to Slice 5, where it is ported to dump `ruff_python_ast` / `ra_ap_syntax` trees (see `01_understand.md`) or deleted.
- **Tests**: no test added or removed. Test edits only rename the language type, apart from the 6 `assert_is_empty` assertions above.
- **Verification**: `cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test && cargo doc --no-deps --document-private-items` is green:
  - lib 1396 passed;
  - `architecture_conformance` 11, `cli` 23 (includes self-dogfooding), `registry` 17;
  - doctests 8 passed, 1 ignored;
  - no rustdoc warnings.

---

## Slice 2 — AST Core (`src/code_lint/ast.rs`, `src/code_lint/ast/statements.rs`) + Native `CallPattern` (`src/code_lint/semantic/calls.rs`) — DONE

- **Goal**: Add `ruff_python_parser = "=0.0.16"`, `ruff_python_ast = "=0.0.16"`, `ruff_text_size = "=0.0.16"`, and `ra_ap_syntax = "=0.0.357"` to `Cargo.toml`; migrate `ParsedFile` core methods, `collect_comment_nodes`, `collect_call_candidates`, `enclosing_non_exempt_function_name`, and `statements::enclosing_statement_header_range` (both Python and Rust) to dedicated ASTs; replace `ast-grep` `$OBJ` / `$LOOP` call patterns in `semantic/calls.rs` with native single-pass call matching (Decision D4) and delete `ast::find_pattern_calls`.
- **Changes**:
  - `Cargo.toml`: Added `ruff_python_parser = "=0.0.16"`, `ruff_python_ast = "=0.0.16"`, `ruff_text_size = "=0.0.16"`, `ra_ap_syntax = "=0.0.357"`, and `multiple_crate_versions = "allow"`.
  - `src/code_lint/ast.rs`:
    - Added `LineIndex`, `CodeLintAst { Python(...), Rust(...) }`, `AstNodeRepr<'a> { Raw(RawNode<'a>), Span { file: &'a ParsedFile, span: SourceSpan } }` (transitional dual-repr deleted in Slice 5), `span_from_ruff_range`, `span_from_rowan_range`, and `is_rust_comment_kind` (`COMMENT | OUTER_DOC_COMMENT | INNER_DOC_COMMENT`).
    - Migrated `ParsedFile::source_text`, `ParsedFile::has_syntax_error`, `collect_comment_nodes`, `collect_call_candidates` (enriched with `receiver_call_callee: Option<String>`), and `enclosing_non_exempt_function_name` to `ruff_python_ast` and `ra_ap_syntax`.
    - Deleted `ast::find_pattern_calls` and `ast::from_support_lang`.
    - Removed `// omni:disable-file [repeated-literal]` from `src/code_lint/ast.rs` (zero repeated string literals remain in `ast.rs`).
  - `src/code_lint/ast/statements.rs`:
    - Migrated `enclosing_statement_header_range` (both Python and Rust) and all unit tests to `ruff_python_ast` and `ra_ap_syntax`.
    - Deleted `is_statement_container` and `earliest_attribute_start_line` from `python.rs` and `rust.rs`, plus `rust::decorated_definition`.
    - Fixed a real F1/F2 interaction uncovered by migrating `collect_comment_nodes`: previously, `tree-sitter-rust` choked on `&& let` (Rust 2024 `let`-chains) in `src/code_lint/ast/python/format_strings.rs`, placing `named_format_field_roots` inside an `ERROR` node so `statements::enclosing_statement_header_range` returned `None` (which had been masked only because `tree-sitter-rust`'s `line_comment` token included the trailing `\n` and smeared line 486's comment onto line 487's `#[must_use]`). With `ra_ap_syntax` in `statements.rs`, `format_strings.rs` parses with zero syntax errors and `enclosing_statement_header_range` resolves `487..=488` cleanly.
  - `src/code_lint/semantic/calls.rs`, `src/code_lint/rules/mock_call_assertion.rs`, `src/code_lint/rules/unstructured_task.rs`:
    - Replaced `$OBJ.assert_*` with `*.assert_*` and `$LOOP($$$LOOP_ARGS).create_task` with `*().create_task`.
    - Rewrote `find_banned_calls` to evaluate literal callees, `*.<method>`, `*().<method>`, and `<receiver_callee>().<method>` in a single pass over `ast::collect_call_candidates(file)`.
  - `src/code_lint/ast/python.rs`:
    - Updated `find_enclosing_with_item`, `find_enclosing_with_statement`, `is_with_context_manager`, and `is_inside_except_clause` to support `AstNodeRepr::Span` via `ruff_python_ast` visitors.
    - Deleted unused `is_comment_kind`, `is_call_kind`, and `extract_method_call_target` from `python.rs` and `rust.rs`.
- **Verification**: `cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test && cargo doc --no-deps --document-private-items` is green:
  - lib 1397 passed;
  - `architecture_conformance` 11, `cli` 23 (includes self-dogfooding), `registry` 17;
  - doctests 8 passed, 1 ignored;
  - no rustdoc warnings.

---

## Slice 3 — Migrate `src/code_lint/ast/rust.rs` to `ra_ap_syntax` (Decisions D1, D6, D7) — DONE

- **Goal**: Migrate `src/code_lint/ast/rust.rs` 100% from `ast-grep` / `tree-sitter-rust` (`RawNode`) to `ra_ap_syntax` (`SourceFile`, `SyntaxNode`, `SyntaxToken`, `SyntaxKind`, typed `ast::*` wrappers, and native literal decoding), and lift `// omni:disable-file [repeated-literal]` from `rust.rs`.
- **Changes**:
  - `src/code_lint/ast/rust.rs`:
    - Migrated all Rust AST extractors to `ra_ap_syntax`:
      - Binding and import extraction (`is_import_binding`, `is_structural_definition`, `is_trait_impl_member`, `collect_bindings`) via `ast::Use`, `ast::UseTree`, `ast::Pat`, `ast::LetStmt`, `ast::ForExpr`, `ast::Param`, `ast::ClosureExpr`, `ast::MatchArm`, and `HasName`.
      - Attribute and inline test range detection (`collect_inline_test_ranges`, `is_test_attribute`, `is_conditional_test_attribute`, `is_doc_attribute`) using `ast::Attr`, `ast::Meta::CfgMeta`, and `ast::CfgPredicate::CfgAtom`.
      - Test assertion and macro argument inspection (`collect_test_function_assertion_counts`, `macro_terminal_name`, `has_top_level_logical_and`, `extract_macro_arguments`, `is_boolean_literal_collection`) using `ast::Fn`, `ast::MacroCall`, `ast::TokenTree`, `ast::ArrayExpr`, and `ast::TupleExpr`.
      - Multiline string detection (`find_unwrapped_multiline_strings`) over `SyntaxKind::STRING | BYTE_STRING | C_STRING` tokens with `insta` snapshot, `#[doc = "..."]`, and wrapper-macro exclusions.
      - Production file summary (`summarize_rust_file`) over typed `ast::Module`, `ast::MacroRules`, `ast::MacroCall`, `ast::Use`, `ast::Path`, and `ast::Attr`.
      - Positional tuple-field reads (`collect_positional_reads`) over `ast::FieldExpr` with `ast::NameRef::as_tuple_field` and mutation/borrow exclusions.
      - Literal occurrence extraction (`collect_literal_occurrences`) using `ast::String::value()` and `ast::ByteString::value()` (`AstToken`) for native escape and raw-string decoding (Decision D6), plus macro `TokenTree` traversal (`rust_string_body` deleted).
      - Function & return type unwrapping (`collect_functions`, `unwrap_return_envelope`, `extract_option_payload`, `unwrap_pointer_wrappers`, `extract_generic_type`, `resolve_type_path`, `extract_slice_type`) over `ast::Fn`, `ast::PathType`, `ast::RefType`, `ast::SliceType`, and `ast::ArrayType`.
    - Removed `// omni:disable-file [repeated-literal]` from `src/code_lint/ast/rust.rs` and extracted module-level constants for shared type/keyword identifiers.
    - Zero `RawNode` or `ast-grep` references remain in `src/code_lint/ast/rust.rs`.
  - `src/code_lint/ast.rs`:
    - Dispatched `is_import_binding` and `is_structural_definition` for Rust directly to `rust::is_import_binding(node)` and `rust::is_structural_definition(node)`.
- **Verification**: `cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test && cargo doc --no-deps --document-private-items` is green:
  - lib 1397 passed;
  - `architecture_conformance` 11, `cli` 23 (includes self-dogfooding), `registry` 17;
  - doctests 8 passed, 1 ignored;
  - no rustdoc warnings.

---

## Slice 4 — Migrate `src/code_lint/ast/python/*` Submodules to `ruff_python_ast` (Decisions D1, D7) — DONE

- **Goal**: Migrate all 7 `src/code_lint/ast/python/` submodules (`strings.rs`, `format_strings.rs`, `logging.rs`, `annotations.rs`, `classes.rs`, `functions.rs`, `scopes.rs`) and their coupled root helpers in `src/code_lint/ast/python.rs` from `ast-grep` / `tree-sitter-python` (`RawNode`) to `ruff_python_ast`, and lift `// omni:disable-file [repeated-literal]` on every migrated submodule.
- **Changes**:
  - `src/code_lint/ast/python/strings.rs`:
    - Migrated `fstring_segments_and_interpolations`, `static_string_text`, and `extract_multiline_docstring_text` to `ruff_python_ast` (`Expr::StringLiteral`, `Expr::FString`, `FStringPartRef`, `InterpolatedStringElement`, `StringFlags`), using `StringLiteral::as_str()` directly and eliminating `normalize_string_content` / `delimited_string_parts` from `strings.rs`.
    - Removed `// omni:disable-file [repeated-literal]`.
  - `src/code_lint/ast/python/format_strings.rs`:
    - Migrated `collect_format_strings` (f-strings, `.format()` calls, `%` binary operators) and `named_format_field_roots` to `ruff_python_ast::visitor::source_order::SourceOrderVisitor` over `Expr::FString`, `Expr::Call`, and `Expr::BinOp` (`Operator::Mod`).
    - Removed `// omni:disable-file [repeated-literal]`.
  - `src/code_lint/ast/python/logging.rs`:
    - Migrated `collect_logger_calls` and `extract_logger_call` to `SourceOrderVisitor` over `Expr::Call` and `ExceptHandler::ExceptHandler`.
  - `src/code_lint/ast/python/annotations.rs`:
    - Migrated `has_unaliased_collections_abc_set_import`, `extract_generic_base_and_args`, `has_final_annotation_expr`, `is_bare_final_annotation_expr`, `collect_type_constructors_expr`, `collect_collection_types`, `collection_type`, `collection_display`, `extract_generic_type`, `return_type_union`, and `unwrap_return_envelope` to `ruff_python_ast` (`Expr::Subscript` uniformly represents both `Final[int]` and `typing.Final[int]`, eliminating the Tree-sitter `generic_type` vs `subscript` split; `Expr::BinOp` with `Operator::BitOr` represents PEP 604 `X | Y` unions).
    - Deleted `unwrap_type_and_parens` (no longer needed with `ruff_python_ast`).
    - Removed `// omni:disable-file [repeated-literal]`.
  - `src/code_lint/ast/python/classes.rs`:
    - Migrated `extract_classes`, `is_in_protocol_or_abc_class`, `collect_class_attributes`, and `collect_instance_attribute_annotations` to `Stmt::ClassDef`, `Stmt::FunctionDef`, and `Stmt::AnnAssign`.
    - Removed `// omni:disable-file [repeated-literal]`.
  - `src/code_lint/ast/python/functions.rs`:
    - Migrated `extract_parameters`, `extract_function_signatures`, `find_nested_functions`, `direct_function_definitions`, `has_override_decorator`, `is_trait_impl_member`, `is_stub_function_body`, and `method_receiver_name_ast` to `Stmt::FunctionDef`, `Parameters`, and `ParameterWithDefault`.
    - Deleted `parse_param_parts` and `extract_return_type_node` (replaced by `Parameters` and `func_def.returns`).
    - Removed `// omni:disable-file [repeated-literal]`.
  - `src/code_lint/ast/python/scopes.rs`:
    - Migrated `collect_bindings`, `parameters_shadow_name`, and `collect_function_scopes` to `ruff_python_ast` (`Stmt::Import`, `Stmt::ImportFrom`, `Pattern`, `Comprehension`, `ExceptHandler`, `WithItem`, `Stmt::FunctionDef`).
    - Removed `// omni:disable-file [repeated-literal]`.
  - `src/code_lint/ast/python.rs` & `src/code_lint/ast.rs`:
    - Migrated `is_import_binding`, `is_structural_definition`, `extract_decorators`, `has_decorator`, `collect_module_assignments`, `call_callee`, `find_unwrapped_multiline_strings`, `collect_locally_mutated_return_functions`, `is_parameter_mutated_or_escaping`, and `analyze_parameter_collection_capability` to `ruff_python_ast`.
- **Verification**: `cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test && cargo doc --no-deps --document-private-items` is green:
  - lib 1397 passed;
  - `architecture_conformance` 11, `cli` 23 (includes self-dogfooding), `registry` 17;
  - doctests 8 passed, 1 ignored;
  - no rustdoc warnings.
