# Phase 6 — Review & Audit (P3 Dedicated AST Migration)

> **Status**: DONE

---

## 1. Requirements & Failure-Mode Verification (`R1`–`R7`, `F1`–`F5`)

| ID | Requirement / Failure Mode | Resolution & Verification |
| :--- | :--- | :--- |
| **F1 / R1** | Modern Python 3.12+ & Rust 2024 syntax fidelity (`&& let` let-chains, PEP 695 `type` aliases, PEP 701 nested f-strings). | `ruff_python_parser = "0.0.16"` and `ra_ap_syntax = "0.0.357"` parse all modern Python and Rust 2024 syntax without `ERROR` nodes. Verified on `src/code_lint/ast/python/format_strings.rs` (where `&& let` previously caused `tree-sitter-rust` to emit an `ERROR` node). |
| **F2 / R2** | Semantic AST uniformity (`Final[int]` vs `typing.Final[int]`; `case -404:` / `{-404: _}` / `Resp(code=-404)` negative literals). | `ruff_python_ast::Expr::Subscript` represents all generic subscripts uniformly. `PatternMatchValue` and `Expr::UnaryOp(USub)` represent negative literals uniformly. Promoted `negative_numbers_in_mapping_and_keyword_patterns` in `src/code_lint/rules/repeated_literal.rs` from `known_gap` (`pass`) to `fail`. |
| **F3 / R3** | Native escape-decoded literal values (`"\x61\x62"` == `"ab"`, `r"a\tb"` != `"a\tb"`). | `ruff_python_ast::StringLiteral::as_str()` / `BytesLiteral::as_slice()` and `ra_ap_syntax::ast::String::value()` / `ByteString::value()` decode escapes natively during lexing/parsing. |
| **F4 / R4** | Compile-time exhaustiveness and zero stringly-typed node-kind / field-name matching in `code_lint`. | All Python and Rust AST queries in `src/code_lint/ast/` match on `ruff_python_ast` enums/structs and `ra_ap_syntax` `ast::*` / `SyntaxKind` variants checked by `rustc`. |
| **F5 / R5** | Eliminate unused C grammar compilation overhead in `Cargo.toml`. | `ast-grep-language` trimmed to `default-features = false, features = ["tree-sitter-bash"]`, dropping 25 unused C/C++ Tree-sitter grammars. |
| **R6** | Architectural boundary enforcement (`tests/architecture_conformance.rs`). | `AST_GREP_OWNERS` restricted to `&["command_lint::command"]`; `DEDICATED_AST_OWNERS` (`ruff_python_parser`, `ruff_python_ast`, `ruff_text_size`, `ra_ap_syntax`) restricted to `&["code_lint::ast", "bin::ast_dumper"]`. |
| **R7** | File-level query memoization (`OnceLock` on `ParsedFile`) and clean rule ergonomics. | `ParsedFile` caches `comment_spans`, `binding_spans`, `call_candidates`, `rust_inline_test_ranges`, `abc_set_imported`, and `locally_mutated_return_functions` via `OnceLock`, eliminating redundant full-AST passes across rules. |

---

## 2. Mandatory Zero-Legacy & Zero-Compat Bloat Audit

| # | Audit Check | Command | Result |
| :--- | :--- | :--- | :--- |
| 1 | Zero `ast-grep` or `tree-sitter` in `src/code_lint/` | `rg 'ast_grep\|tree_sitter\|SupportLang\|RawNode\|SourceDoc\|find_pattern_calls' src/code_lint/` | **0 matches** (PASS) |
| 2 | Zero `SupportLang` outside `src/command_lint/command.rs` | `rg 'SupportLang' src/ tests/` | Matches **only** in `src/command_lint/command.rs` (PASS) |
| 3 | Zero legacy string/delimiter stripping shims | `rg 'normalize_string_content\|rust_string_body\|delimited_string_parts' src/` | **0 matches** (PASS) |
| 4 | Zero `generic_type` vs `subscript` workarounds in Python AST | `rg 'generic_type' src/code_lint/ast/python*` | Matches only the public function name `extract_generic_type` (PASS) |
| 5 | Zero `// omni:disable-file [repeated-literal]` in `src/code_lint/ast*` | `rg 'omni:disable-file \[repeated-literal\]' src/code_lint/ast*` | **0 matches** (PASS) |
| 6 | Zero transitional `Option` fields or `#[allow(clippy::unwrap_used)]` on `ParsedFile` / `AstNode` | Inspect `src/code_lint/ast.rs` | `AstNode<'a>` is `{ file: &'a ParsedFile, span: SourceSpan }` (`Copy`); 0 `.unwrap()` / `#[allow]` (PASS) |
| 7 | Visibility audit of `src/code_lint/ast/` | Inspect `pub fn` in `src/code_lint/ast/{python,rust}.rs` | All `ast.rs`-dispatched helpers are `pub(super)` / `pub(in crate::code_lint::ast)` (PASS) |
| 8 | Full quality gate | `cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test && RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --document-private-items` | **100% green** (1,397 lib + 11 arch + 23 CLI + 17 registry + 8 doctests) (PASS) |
