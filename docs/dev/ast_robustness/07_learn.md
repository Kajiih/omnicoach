# Phase 7 — Learnings & Takeaways (P3 Dedicated AST Migration)

> **Status**: DONE

---

## 1. Key Technical Learnings

1. **Span-Backed `Copy` `AstNode<'a>` Keeps Parser Types Strictly Encapsulated**:
   - Representing `AstNode<'a>` as `{ file: &'a ParsedFile, span: SourceSpan }` (`#[derive(Clone, Copy)]`) allowed all 32 code lint rules and 5 semantic modules to remain 100% parser-agnostic while making every `AstNode` location/text query infallible ($O(1)$ slice into `file.source` and $O(\log L)$ binary search into `file.line_index`).
   - Caching `Vec<SourceSpan>` (and `Vec<CachedCallCandidate>`) in `OnceLock` fields on `ParsedFile` lets `collect_comment_nodes`, `collect_bindings`, and `collect_call_candidates` avoid repeated AST traversals across rules while still returning lightweight `AstNode<'_>` handles.

2. **`ruff_python_ast` API Subtlety — `StringLiteralValue::len()` vs `.as_slice()`**:
   - `ruff_python_ast::StringLiteralValue::len()` returns the **byte length of the concatenated string value**, whereas `StringLiteralValue::as_slice()` returns `&[StringLiteral]` (the individual literal parts in an implicit string concatenation). Checking for single-part string literals must match `let [part] = str_lit.value.as_slice()` (or `BytesLiteralValue::as_slice()`), never `.len() == 1`.

3. **Python Comment Tokenization Without `ruff_python_trivia`**:
   - Because `ruff_python_ast` is a pure abstract syntax tree that omits comments from its node hierarchy, scanning `&file.source` with a lightweight state machine that tracks string delimiters (`'`, `"`, `'''`, `"""`), raw-string prefixes, `\` escapes, and Python 3.12 PEP 701 nested f-string `{ ... }` interpolation depth extracts `#` comment spans in a single $O(N)$ pass without pulling in `ruff_python_trivia`.

4. **Lossless Rowan CST (`ra_ap_syntax`) for Rust Macros and Attributes**:
   - `ra_ap_syntax` combines typed AST wrappers (`ast::Fn`, `ast::Attr`, `ast::MacroCall`, `ast::PathType`, `ast::Literal`, `ast::String::value()`) with lossless token access (`SyntaxNode`, `SyntaxToken`, `SyntaxElement`).
   - Unlike `tree-sitter-rust`, `ra_ap_syntax` attaches outer `#[...]` attributes and `///` doc comments directly inside the owning item (`HasAttrs::attrs()`), parses `#[cfg(test)]` via `ast::Meta::CfgMeta` (`ast::CfgPredicate::CfgAtom`), and parses Rust 2024 `&& let` let-chains natively without `ERROR` nodes.

5. **Eliminating `ast-grep` Metavariable Compilation via Enriched `AstCallCandidate`**:
   - Adding `receiver_call_callee: Option<String>` to `AstCallCandidate` allowed all chained method call patterns (`*().create_task`, `asyncio.get_running_loop().create_task`, `*.assert_called_once`) to be evaluated in the same single pass over `collect_call_candidates(file)` as direct and qualified calls, deleting `ast::find_pattern_calls` and all runtime pattern compilation.
