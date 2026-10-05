# Dedicated AST migration (P3): 01 Understand

> [!NOTE]
> **Status: VALIDATED (D1–D8 resolved). Ready for Phase 2/3 (`02_references.md` + `03_design_plan.md`).**
> Scope: replacing `ast-grep` / Tree-sitter in `code_lint` with `ruff_python_parser` + `ruff_python_ast` (Python) and `ra_ap_syntax` (Rust), replacing `ast_grep_language::SupportLang` outside `command_lint` with an Omni `Language` type, refining `code_lint::ast` and rule-authoring abstractions with **zero legacy or backward-compatibility bloat**, and trimming `ast-grep-language` to `tree-sitter-bash` only until `command_lint`'s own AST migration (`ROADMAP.md`).
> Supersedes the exploratory `01_understand.md` on side change `lxslyzoskzpz` now that prototypes P1–P3 (`scratch/proto_p{1,2,3}/REPORT.md`) are complete and [ast_policy_extraction](../ast_policy_extraction/01_understand.md) has landed on `main` (`lnwtoytpwkmx`).
> Line numbers are from `lnwtoytpwkmx` (`main`) and will drift. `[U]` marks claims not checked against code or tool output.

## 1. Problem

- **Unchecked string grammar vocabulary (F1, F4).** `src/code_lint/ast` has **679** `.kind()` / `.field()` string sites across 6,971 non-test lines (§4.1). A typo or upstream grammar change compiles and silently never matches. Ten files carry `// omni:disable-file [repeated-literal]` (`ast.rs:8`, `python.rs:3`, `rust.rs:3`, `python/*.rs:3–4`).
- **Tree-sitter CST shape quirks leak into extractors (F2).** Because Tree-sitter produces a concrete syntax tree tuned for editor highlighting rather than language semantics, `code_lint::ast` maintains ~1,100 lines of CST normalization:
  - Python type annotations wrap expressions in `type`, and split `Final[int]` (`generic_type` + `type_parameter`) from `typing.Final[int]` (`subscript`) (`annotations.rs:21–33,87–148`).
  - Python parameters have five distinct node shapes parsed by hand (`functions.rs:103–189`, noted as a defect in `ROADMAP.md:25–26`).
  - Python strings, prefixes, `\N{...}` named escapes, implicit concatenations, and f-string replacement fields are re-parsed from raw source text (`strings.rs:8–188`, `format_strings.rs:68–202`, `ast.rs:361–373`).
  - Negative numbers in Python `match` mapping/keyword patterns (`case {-404: _}:`, `case Resp(code=-404):`) parse as an anonymous `"-"` token plus a sibling `integer`, causing `known_gap_negative_numbers_in_mapping_and_keyword_patterns` (`repeated_literal.rs:283`).
  - Python `@decorator` wraps `function_definition` / `class_definition` in a parent `decorated_definition` node (`python.rs:64–70,248–290`).
  - Rust `#[cfg(test)]` and `#[test]` attributes are preceding siblings of the item rather than children (`rust.rs:29–33,391–445`).
- **Non-exhaustive string matching (F3).** Every `match node.kind().as_ref()` ends with `_ =>`, so `rustc` never flags unhandled statement, expression, or pattern variants.
- **Build and dependency bloat (F5).** `ast-grep-language = "0.45"` compiles and links **28 C/C++ Tree-sitter grammars** by default (`Cargo.toml:26`), while Omni uses 3 (`Python`, `Rust`, `Bash`).
- **`SupportLang` leaks across the crate.** `ast_grep_language::SupportLang` appears in **50 `src/` files outside `code_lint::ast`** (§4.2): every rule declaration, `Example`, `violation_template!`, `rule_test!`, `options.rs`, `runner.rs`, and `taxonomy.rs`. `tests/architecture_conformance.rs:426–430` only encapsulates `ast_grep_core`, not `ast_grep_language`.
- **`ast-grep` pattern syntax leaked into call matching.** `semantic::calls::find_banned_calls` (`semantic/calls.rs:61–103`) routes banned-callee entries containing `$` or ending in `)` to `ast::find_pattern_calls` (`ast.rs:378–400`), which calls `file.grep.root().find_all(pattern)`. In-tree rules use `$OBJ.<method>` (`mock_call_assertion.rs:19–32`, already handled by the fast path in `calls.rs:63–68`) and `"$LOOP($$$LOOP_ARGS).create_task"` (`unstructured_task.rs:25`, which hits `find_pattern_calls`).
- **Repeated per-rule traversals and handle round-trips.** Production parses each file once (`runner.rs:120`), then runs 36 code rules sequentially (`runner.rs:132–144`), each re-walking the tree: per Python file, `extract_function_signatures` runs 7 times, `has_unaliased_collections_abc_set_import` 4 times, `collect_bindings` 4 times, `find_banned_calls` 10 times, `extract_classes` 3 times, and `collect_class_attributes` 2 times. Rules also pass `AstNode` handles back into 25+ `ast` helpers for upward context or subtree inspection (§4.1).

## 2. Prototype comparison (P1 vs. P2 vs. P3)

All three prototypes (`scratch/proto_p{1,2,3}/REPORT.md`, base commit `plvwzpxq`) implemented the same four vertical slices (`has_syntax_error` + comments; Rust inline test ranges + attributes; call candidates + `os.environ` subscripts + enclosing function; Python and Rust `collect_literal_occurrences`) and passed the full verification gate with 0 inherited test cases modified:

| Dimension | P1: In-tree `Kind`/`Field` enums (`u16` IDs) | P2: `type-sitter` 0.10.1 codegen | P3: `ruff_python_ast` + `ra_ap_syntax` |
| :--- | :--- | :--- | :--- |
| **Status** | Verified (`uwyqsrnt`) | **Rejected** (`vpvvompx`) | **Selected** (`noxlrwmu`) |
| **F1 / F4 (string kinds)** | Fixed (runtime test + numeric IDs) | Fixed (compile-time types) | Fixed (`Stmt`/`Expr`/`Pattern`, `ast::Fn`/`ast::Attr`) |
| **F2 (CST shape quirks)** | **Not fixed** (kept all normalizers + 10 golden CST tests) | **Not fixed** (same Tree-sitter CST) | **Fixed** (`Subscript` unified, `Parameters` structured, `UnaryOp(USub)` in patterns, `HasAttrs` in Rust, decoded strings/f-strings) |
| **F3 (exhaustiveness)** | Partial (flat 200+ token/node enum) | Partial (per-node child unions) | **Fixed** (scoped Rust enums per syntactic category) |
| **`unsafe` blocks** | 0 | **940** across 57,806 generated lines | 0 in Omni (`unsafe_code = "forbid"` preserved) |
| **`Cargo.lock` crates** | 126 (−24 vs. 150 base, F5 trimmed) | 145 (pins yanked `enum-map 3.0.0-beta.2` and `ast-grep =0.44.1`) | 203 (+53; see §5.1 for without `ast-grep`) |

**Shortcuts in the P3 prototype that the production design must not carry over:**
1. `ParsedFile` kept `AstGrep` alongside both new parsers (double-parsing every file) because only 4 slices were migrated.
2. `AstNode` became a struct with four `Option` fields and `#[allow(clippy::struct_field_names)]`.
3. `Cargo.toml` added a workspace-wide `multiple_crate_versions = "allow"`.
4. `python::collect_literal_occurrences` artificially skipped negative numbers in mapping/keyword patterns and sliced raw string text to mimic Tree-sitter's undecoded literals.

## 3. Goals and non-goals

- **Goals**
  - **G1.** Replace `ast-grep-core` and `ast-grep-language` in `code_lint` with `ruff_python_parser` + `ruff_python_ast` (Python) and `ra_ap_syntax` (Rust), and remove all 10 `omni:disable-file [repeated-literal]` directives in `src/code_lint/ast*`.
  - **G2.** Replace `ast_grep_language::SupportLang` across `code_lint`, `rule_declaration`, `config`, and `test_utils` with an Omni-owned `Language` enum in `diagnostic`, enforced by `tests/architecture_conformance.rs`.
  - **G3.** Preserve `unsafe_code = "forbid"`, keep `cargo publish --locked` working (crates.io dependencies only, no `git` dependencies), and keep the full verification gate green after every commit.
  - **G4.** Preserve existing rule behavior across all 37 `CodeRule`s and 4 suppression audits, except where P3 closes `known_gap_negative_numbers_in_mapping_and_keyword_patterns` and replaces Tree-sitter's undecoded string literal workaround with native escape-decoded values.
  - **G5.** Refine architecture, abstractions, and boundaries: delete all Tree-sitter CST normalizers, tighten `code_lint::ast` visibility, eliminate avoidable upward `AstNode` round-trips, memoize shared file-level domain facts on `ParsedFile`, and provide declarative rule helpers (§5.3–5.5).
  - **G6.** **Zero legacy or backward-compatibility bloat** at completion, verified by a dedicated Phase 5/6 audit across all touched files.
  - **G7.** Once `code_lint` is off `ast-grep`, trim `ast-grep-language` in `Cargo.toml` to `default-features = false, features = ["tree-sitter-bash"]` (removing 27 of 28 C/C++ grammars) and restrict `AST_GREP_OWNERS` in `tests/architecture_conformance.rs` to `CommandLintCommand` only.
- **Non-goals**
  - **NG1.** Cross-file type inference or full import/type-alias symbol resolution (`ROADMAP.md:52–61,87,90`), which remain separate follow-ups after the AST migration.
  - **NG2.** Re-enabling `call-before-definition` (`ROADMAP.md:94–101`). Its AST extractor (`scopes.rs`) is migrated alongside the rest of `ast/python`.
  - **NG3.** Expanding Rust procedural/declarative macro bodies (`TokenTree`); `ra_ap_syntax` keeps macro arguments as `ast::TokenTree` just like Tree-sitter (§4.3).
  - **NG4.** Migrating `command_lint::command` off `ast-grep` in this project; the full shell parser comparison (`brush-parser` vs. `tree-sitter-bash` vs. in-tree parser) and `command_rule_test!` design are recorded in [ROADMAP.md](../../../ROADMAP.md#L20-L27) and §5.6 for the dedicated `command_lint` phase.

## 4. Inventory

### 4.1 Syntax provider (`src/code_lint/ast*`, `semantic/*`, `command_lint/command.rs`)

Counts at `lnwtoytpwkmx` (`prod` = lines before `#[cfg(test)]`; `kind/fld` = `.kind()`, `.field("...")`, and kind-string literals; `up` = `.parent()` + `.ancestors()`; `side` = `.prev()` / `.next()`; `&AstNode in` = functions taking `&AstNode` as input):

| File | Total | Prod | Test | `pub` items | `kind/fld` | `up` | `side` | `&AstNode` in | P3 impact |
| :--- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | :--- |
| [ast.rs](../../../src/code_lint/ast.rs) | 565 | 498 | 67 | 38 | 8 | 4 | 0 | 4 | Owns `ParsedFile`, `AstNode`, `LineIndex`, cross-language dispatch; `delimited_string_parts` and `find_pattern_calls` deleted |
| [ast/python.rs](../../../src/code_lint/ast/python.rs) | 2,965 | 1,634 | 1,331 | 35 | 217 | 25 | 0 | 10 | Decorators on `StmtFunctionDef`/`StmtClassDef`; literals use native decoded values; subscripts and usage walks match `Stmt`/`Expr` |
| [python/annotations.rs](../../../src/code_lint/ast/python/annotations.rs) | 627 | 627 | 0 | 18 | 29 | 0 | 0 | 5 | `unwrap_type_and_parens` and `generic_type` vs `subscript` split disappear; `Final[T]` is `Expr::Subscript` |
| [python/classes.rs](../../../src/code_lint/ast/python/classes.rs) | 469 | 469 | 0 | 15 | 53 | 2 | 0 | 1 | `StmtClassDef.arguments` + `.decorator_list` + `.body` replace `decorated_definition` and `argument_list` walks |
| [python/format_strings.rs](../../../src/code_lint/ast/python/format_strings.rs) | 513 | 513 | 0 | 6 | 23 | 5 | 0 | 0 | `ExprFString` provides `FStringPart`, `.conversion`, `.format_spec`, `.debug_text`; `.format()` and `%` string parsers stay on `&str` |
| [python/functions.rs](../../../src/code_lint/ast/python/functions.rs) | 469 | 469 | 0 | 19 | 61 | 1 | 0 | 5 | `Parameters` (`posonlyargs`, `args`, `vararg`, `kwonlyargs`, `kwarg`) replaces the 80-line `parse_param_parts` |
| [python/logging.rs](../../../src/code_lint/ast/python/logging.rs) | 134 | 134 | 0 | 3 | 13 | 0 | 0 | 0 | Matches `ExprCall` (`func`, `arguments.args`, `arguments.keywords`) |
| [python/scopes.rs](../../../src/code_lint/ast/python/scopes.rs) | 746 | 746 | 0 | 7 | 79 | 0 | 0 | 0 | `PatternMatchAs`, `PatternMatchStar`, `PatternMatchValue` replace `dotted_name` dot-text heuristics |
| [python/strings.rs](../../../src/code_lint/ast/python/strings.rs) | 189 | 189 | 0 | 8 | 12 | 2 | 1 | 0 | Most of file disappears: `ExprStringLiteral` / `ExprBytesLiteral` / `ExprFString` carry `Flags`, implicit-concat parts, and decoded values (handles `\N{...}` natively) |
| [ast/rust.rs](../../../src/code_lint/ast/rust.rs) | 1,927 | 1,598 | 329 | 33 | 175 | 9 | 7 | 11 | `HasAttrs` removes sibling `preceding_attributes` walks; `ast::Fn`/`ast::Impl`/`ast::UseTree`/`ast::Literal` replace string matching; `TokenTree` scans stay |
| [ast/statements.rs](../../../src/code_lint/ast/statements.rs) | 264 | 94 | 170 | 3 | 2 | 2 | 0 | 0 | Statement header span computed directly from `Stmt` / `ast::Item` + attached decorators/attributes |
| [semantic/bindings.rs](../../../src/code_lint/semantic/bindings.rs) | 92 | 92 | 0 | 3 | 0 | 0 | 0 | 0 | Consumes `ast::collect_bindings`; simpler once `Binding` carries its `BindingKind` (§5.3) |
| [semantic/calls.rs](../../../src/code_lint/semantic/calls.rs) | 201 | 109 | 92 | 2 | 0 | 0 | 0 | 0 | Replaces `ast::find_pattern_calls` with native `CallPattern` matching (§5.4) |
| [semantic/comments.rs](../../../src/code_lint/semantic/comments.rs) | 551 | 298 | 253 | 9 | 0 | 0 | 0 | 0 | Unchanged; consumes `collect_comment_nodes` and `enclosing_statement_header_range` |
| [command_lint/command.rs](../../../src/command_lint/command.rs) | 363 | 230 | 133 | 10 | 4 | 0 | 0 | 0 | Deferred to `ROADMAP.md` (D8); sole remaining user of `ast-grep` (`tree-sitter-bash` only) |
| **Total** | **10,083** | **8,101** | **1,982** | **209** | **676** | **50** | **8** | **36** | |

**Dead or over-exposed `pub` surface in `code_lint::ast` found by the sweep (to be cleaned up):**
- Used only by unit tests: `KeywordArg::as_bool` (`python.rs:168`), `PythonClassInfo::inherits_from` (`classes.rs:70`), `extract_parameters` (`functions.rs:273`).
- Used only inside `ast/` (plus unit tests): `extract_decorators`, `has_decorator`, `find_enclosing_with_item`, `find_enclosing_with_statement`, `collect_outer_test_functions`, `has_override_decorator`, `DecoratorInfo::get_arg`, and `rust::{is_statement_container, decorated_definition, is_comment_kind, is_import_binding_parent, is_structural_definition_parent, is_call_kind}`.
- Seven functions are exported both as `ast::X` and `ast::{python,rust}::X`: `collect_positional_reads`, `collect_literal_occurrences`, `collect_bindings`, `is_trait_impl_member`, `collect_test_function_assertion_counts`, `find_unwrapped_multiline_strings`.

### 4.2 Consumers outside `code_lint::ast`

- **37 `CodeRule` definitions in 36 files** (`src/code_lint/rules/*.rs`; 36 registered in `CODE_RULES` at `rules.rs:44–83`, plus `call-before-definition` commented out at `rules.rs:79–80`):
  - **Languages:** 13 Python + Rust, 24 Python-only.
  - **Check shapes:** 24 rules call a single batch query and map/filter into diagnostics; 13 rules join 2–4 queries or pass an `AstNode` from one query into a second `ast` helper (`collect_collection_types`, `return_type_union`, `extract_generic_type`, `collection_type`, `collection_display`, `call_callee`, `unwrap_return_envelope`, `extract_option_payload`, `is_parameter_mutated_or_escaping`, `analyze_parameter_collection_capability`, `is_inside_except_clause`, `is_with_context_manager`, `enclosing_non_exempt_function_name`, and the `packed-assertion` helpers).
  - **Size breakdown across the 36 rule files (~11.0k lines):** ~35% `Declaration` + docs, ~13% check logic, ~52% `rule_test!` cases.
- **4 suppression audits** (`src/code_lint/suppression.rs:27–183`): consume `ast::collect_comment_nodes` and `AstNode::{text, span, start_coordinate}`.
- **Test harness (`src/test_utils.rs`):** `rule_test!` parses each case via `ParsedFile::new`, asserts exact flagged text slices and byte-offset shifts under `{code}\n{code}`, and uses `collect_literal_occurrences` + `has_syntax_error` for `RepeatCheck::DistinctLiterals` (`test_utils.rs:220–270`).
- **Architecture conformance (`tests/architecture_conformance.rs`):** calls `ParsedFile::rust` + `ast::rust::summarize_rust_file` on every `src/**/*.rs` file (`L32–44`) and checks `AST_GREP_OWNERS` (`L20,426–430`).
- **Developer binary (`src/bin/ast_dumper.rs`):** 42-line debug CLI importing `ast_grep_core` and `SupportLang` directly (to be updated to dump `ruff_python_ast` / `ra_ap_syntax` trees for `.py` / `.rs` files).

### 4.3 Documented `known_gap_` test cases vs. P3

| Test case (`file:line`) | Cause | P3 outcome |
| :--- | :--- | :--- |
| `known_gap_negative_numbers_in_mapping_and_keyword_patterns` ([repeated_literal.rs:283](../../../src/code_lint/rules/repeated_literal.rs#L283)) | `tree-sitter-python` emits a bare `"-"` token + `integer` inside `dict_pattern` and `keyword_pattern` | **Fixed automatically:** `ruff_python_ast` represents `-404` as `Expr::UnaryOp(USub, ExprNumberLiteral)` in both patterns (promote to `fail` in Slice 5) |
| `known_gap_string_annotation_not_parsed` ([concrete_collection_parameter.rs:263](../../../src/code_lint/rules/concrete_collection_parameter.rs#L263)) | Forward-reference string annotation `items: "list[int]"` is a string literal node | **Post-migration follow-up commit:** `ruff_python_parser::typing::parse_type_annotation` can sub-parse string annotations with source offsets |
| `known_gap_values_inside_exempt_macros` ([repeated_literal.rs:443](../../../src/code_lint/rules/repeated_literal.rs#L443)) | Whole exempt macro (`assert_eq!`, `format!`) is skipped | **Remains:** needs per-macro format-string position table |
| `known_gap_negative_numbers_in_macros` ([repeated_literal.rs:449](../../../src/code_lint/rules/repeated_literal.rs#L449)) | `vec![-42]` is an unparsed `TokenTree` where `-` and `42` are separate tokens | **Remains** unless macro `TokenTree`s are re-parsed as expressions |
| `known_gap_macro_arguments_not_inspected` ([repeated_index_access.rs:393](../../../src/code_lint/rules/repeated_index_access.rs#L393)) | `assert_eq!(t.0, t.1)` is an unparsed `TokenTree` | **Remains** for the same reason |
| `known_gap_typing_module_alias_not_resolved` ([concrete_collection_parameter.rs:267](../../../src/code_lint/rules/concrete_collection_parameter.rs#L267)) | `import typing as t; t.List[int]` needs import-alias resolution | **Remains** (ROADMAP "Import-Aware Qualified Call Resolution") |
| `known_gap_same_named_callee_exempts_function` ([mutable_collection_return.rs:227](../../../src/code_lint/rules/mutable_collection_return.rs#L227)) | Name-based callee matching without symbol resolution | **Remains** |

*(Note: earlier chat discussion mentioned bugs S4, C2, and C3 from [python_ast_consolidation](../python_ast_consolidation/01_understand.md); those three were already fixed on `main` in commit `nynwwkrplwsn` via Tree-sitter workarounds in `strings.rs:127–150` and `scopes.rs:181,371–384,447`. P3 replaces those workarounds with native AST fields.)*

## 5. Design dimensions and options

### 5.1 Crate versions, `rustc` toolchain upgrade, and `cargo publish` compatibility

- **Why upgrading `rustc` (`1.90.0 -> 1.99.0`) is required for crates.io publishing:**
  - Commit `uzoqysyx` added [.github/workflows/release.yml:130](../../../.github/workflows/release.yml#L130) with `cargo publish --locked`. Crates.io **rejects any crate that uses `git = "..."` dependencies**.
  - `ruff_python_parser = "=0.0.16"`, `ruff_python_ast = "=0.0.16"`, and `ruff_text_size = "=0.0.2"` **are published on crates.io** with `rust-version = "1.97"`.
  - `rustup check` on this machine confirmed `stable-x86_64-unknown-linux-gnu: 1.90.0 -> 1.99.0` is ready to install via `rustup update stable`.
  - Upgrading the local toolchain to `1.99.0` allows Omni to depend on published crates.io versions (`ruff_python_parser = "=0.0.16"`, `ruff_python_ast = "=0.0.16"`, `ruff_text_size = "=0.0.2"`, and `ra_ap_syntax = "=0.0.357"` or `"=0.0.307"`) with **zero `git` dependencies**.
- **Duplicate transitive versions (`hashbrown`, `rustc-hash`):**
  - Handled with a targeted `#[allow(clippy::multiple_crate_versions)]` on `src/lib.rs` documenting the specific transitive pair, keeping `Cargo.toml` lint levels strict.

### 5.2 Omni `Language` enum (replacing `ast_grep_language::SupportLang`)

Define `pub enum Language { Python, Rust }` in `crate::diagnostic` (the bottom of the DAG, already imported by `RuleDeclaration`, `Config`, and `CodeLintAst`):
- Implements `Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Display, EnumString, AsRefStr, VariantArray` (`strum` is already in `Cargo.toml`).
- `violation_template!`, `rule_test!`, `Declaration`, `Example`, and `LanguageDefaults` switch from `SupportLang` to `Language` without changing rule syntax (`Python => ...`, `Language::Python`).
- `tests/architecture_conformance.rs` forbids `ruff_python_*` and `ra_ap_syntax` outside `code_lint::ast` and `bin::ast_dumper`, and restricts `ast_grep_*` to `command_lint::command` only.

### 5.3 `ParsedFile`, `AstNode<'a>`, upward navigation, and native literal decoding

- **`ParsedFile`:**
  - Holds `source: String`, a shared `LineIndex` (byte offsets of `\n`, built once for $O(\log \text{lines})$ `LineColumn` lookup), `tree: ParsedTree` (`Parsed<ModModule>` for Python, `Parse<ast::SourceFile>` for Rust), and `OnceLock` slots for shared file-level domain facts (§5.5).
- **`AstNode<'a>`:**
  ```rust
  pub struct AstNode<'a> {
      pub(in crate::code_lint::ast) file: &'a ParsedFile,
      pub(in crate::code_lint::ast) kind: AstNodeKind<'a>,
  }
  pub(in crate::code_lint::ast) enum AstNodeKind<'a> {
      Python(AnyNodeRef<'a>),
      Rust(ra_ap_syntax::SyntaxNode),
      Span(SourceSpan), // tokens, comments, and multi-token spans (e.g. negative literals)
  }
  ```
  `ruff_python_ast::AnyNodeRef<'a>` is a `Copy` enum over every `&'a Stmt`, `&'a Expr`, `&'a Pattern`, `&'a Parameter`, `&'a Decorator`, etc., so helpers taking `&AstNode<'a>` match on `AnyNodeRef` or `SyntaxNode` directly in $O(1)$.
- **Upward navigation in Python (`ruff_python_ast` has no `.parent()`):**
  - AST node `TextRange`s nest strictly and siblings are disjoint. Finding the ancestor chain of any `TextRange` from `ModModule` is a top-down walk that only descends into the single child whose `range().contains_range(target)` — $O(\text{depth}) \approx 5\text{–}15$ node visits and **zero memory overhead**.
  - Precomputing `BindingKind` on `Binding` (`collect_bindings`) and `is_in_protocol_or_abc` on `PythonFunctionSignature` eliminates 4 of the most frequent upward round-trips (`is_import_binding`, `is_structural_definition`, `is_trait_impl_member`, `is_in_protocol_or_abc_class`) altogether.
- **Native escape-decoded `LiteralValue::Str` / `LiteralValue::Bytes` (zero legacy Tree-sitter workarounds):**
  - Previously, `LiteralValue::Str` stored raw source text between quotes and manually doubled `\` in raw strings (`python.rs:1490–1492`, `rust.rs:1263–1267`) only because Tree-sitter did not decode escapes.
  - Under P3, `LiteralValue::Str(String)` and `LiteralValue::Bytes(Vec<u8>)` store the **parser-decoded semantic value** produced natively by `ruff_python_ast` (`ExprStringLiteral.value.to_str()`, `ExprBytesLiteral.value`) and `ra_ap_syntax` (`ast::String::value()`, `ast::ByteString::value()`, `ast::CString::value()`).
  - Consequence: `normalize_string_content`, `rust_string_body`, `delimited_string_parts`, and raw-string backslash doubling are deleted completely; `"a\nb"` and `r"a\nb"` are naturally distinct decoded strings (`"a\nb"` vs `"a\\nb"`), while `"\x41"` and `"A"` are recognized as the same runtime string `"A"`. The two unit test cases in `python.rs` (`case_06_escapes_kept`, `case_07_raw_backslash_spelled_plain`) that asserted the old undecoded workaround are updated to assert decoded values.

### 5.4 Replacing `ast::find_pattern_calls` (`semantic/calls.rs`) with a clean `CallPattern` model

With no backward-compatibility requirement for `ast-grep`'s `$OBJ` / `$LOOP($$$LOOP_ARGS)` metavariable DSL:
- Delete `ast::find_pattern_calls` (`ast.rs:378–400`) completely.
- Enrich `CallCandidate` in `code_lint::ast` with `receiver_call_callee: Option<String>` (set when a method call's receiver is itself a call expression, e.g. `asyncio.get_event_loop().create_task(...)` → `callee = "asyncio.get_event_loop().create_task"` or `method_name = Some("create_task")`, `receiver_call_callee = Some("asyncio.get_event_loop")`).
- Replace the `$` metavariable strings in `semantic::calls::find_banned_calls` with an explicit, readable `CallPattern` syntax matched directly over `collect_call_candidates(file)`:
  1. `"pkg.func"` / `"pkg::func"` — exact callee path (`time.sleep`, `tokio::time::sleep`).
  2. `"*.method"` — method call on any receiver (`"*.assert_called_once"` in `mock-call-assertion`, replacing `"$OBJ.assert_called_once"`).
  3. `"*().method"` or `"pkg.func().method"` — method call chained on the return value of a call (`"*().create_task"` in `unstructured-task`, replacing `"$LOOP($$$LOOP_ARGS).create_task"`).

### 5.5 Architecture, boundary, and rule-authoring refinements

1. **Tighten `code_lint::ast` boundary visibility:**
   - Demote the ~20 `pub` helpers in `ast/{python,rust}` that are only used inside `ast` (or unit tests) to `pub(super)` / `pub(in crate::code_lint::ast)`.
   - Remove duplicate public exports where the same function is exported on both `ast::X` and `ast::{python,rust}::X`.
2. **Memoize shared file-level domain facts on `ParsedFile` (`OnceLock`):**
   - Cache `function_signatures`, `classes`, `class_attributes`, `bindings`, `call_candidates`, `comment_nodes`, `inline_test_ranges`, and `has_unaliased_collections_abc_set_import` on `ParsedFile`.
   - Each shared extractor runs **at most once per file** across all 36 rules without changing the `CodeRule` signature or breaking `CodeLintAst` encapsulation.
3. **Declarative rule constructors/helpers:**
   - Add shared helpers on `CodeRule` / `code_lint::policy` for common rule families (banned calls with context predicate, suffix/segment binding checks, collection signature checks).

### 5.6 `command_lint` Bash parser options (deferred to `ROADMAP.md`)

Per **D8**, migrating `src/command_lint/command.rs` off `ast-grep` is deferred to a dedicated follow-up tracked in [ROADMAP.md](../../../ROADMAP.md#L20-L27). In this project, once `code_lint` drops `ast-grep`, we trim `ast-grep-language` in `Cargo.toml` to `default-features = false, features = ["tree-sitter-bash"]` (dropping 27 of 28 C/C++ grammars immediately) and restrict `AST_GREP_OWNERS` in `tests/architecture_conformance.rs` to `CommandLintCommand`.

For reference when the `command_lint` project starts, the verified options and consequences are:
- **Disqualified crates (`docs.rs`):** `yash-syntax` (`0.25.0`), `mystsh` (`0.0.3`), and `bash-ast` are `GPL-3.0-or-later` (incompatible with `omni-lint`'s `MIT` license); `conch-parser` (`0.1.1`) is unmaintained (2018) and POSIX-only.
- **Viable options:**
  1. **Option A (`brush-parser = "0.4.0"`, `MIT`, pure Rust PEG):** 0 C compilers; typed POSIX/Bash AST out of the box; +~15 pure-Rust crates in `Cargo.lock`; `char`-indexed spans (`source.rs:405`) needing a 3-line byte-offset helper; returns `Err` on malformed shell syntax.
  2. **Option B (Direct `tree-sitter` + `tree-sitter-bash = "0.25"`, `MIT`):** 0 new crates; error-tolerant CST with native byte spans; keeps 1 C grammar (`cc` build script) and requires a ~150–200 line CST lowering module + `shell-words`.
  3. **Option C (In-tree shell lexer + recursive-descent parser):** 0 external crates, 0 C compilers, native byte spans and quote kinds, ~350–450 lines of parser code in `src/command_lint/`.

## 6. Proposed phasing

Every slice must leave `cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test && cargo doc --no-deps --document-private-items` green:

1. **Slice 0 — Toolchain update:** Update local `rustc` to `1.99.0` (`rustup update stable`) so crates.io `ruff_python_parser = "=0.0.16"` and `ruff_python_ast = "=0.0.16"` compile without `git` dependencies.
2. **Slice 1 — Omni `Language` enum:** Replace `ast_grep_language::SupportLang` outside `code_lint::ast` and `command_lint::command` with `crate::diagnostic::Language` (50 files, mechanical, zero behavior change).
3. **Slice 2 — Core `ParsedFile`, `AstNode<'a>`, `LineIndex`, and `CallPattern` matcher:** Introduce `ruff_python_parser`/`ruff_python_ast` and `ra_ap_syntax` inside `code_lint::ast`, implement the $O(\text{depth})$ Python ancestor walk, and replace `ast::find_pattern_calls` with native `CallPattern` matching (`"*.method"`, `"*().method"`) in `semantic/calls.rs`.
4. **Slice 3 — Migrate `ast/rust.rs` and `ast/statements.rs` to `ra_ap_syntax`:** Migrate all Rust extractors (inline test ranges, attributes via `HasAttrs`, bindings, calls, macros, escape-decoded literals, positional reads, functions/types, and `summarize_rust_file`), then lift `omni:disable-file [repeated-literal]` on `rust.rs`.
5. **Slice 4 — Migrate `ast/python/*` submodules (`strings`, `format_strings`, `logging`, `annotations`, `classes`, `functions`, `scopes`) to `ruff_python_ast`:** One submodule group per commit, deleting the Tree-sitter CST normalizers (`unwrap_type_and_parens`, `parse_param_parts`, `strip_named_unicode_escapes`, `decorated_definition`) and lifting `omni:disable-file [repeated-literal]` on each file.
6. **Slice 5 — Migrate `ast/python.rs` root extractors and drop `ast-grep` from `code_lint`:** Migrate remaining `python.rs` walkers (escape-decoded literals, positional reads, capability/mutation analysis, mutable module assignments), promote `known_gap_negative_numbers_in_mapping_and_keyword_patterns` to a `fail` test, remove `AstGrep` from `ParsedFile`, lift the remaining `omni:disable-file` directives, trim `ast-grep-language` to `tree-sitter-bash` only, and restrict `AST_GREP_OWNERS` to `CommandLintCommand`.
7. **Slice 6 — Architecture & boundary refinement + `ParsedFile` fact memoization + Zero-Legacy Audit:** Memoize shared file-level queries (`OnceLock`) on `ParsedFile`, tighten `code_lint::ast` visibility (`pub` → `pub(in crate::code_lint::ast)`), remove duplicate exports, add declarative rule helpers, and execute the **Phase 5/6 Zero-Legacy & Zero-Compat Bloat Audit**.

## 7. Decisions (all resolved)

- **D1 (P3 architecture):** Proceed with **P3** (`ruff_python_parser` + `ruff_python_ast` for Python, `ra_ap_syntax` for Rust) in `code_lint`.
- **D2 (`rustc` update + crates.io dependencies — Q1):** Update local `rustc` (`1.90.0 -> 1.99.0` via `rustup update stable`) and use crates.io packages (`ruff_python_parser = "=0.0.16"`, `ruff_python_ast = "=0.0.16"`, `ruff_text_size = "=0.0.2"`, `ra_ap_syntax = "=0.0.357"`), preserving `cargo publish --locked` compatibility in `.github/workflows/release.yml`.
- **D3 (Omni `Language` enum — Q2):** Replace `ast_grep_language::SupportLang` with `crate::diagnostic::Language` across the crate (outside `command_lint::command`) as Slice 1.
- **D4 (`CallPattern` replacing `find_pattern_calls` with zero legacy DSL — Q3):** Drop `ast-grep` `$VAR` / `$$$ARGS` pattern syntax completely; replace with a clean, native `CallPattern` matcher (`"pkg.func"`, `"*.method"`, `"*().method"` / `"pkg.func().method"`).
- **D5 (`known_gap_` handling — Q4):** Promote `known_gap_negative_numbers_in_mapping_and_keyword_patterns` (`repeated_literal.rs:283`) to a `fail` test in Slice 5; defer `known_gap_string_annotation_not_parsed` (`concrete_collection_parameter.rs:263`) to a post-migration commit.
- **D6 (Decoded `LiteralValue::Str` / `Bytes` & zero legacy bloat — Q5):** Use native parser-decoded string/byte values in `LiteralValue::Str` and `LiteralValue::Bytes` (deleting raw-slice and backslash-doubling shims), and execute a mandatory **Zero-Legacy / Zero-Compat Bloat Audit** in Phase 5 and Phase 6.
- **D7 (Architecture, boundaries, and rule ergonomics — Q6):** Keep `CodeLintAst` encapsulated, tighten `code_lint::ast` visibility, move binding/class-context facts onto domain structs, memoize shared file queries on `ParsedFile` (`OnceLock`), and add declarative rule helpers.
- **D8 (`command_lint` deferred to `ROADMAP.md` — Q7):** Defer `command_lint`'s shell parser & `command_rule_test!` migration to a dedicated follow-up project in [ROADMAP.md](../../../ROADMAP.md#L20-L27); in Slice 5 of this project, trim `ast-grep-language` to `default-features = false, features = ["tree-sitter-bash"]` and restrict `AST_GREP_OWNERS` to `CommandLintCommand`.

## 8. Side notes

- **Unmerged exploration commit `lxslyzoskzpz` and `scratch/proto_p{1,2,3}`:** Commit `lxslyzoskzpz` holds the earlier exploration docs (`01_understand.md`, `02_references.md`, `02b_prototype_comparison.md`), and `scratch/proto_p{1,2,3}/REPORT.md` hold the final prototype reports. Once Phase 2 (`02_references.md`) records the final prototype comparison table here, we can forget the three `proto_p{1,2,3}` jj workspaces and abandon `lxslyzoskzpz`.
- **`ROADMAP.md` updates:** Updated in [ROADMAP.md:18–35](../../../ROADMAP.md#L18-L35).

