# Phase 5 — Cleanup (P3 Dedicated AST Migration)

> **Status**: DONE

---

## 1. Dead Code & Legacy Shim Removal

All legacy `ast-grep` / `tree-sitter` scaffolding, transitional compatibility shims, and unused public helpers in `src/code_lint/` have been removed:

| Removed Item | Location | Reason |
| :--- | :--- | :--- |
| `SourceDoc`, `RawNode`, `AstGrep<SourceDoc>` | `src/code_lint/ast.rs` | Replaced by `CodeLintAst { Python(Parsed<ModModule>), Rust(Parse<SourceFile>) }`. |
| `AstNodeRepr`, `AstNode::from_raw`, `AstNode::raw_opt`, `AstNode::file_opt` | `src/code_lint/ast.rs` | Transitional Slice 2–4 dual-representation wrapper deleted in Slice 5; `AstNode<'a>` is a `Copy` `{ file: &'a ParsedFile, span: SourceSpan }` handle. |
| `detect_language`, `SUPPORTED_LANGUAGES`, `support_lang_name`, `to_support_lang`, `from_support_lang` | `src/code_lint/ast.rs`, `src/diagnostic.rs` | Replaced by `crate::diagnostic::Language` (`Language::from_path`, `Language::VARIANTS`, `Language::as_str`, `Display`). |
| `find_pattern_calls` | `src/code_lint/ast.rs` | Replaced by native single-pass `CallPattern` matching (`"*.method"`, `"*().method"`, `"<callee>().method"`) in `src/code_lint/semantic/calls.rs`. |
| `normalize_string_content`, `strip_named_unicode_escapes`, `delimited_string_parts` | `src/code_lint/ast/python.rs` | Replaced by `ruff_python_ast` native literal decoding (`StringLiteral::as_str()`, `BytesLiteral::as_slice()`, `StringLiteralValue::to_str()`). |
| `rust_string_body` | `src/code_lint/ast/rust.rs` | Replaced by `ra_ap_syntax::ast::String::value()` and `ast::ByteString::value()`. |
| `unwrap_type_and_parens` | `src/code_lint/ast/python/annotations.rs` | Parenthesized expressions and `type` wrappers are handled natively by `ruff_python_ast::Expr`. |
| `parse_param_parts`, `extract_return_type_node` | `src/code_lint/ast/python/functions.rs` | Replaced by typed `ruff_python_ast::Parameters`, `ParameterWithDefault`, and `StmtFunctionDef.returns`. |
| `is_statement_container`, `earliest_attribute_start_line`, `decorated_definition` | `src/code_lint/ast/{python,rust}.rs` | Statement header ranges in `src/code_lint/ast/statements.rs` are computed directly from `ruff_python_ast::Stmt` and `ra_ap_syntax::SyntaxNode` / `HasAttrs`. |
| `is_comment_kind`, `is_call_kind`, `extract_method_call_target` | `src/code_lint/ast/{python,rust}.rs` | Comment and call collection are implemented directly on the typed AST/CST in `src/code_lint/ast.rs`. |
| `collect_outer_test_functions` | `src/code_lint/ast/python.rs` | Dead public helper with zero callers anywhere in the repository. |
| `unwrap_return_envelope` | `src/code_lint/ast/python/annotations.rs` | Dead public helper with zero callers anywhere in the repository (`return_type_union` calls private `unwrap_return_envelope_expr` directly). |

---

## 2. Visibility Tightening Across `src/code_lint/ast/`

Internal language-specific helpers that were previously `pub` have been restricted to `pub(super)`, `pub(in crate::code_lint::ast)`, or private `fn` so external callers in `src/code_lint/rules/` and `src/code_lint/semantic/` use the language-dispatched entry points on `code_lint::ast`:

- **`src/code_lint/ast/rust.rs`**:
  - Tightened to `pub(super) fn`: `is_import_binding`, `is_structural_definition`, `is_trait_impl_member`, `collect_bindings`, `collect_test_function_assertion_counts`, `find_unwrapped_multiline_strings`, `collect_positional_reads`, `collect_literal_occurrences`.
  - Tightened to private `fn`: `attribute_terminal_name`, `is_test_attribute`, `is_conditional_test_attribute`, `is_doc_attribute`, `has_preceding_doc_comment`.
- **`src/code_lint/ast/python.rs` & submodules**:
  - Tightened to `pub(super)` / `pub(in crate::code_lint::ast)`: `is_import_binding`, `is_structural_definition`, `is_trait_impl_member`, `collect_bindings`, `has_decorator`, `collect_test_function_assertion_counts`, `find_unwrapped_multiline_strings`, `collect_positional_reads`, `collect_literal_occurrences`.
  - Tightened to private `fn`: `extract_decorators`, `find_enclosing_with_item`, `find_enclosing_with_statement`, `has_override_decorator`.
  - Tightened to `#[cfg(test)] pub(super) fn`: `extract_parameters`, `find_parameters_at_span`.

---

## 3. Removal of All `// omni:disable-file [repeated-literal]` Directives in `src/code_lint/ast/`

Replacing stringly-typed node-kind and field-name queries with typed `ruff_python_ast` and `ra_ap_syntax` nodes/enums eliminated the repeated grammar-string literals that previously required file-level suppressions across all 10 files in `src/code_lint/ast/`:

- `src/code_lint/ast.rs`
- `src/code_lint/ast/rust.rs`
- `src/code_lint/ast/python.rs`
- `src/code_lint/ast/python/annotations.rs`
- `src/code_lint/ast/python/classes.rs`
- `src/code_lint/ast/python/format_strings.rs`
- `src/code_lint/ast/python/functions.rs`
- `src/code_lint/ast/python/scopes.rs`
- `src/code_lint/ast/python/strings.rs`
- `src/code_lint/ast/statements.rs`

Zero `omni:disable-file [repeated-literal]` directives remain anywhere in `src/code_lint/ast*`.
