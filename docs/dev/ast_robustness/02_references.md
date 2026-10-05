# Phase 2 — References, Crate Evaluation & Prototype Comparison

> **Status**: COMPLETE (all three prototypes `P1`, `P2`, `P3` finished and verified; `rustc 1.99.0` upgraded; crates.io `ruff_python_parser = "=0.0.16"` and `ra_ap_syntax = "=0.0.357"` verified)

---

## 1. How Production Rust Linters Solve This Problem

| Linter | Target Language(s) | Parser / Tree Representation | How Node Kinds & Fields Are Typed | How Trivia / Comments Are Handled |
| :--- | :--- | :--- | :--- | :--- |
| **Ruff (`astral-sh/ruff`)** | Python | Hand-written recursive-descent parser (`ruff_python_parser`) producing a strongly typed, arena/box-allocated AST (`ruff_python_ast`) | Exhaustive Rust `enum`s (`Stmt`, `Expr`, `Pattern`, `CmpOp`, `UnaryOp`, `BoolOp`, `Operator`) and structs (`StmtFunctionDef`, `ExprCall`, `Parameters`, `Arguments`, `Decorator`, `Alias`) with typed fields — **zero string node kinds** | Separate token/comment stream (`Parsed::tokens()` yielding `Token` items with `TokenKind::Comment` and `TextRange`) |
| **rust-analyzer (`ra_ap_syntax`)** | Rust | Hand-written error-recovering parser producing a lossless **Rowan CST** (`SyntaxNode` / `SyntaxToken` keyed by `SyntaxKind` enum) wrapped by generated **typed AST facade structs** (`ra_ap_syntax::ast::*`) | Two-layer typing: exhaustive `SyntaxKind` enum at the token/CST layer + zero-cost typed wrapper structs (`ast::Fn`, `ast::Struct`, `ast::Impl`, `ast::Attr`, `ast::MacroCall`, `ast::Literal`, `ast::Comment`) generated from `rust.ungram` | Comments and whitespace are first-class `SyntaxToken`s (`SyntaxKind::COMMENT`, `SyntaxKind::WHITESPACE`) inside the CST, with `ast::Comment` wrappers |
| **Biome (`biomejs/biome`)** | JS/TS, JSON, CSS, GraphQL | Fork of rust-analyzer's `rowan` (`biome_rowan`) + codegen from `.ungram` grammar files | Exhaustive `SyntaxKind` enum + generated typed AST structs (`JsCallExpression`, `JsFunctionDeclaration`) where every field accessor returns `SyntaxResult<T>` | Trivia attached directly to tokens in the lossless CST |
| **oxlint (`oxc-project/oxc`)** | JS/TS | Hand-written arena-allocated AST (`oxc_ast`) + generated `AstKind` enum | Exhaustive Rust enums/structs + typed `AstKind` parent stack recorded during traversal | Separate trivia table indexed by byte offset |

### Architectural Takeaways

1. **No production linter writes rules against untyped `node.kind() == "..."` and `node.field("...")` strings.** Every mature Rust linter uses either:
   - A **dedicated language AST** (`ruff_python_ast`, `oxc_ast`), or
   - A **lossless CST paired with generated typed AST wrappers** (`ra_ap_syntax`, `biome_rowan`).
2. **Why `ast-grep` is different**: `ast-grep` was designed for **end-user YAML/CLI structural search and replace across 25+ languages**, where the user writes a surface syntax pattern (`$A && $A()`) and does not walk AST nodes in Rust. When used as an internal compiler frontend for a static analyzer with 363 kind/field inspection sites, `ast-grep` leaks Tree-sitter's stringly-typed grammar vocabulary and CST quirks (`F1`–`F5`).

---

## 2. Candidate Crate Evaluation

### 2.1 Dedicated Python Parsers in Rust

| Crate | Crates.io Version | Typed AST | Byte Spans (`R2`) | Comments (`R3`) | Error Recovery (`R4`) | Parent Links (`R5`) | Verdict |
| :--- | :--- | :--- | :--- | :--- | :--- | :--- | :--- |
| **`ruff_python_parser` + `ruff_python_ast` + `ruff_text_size`** | `0.0.16` / `0.0.2` (`rust-version = 1.97`, MIT) | **Yes** — `ModModule`, `Stmt`, `Expr`, `Pattern`, `Parameters`, `Arguments`, `Decorator`, `AnyNodeRef`, `Visitor`, `StatementVisitor` | **Yes** — `Ranged::range()` returns `TextRange` (`u32` byte offsets) on every AST node and token | **Yes** — `Parsed::tokens()` yields all `Token`s including `TokenKind::Comment` with exact `TextRange` | **Yes** — full error-recovering parser (`parse_module(src)` always returns a `Parsed<ModModule>` + `errors()` slice) | **No** — top-down tree; requires visitor context stacks or $O(\text{depth})$ range-containment walk from `ModModule` | **Selected Python parser** |
| **`rustpython-parser` + `rustpython-ast`** | `0.4.0` | Yes | Yes (`text_size::TextRange`) | **No** — lexer discards `#` comments during tokenization | **No** — LALRPOP parser aborts on the first syntax error | No | **Reject** — violates `R3` (comments) and `R4` (error recovery) |
| **`libcst` (`libcst_native`)** | `1.8.6` | Yes (concrete syntax tree) | Requires line/col conversion | Yes (attached to leading/trailing whitespace) | **No** — `parse_module` aborts on syntax error; pulls `pyo3` optional bindings | No | **Reject** — violates `R4`, awkward span model |

**Why `ruff_python_ast` eliminates Omni's Python CST bugs (`F2`)**:
- `Final[int]` and `typing.Final[int]` are **both** `Expr::Subscript(ExprSubscript { value, slice, .. })` (`value` is `Expr::Name("Final")` or `Expr::Attribute("typing.Final")`). Tree-sitter's split between `generic_type` and `subscript` does not exist.
- `case -404:`, `case {-404: _}:`, and `case Resp(code=-404):` parse `-404` as `Expr::UnaryOp(ExprUnaryOp { op: UnaryOp::USub, operand: Expr::NumberLiteral(..) })` — never a bare `"-"` sibling token.
- `Parameters` separates `posonlyargs`, `args`, `vararg`, `kwonlyargs`, and `kwarg` into typed `ParameterWithDefault` / `Parameter` structs with `.name()` and `.annotation()`, replacing 150 lines of `parse_param_parts` CST heuristics in `src/code_lint/ast/python.rs`.
- String literals (`Expr::StringLiteral`, `Expr::BytesLiteral`, `Expr::FString`) decode escape sequences into `.value.to_str()` / `.value.as_slice()`, track `.flags` (`StringLiteralFlags` / `BytesLiteralFlags` / `FStringFlags` with `.prefix()`), and distinguish implicit string concatenation (`s.value.is_implicit_concatenated()`).

### 2.2 Dedicated Rust Parsers in Rust

| Crate | Crates.io Version | Typed AST/CST | Byte Spans (`R2`) | Comments (`R3`) | Error Recovery (`R4`) | Parent Links (`R5`) | Verdict |
| :--- | :--- | :--- | :--- | :--- | :--- | :--- | :--- |
| **`ra_ap_syntax`** (rust-analyzer's `rowan` CST) | `0.0.357` (`rust-version = 1.98`, MIT OR Apache-2.0) | **Yes** — both typed `ast::*` wrappers (`ast::Fn`, `ast::Struct`, `ast::Enum`, `ast::Trait`, `ast::Impl`, `ast::Attr`, `ast::MacroCall`, `ast::Literal`, `ast::Comment`) and `SyntaxKind` enum | **Yes** — `TextRange` (`u32` byte offsets) on every `SyntaxNode` and `SyntaxToken` | **Yes** — `SyntaxKind::COMMENT` tokens in-tree; `ast::Comment::kind()` distinguishes `///`/`//!` doc comments from `//`/`/* */` | **Yes** — `SourceFile::parse(src, Edition::CURRENT)` always yields a full tree + `.errors()` | **Yes** — `rowan` CST provides `.parent()`, `.ancestors()`, `.children()`, `.children_with_tokens()`, `.siblings()` | **Selected Rust CST/AST** |
| **`syn` 2 (`full`, `visit`) + `proc-macro2`** | `2.0.x` | Yes (`syn::File`, `Item`, `Expr`) | Semver-exempt cfg required for byte ranges | **No** — strips `//` and `/* */` comments; converts `///` to `#[doc = "..."]` | **No** — `syn::parse_file` returns `Err` on first syntax error | No | **Reject** — violates `R3` (comments) and `R4` (error recovery) |
| **`rustc_ast`** | Nightly only | Yes | Yes | No | Partial | No | **Reject** — requires `#![feature(rustc_private)]` nightly compiler |

**Why `ra_ap_syntax` eliminates Omni's Rust CST bugs (`F2`)**:
- Outer and inner attributes (`#[cfg(test)]`, `#[test]`, `#[allow(...)]`) are **children** of the `ast::Fn` / `ast::Module` / `ast::Item` via `HasAttrs::attrs()`, and `item.syntax().text_range()` already covers the item's attributes. This eliminates the backward-sibling state machine in `collect_inline_test_ranges_rec`.
- `ast::Comment::cast(token)` provides `.kind().doc` (`Some(CommentPlacement::Inner | Outer)` vs `None`), directly distinguishing doc comments from regular comments without string prefix checks (`///`, `//!`, `/**`, `/*!`).
- Tuple field access (`pair.0`) is `ast::FieldExpr` (whose token is inside `ast::FieldExpr`, not an `ast::Literal` expression), distinct from numeric literal expressions.
- `ast::Literal` provides `.kind()` returning `ast::LiteralKind::{String, ByteString, CString, IntNumber, FloatNumber, Char, Byte, Bool}` with built-in value extraction (`.value()` decoding escape sequences and stripping raw-string delimiters).

### 2.3 Typed Tree-Sitter Alternatives Evaluated

| Approach | Crates | Verdict |
| :--- | :--- | :--- |
| **`type-sitter` (`0.10.1`)** | `type-sitter-lib`, `type-sitter-gen` | **Rejected (Prototype P2)** — generates ~1.8 MB of `unsafe transmute` code violating `unsafe_code = "forbid"`, depends on pre-release `enum-map = "^3.0.0-beta"`, pins `tree-sitter` to `0.26`, strips source text from raw nodes, and still exposes Tree-sitter's raw grammar quirks (`F2`). |
| **In-tree `NODE_TYPES` vocabulary (`PyKind`/`RsKind`/`PyField`/`RsField`)** | Existing `tree-sitter-python` / `tree-sitter-rust` | **Viable fallback (Prototype P1), superseded by P3** — replaces string literals with `u16`-backed enums validated against `SupportLang::get_ts_language()`, but preserves all Tree-sitter CST shape quirks (`F2`) and cannot unlock semantic AST representation for Python/Rust rules. |

---

## 3. Toolchain & Crates.io Verification (`rustc 1.99.0`)

1. **Release Workflow Constraint (`.github/workflows/release.yml:130`)**:
   - The `publish-crates` job runs `cargo publish --locked`.
   - Crates.io strictly rejects any package whose `Cargo.toml` contains a `git = "..."` dependency.
   - Therefore, all AST dependencies **must** come from crates.io.
2. **Toolchain Upgrade**:
   - Local toolchain updated from `rustc 1.90.0` to **`rustc 1.99.0 (b940084d7 2026-09-28)`** / **`cargo 1.99.0`**.
   - Verified via `cargo info`:
     - `ruff_python_parser = "=0.0.16"` (`rust-version = 1.97`, license `MIT`)
     - `ruff_python_ast = "=0.0.16"` (`rust-version = 1.97`, license `MIT`)
     - `ruff_text_size = "=0.0.2"` (license `MIT`)
     - `ra_ap_syntax = "=0.0.357"` (`rust-version = 1.98`, license `MIT OR Apache-2.0`)
   - Note on `Cargo.toml` / `.github/workflows/ci.yml`: CI uses `dtolnay/rust-toolchain@stable` (which installs `1.99.0`).

---

## 4. Prototype Comparison Summary (`P1` vs. `P2` vs. `P3`)

All three prototypes implemented the same 4 vertical slices on top of base commit `plvwzpxq` in isolated `jj` workspaces (`scratch/proto_p1`, `scratch/proto_p2`, `scratch/proto_p3`) and were completed to 100% passing gates (`cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test`, `cargo doc --no-deps --document-private-items`, including `test_self_dogfooding_code_lint`):

| Metric / Dimension | **P1 (`scratch/proto_p1`)** | **P2 (`scratch/proto_p2`)** | **P3 (`scratch/proto_p3`)** |
| :--- | :--- | :--- | :--- |
| **Commits & Report** | `uwyqsrnt` (`scratch/proto_p1/REPORT.md`) | `vpvvompx` (`scratch/proto_p2/REPORT.md`) | `noxlrwmu` (`scratch/proto_p3/REPORT.md`) |
| **All tests pass** | 1,078 lib + 11 arch + 35 cli + 16 registry + 3 doc | 1,067 lib + 11 arch + 35 cli + 16 registry + 3 doc | 1,067 lib + 11 arch + 35 cli + 16 registry + 3 doc |
| **Slice 1–4 LOC delta** | `454 -> 539` (`+85` LOC) + `523` LOC vocab module | `454 -> 902` (`+448` LOC) + `46,431` LOC generated | `454 -> 527` (`+73` LOC) |
| **`unsafe` code** | 0 | 200+ `unsafe { std::mem::transmute }` in generated wrappers | 0 |
| **Eliminates `F2` CST shape quirks** | No (locked down via 11 golden S-expression shape tests, but CST quirks remain in code) | No (same CST quirks + verbose `Option<Result<Union, IncorrectKind>>` unwrapping) | **Yes** (`Final[int]`, `case -404:`, Rust `#[cfg(test)]` child attributes, `pair.0` tuple fields) |
| **Verdict** | Rejected in favor of P3 | Rejected | **Selected for full migration** |

### Key Lessons from the P3 Prototype (`scratch/proto_p3`) for Production Design

The P3 prototype proved the viability of `ruff_python_ast` + `ra_ap_syntax`, and exposed 5 specific prototype shortcuts that we **must replace with clean production designs** in Phase 3:

1. **No double parsing (`AstGrep` alongside `Parsed<ModModule>` / `SourceFile`)**:
   - *Prototype shortcut*: Kept `AstGrep<StrDoc<SupportLang>>` inside `CodeLintAst` alongside the new ASTs because only 4 slices were migrated.
   - *Production design*: Migrate all 5 slices (`ast.rs`, `ast/rust.rs`, `ast/python.rs`, `semantic/calls.rs`, and rule callers) so `CodeLintAst` holds **only** `Python(PythonAst)` or `Rust(RustAst)` with zero `AstGrep` in `code_lint`.
2. **Clean `AstNode<'a>` sum type + `LineIndex` instead of 4-`Option` struct**:
   - *Prototype shortcut*: `AstNode<'a>` had 4 `Option` fields (`raw`, `py_node`, `rs_node`, `source_span`) and computed 0-indexed `(line, col)` via linear byte scans.
   - *Production design*: `AstNode<'a>` holds `file: &'a ParsedFile` and `kind: AstNodeKind<'a>` (`Python(AnyNodeRef<'a>) | Rust(SyntaxNode) | Span(SourceSpan)`), using a precomputed `LineIndex` (`Vec<u32>` of line-start byte offsets) on `ParsedFile` for $O(\log L)$ line/column lookup.
3. **Native decoded string/byte values in `LiteralValue` instead of raw-text re-slicing**:
   - *Prototype shortcut*: Re-sliced raw source text and ported Tree-sitter's `normalize_string_content`, `rust_string_body`, and `delimited_string_parts` to satisfy legacy raw-escape test expectations.
   - *Production design (Decision D6)*: Use `ruff_python_ast::ExprStringLiteral::value.to_str()`, `ExprBytesLiteral::value`, and `ra_ap_syntax::ast::String::value()` / `ByteString::value()` directly, deleting ~85 lines of manual delimiter/escape stripping.
4. **Promote `known_gap_negative_numbers_in_mapping_and_keyword_patterns` instead of artificial skipping**:
   - *Prototype shortcut*: Added `is_inside_non_match_value` in `LiteralVisitor::visit_pattern` solely to mimic Tree-sitter's bug so 0 inherited tests changed.
   - *Production design (Decision D5)*: Remove that artificial skip and promote `known_gap_negative_numbers_in_mapping_and_keyword_patterns` (`src/code_lint/rules/repeated_literal.rs:283`) to a `fail` test.
5. **Domain-level context fields instead of upward `AstNode` parent walks**:
   - *Prototype observation*: `ruff_python_ast` has no `.parent()` links, so `enclosing_non_exempt_function_name` walked `ModModule` by byte-range containment.
   - *Production design (Decision D7)*: Attach `BindingKind` (`Normal`, `Import`, `StructuralDefinition`, `comprehension_Target`, `ExceptHandler`) to `NameWrite` and `is_class_attribute` / `in_protocol_or_abc` to `AnnotatedAssignment` / `FunctionBlock` during the single top-down pass, and provide a shared $O(\text{depth})$ `python::ancestors_at_range(module, range)` helper for the remaining span-based queries (`enclosing_non_exempt_function_name`, `find_enclosing_with_statement`).

---

## 5. Concrete API Reference Cheat-Sheet

### 5.1 Python (`ruff_python_parser = "=0.0.16"`, `ruff_python_ast = "=0.0.16"`, `ruff_text_size = "=0.0.2"`)

- **Parsing & Errors**:
  - `let parsed: Parsed<ModModule> = ruff_python_parser::parse_module(source);`
  - `parsed.has_invalid_syntax() -> bool` (or `!parsed.errors().is_empty()`)
  - `parsed.syntax() -> &ModModule` (`parsed.suite() -> &[Stmt]`)
- **Comments & Tokens**:
  - `parsed.tokens() -> &Tokens` (`iter()` yields `&Token`)
  - `token.kind() == TokenKind::Comment`
  - `token.range() -> TextRange` (`usize::from(range.start())..usize::from(range.end())`)
- **Spans (`ruff_text_size`)**:
  - `use ruff_text_size::{Ranged, TextRange, TextSize};`
  - Every AST node (`Stmt`, `Expr`, `Pattern`, `Parameter`, `ParameterWithDefault`, `Decorator`, `Alias`, `ExceptHandler`, `WithItem`, `Comprehension`, `MatchCase`, `Arguments`, `Keyword`, `AnyNodeRef`) implements `Ranged::range(&self) -> TextRange`.
- **Untyped / Sum-Type Node Reference (`AnyNodeRef<'a>`)**:
  - `ruff_python_ast::AnyNodeRef<'a>` wraps a reference to any AST node variant (`From<&'a Stmt>`, `From<&'a Expr>`, `From<&'a Pattern>`, `From<&'a Parameter>`, `From<&'a Decorator>`, `From<&'a ExceptHandler>`, `From<&'a WithItem>`, etc.).
  - `AnyNodeRef::visit_preorder(&self, visitor)` or manual child dispatch enables generic $O(\text{depth})$ range-containment ancestor walks from `&ModModule`.
- **Visitors**:
  - `ruff_python_ast::visitor::{Visitor, walk_stmt, walk_expr, walk_pattern, walk_body, ...}`
  - `ruff_python_ast::statement_visitor::{StatementVisitor, walk_stmt, walk_body}`
- **Literals**:
  - `Expr::StringLiteral(s)`:
    - `s.value.to_str() -> &str` (decoded UTF-8 content across implicit concatenations)
    - `s.value.is_implicit_concatenated() -> bool`
    - `s.value.iter() -> impl Iterator<Item = &StringLiteral>` (each part has `.flags: StringLiteralFlags`, `.range()`)
  - `Expr::BytesLiteral(b)`:
    - `b.value` (`Box<[u8]>` or `Cow<[u8]>` via `Vec<u8>::from(&b.value)`), `b.value.is_implicit_concatenated()`
  - `Expr::FString(f)`:
    - `f.value.iter()` yields `FStringPart::Literal(StringLiteral)` and `FStringPart::FString(FString)`.
    - An `FString` has `.elements: InterpolatedStringElements`; if `.elements.iter().any(|e| matches!(e, InterpolatedStringElement::Interpolation(_)))`, it is an interpolated f-string (skip as non-constant literal). Otherwise, concatenate literal element strings!
  - `Expr::NumberLiteral(n)`:
    - `n.value` is `Number::Int(Int)` | `Number::Float(f64)` | `Number::Complex { real, imag }`.
  - `Expr::BooleanLiteral(b)` (`b.value: bool`), `Expr::NoneLiteral(_)`, `Expr::EllipsisLiteral(_)`.

### 5.2 Rust (`ra_ap_syntax = "=0.0.357"`)

- **Parsing & Errors**:
  - `let parse: Parse<SourceFile> = ra_ap_syntax::SourceFile::parse(source, ra_ap_syntax::Edition::CURRENT);`
  - `!parse.errors().is_empty() -> bool`
  - `let file: SourceFile = parse.tree();`
  - `file.syntax() -> &SyntaxNode`
- **Comments & Tokens**:
  - Iterate `file.syntax().descendants_with_tokens()` filtering `NodeOrToken::Token(token)` where `token.kind() == SyntaxKind::COMMENT`.
  - Wrap with `ast::Comment::cast(token)`:
    - `comment.kind().doc.is_some()` -> `true` for doc comments (`///`, `//!`, `/**`, `/*!`), `false` for regular comments (`//`, `/* */`).
- **Attributes & Inline Test Ranges**:
  - Any `ast::Item` (`ast::Module`, `ast::Fn`, `ast::Struct`, `ast::Enum`, `ast::Impl`, etc.) implements `ast::HasAttrs`.
  - `item.attrs()` yields `ast::Attr` children.
  - `attr.meta()` / `attr.path()` / `attr.token_tree()` inspect the attribute (`#[test]`, `#[tokio::test]`, `#[rstest]`, `#[cfg(test)]`).
  - Because `ast::Attr` nodes are children of the `ast::Item` in `ra_ap_syntax`, `item.syntax().text_range()` **already includes** leading outer attributes!
- **Literals (`ast::Literal`)**:
  - `ast::Literal::cast(node)` or `ast::Literal::from_token(token)`:
    - `lit.kind()` returns `ast::LiteralKind`:
      - `LiteralKind::String(s)` -> `s.value(): Result<Cow<'_, str>, _>` (decoded string content with escapes resolved and raw `#` delimiters stripped!)
      - `LiteralKind::ByteString(b)` -> `b.value(): Result<Cow<'_, [u8]>, _>`
      - `LiteralKind::CString(c)` -> `c.value(): Result<Cow<'_, [u8]>, _>`
      - `LiteralKind::Char(c)` -> `c.value(): Result<char, _>`
      - `LiteralKind::Byte(b)` -> `b.value(): Result<u8, _>`
      - `LiteralKind::IntNumber(i)` -> `i.value(): Result<u128, _>`, `i.suffix() -> Option<&str>`, `i.float_value()`
      - `LiteralKind::FloatNumber(f)` -> `f.value(): Result<f64, _>`, `f.suffix() -> Option<&str>`
      - `LiteralKind::Bool(bool)`
  - **Macro `TokenTree` handling**:
    - Inside `ast::MacroCall`, arguments are stored in an `ast::TokenTree` whose descendants are raw `SyntaxToken`s (`SyntaxKind::INT_NUMBER`, `SyntaxKind::FLOAT_NUMBER`, `SyntaxKind::STRING`, `SyntaxKind::BYTE_STRING`, `SyntaxKind::CHAR`, `SyntaxKind::BYTE`, `SyntaxKind::IDENT` (`true`/`false`), and `SyntaxKind::MINUS`).
    - Walking `token_tree.syntax().children_with_tokens()` recursively and converting literal tokens via `ast::Literal::from_token(token)` (with preceding `-` sign tracking for numeric tokens, excluding `->` and negative literals after binary operands) handles both normal AST literals and macro literals uniformly.
