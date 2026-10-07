# Semantic index: 01 Understand

> [!NOTE]
> **Status: IN PROGRESS (2026-10-07).** Follow-up to the dedicated AST migration
> ([ast_robustness](../ast_robustness/07_learn.md)). Inputs: a call-graph audit of `src/code_lint/`
> and a review of how Ruff (`ruff_python_semantic`), Oxlint (`oxc_semantic`), Biome
> (`biome_analyze`) and Clippy / rust-analyzer structure their AST, semantic model and rule queries.

## 1. Problem

`AstNode<'a>` is `{ file, span }`. That keeps rules parser-agnostic, but `ruff_python_ast` has no
parent links, so any helper taking an `AstNode` must walk the module from the root to find the
node again. Helpers that extract a fact (a function, a binding, a call) drop context they had
during the walk, and callers re-derive it by span.

| ID | Finding | Evidence |
| :--- | :--- | :--- |
| P1 | 21 Python helpers re-walk the module from the root per call; 13 Rust helpers re-run `covering_element`. | `find_expr_at_span`, `find_function_def_at_span`, `find_assert_at_span`, `is_import_binding`, `is_structural_definition`, `is_trait_impl_member` (2 walks), `extract_decorators`, `is_in_protocol_or_abc_class`, `is_stub_function_body`, `is_with_context_manager` (2 walks), `is_inside_except_clause`, … |
| P2 | Hot loops: every binding (4 naming rules), every function (7 signature rules), every assert, every `suppress` call. | `semantic/bindings.rs`, `PythonFunctionSignature::is_exempt_from_*` |
| P3 | Multi-consumer collectors are recomputed per rule. | `extract_function_signatures` (7 rules), `CommentIndex::from_file` (per `RequireExplanation` rule), `extract_classes` (3), `collect_class_attributes` (2), `rust::collect_functions` (2) |
| P4 | Two ~300-line parameter-use visitors walk each body once per parameter and disagree. | `MutationOrEscapeFinder` vs `CapabilityVisitor` in `ast/python.rs` |
| P5 | Names are matched syntactically: aliases are missed, unrelated imports and local definitions match. | `known_gap_typing_module_alias_not_resolved`; `abc_set_imported: bool` threaded through 9 rules |
| P6 | 5 multi-language rules branch on `file.lang()` and duplicate logic; rule files hold AST logic. | `nullable_collection_return.rs`, `identical_positional_types.rs`, `packed_assertion.rs`, `call_before_definition.rs`, `environment_variable_in_function.rs` |
| P7 | `ast/python.rs` is 3,791 lines across 7 domains. | |

### Suspected divergence bugs between the parameter visitors (P4): not confirmed

The audit read three bugs from the code. Regression cases written before the change passed on
the old code, so none was real; the cases stay as behavior pins (they were the only coverage).

| ID | Suspected | Why it was not a bug |
| :--- | :--- | :--- |
| B1 | `return flag and items` suggests `Collection`. | `BoolOp` operands count as truthiness only inside a boolean context (`if`, `while`, `assert`, `bool()`); a `return` is not one. |
| B2 | `sorted(items, reverse=True)`, `dict(items)`, `map`/`filter` count as mutation. | The read-only builtin list already has `dict`/`map`/`filter`, and keywords were never rejected. |
| B3 | `items == other` counts as mutation. | Every `Compare` operand, the left one included, is read-only. |

Lesson: an audit by reading is a hypothesis; reproduce before recording a bug.

## 2. How SOTA tools avoid this

- **Ruff**: a traversal-time ancestor stack plus a `Nodes` arena (`NodeId` → parent). Bindings and
  references carry `NodeId`s; `resolve_qualified_name` maps a name to its import path in one file.
- **Oxlint**: `AstNodes` with a parallel `parent_ids` vector; `Reference` flags (read/write).
- **Biome**: rules subscribe to a `Queryable`; one visitor per phase builds the matches and shared
  services (semantic model) once.
- **rust-analyzer / Rowan**: red nodes have `parent()`; Omni loses that by converting to `AstNode`.

Takeaway for Omni: keep `AstNode` and the parser boundary, but compute context **once, during the
walk that extracts the fact**, and memoize shared projections on `ParsedFile`.

## 3. Plan (one atomic commit per slice)

1. **Parameter use summary.** One visitor classifies the uses of all parameters of a function in a
   single pass; both rules read their answer from it. **Done:** `summarize_parameter_usages`
   returns a `ParameterUsage` per parameter; a `UseRole` passed down the walk replaces the
   byte-offset sets. Unified edge cases: `assert x` now reads like `if x` (`Collection`), `not`
   always opens a truthiness context, and nested `def` decorators/defaults are visited at the
   enclosing loop depth.
2. **No root re-walks; memoized projections.** Record context flags at extraction time (bindings,
   function signatures, call candidates, asserts, module assignments); keep chained type queries on
   parser types inside `ast/`; memoize the multi-consumer collectors and `CommentIndex`; delete the
   span finders that become unused.
3. **Per-file import map.** Resolve Python `import` / `from … import` and Rust `use` aliases, with
   local definitions shadowing imports; replace `abc_set_imported`; promote the alias `known_gap`.
   *Done:* `ast::resolve_name` (cached `ImportMap` on `ParsedFile`) returns
   `Imported(path)` / `Local` / `Unbound` from module-level Python imports and `def`/`class`
   (assignments excluded) and root-level Rust `use` trees and items. Wired into banned-call
   literal matching (D1) and Python collection/`Annotated`/`Final` classification;
   `abc_set_imported` is gone. Protocol/ABC bases, `@override` and `@dataclass` still match by
   terminal name.
4. **Cross-language symmetry.** Move rule-held AST logic into `ast/` and remove duplicated
   per-language algorithms in the 5 branching rules.
5. **Split `ast/python.rs`.** Move parameter analysis and literal/positional-read collection (with
   their tests) into submodules.

Gate per slice: `cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test &&
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --document-private-items`. Existing `rule_test!`
cases are the oracle: only B1–B3 and the alias gap may change expected results.

## 4. Decisions

- **D1.** Unimported bare names (`cast(...)`, `Sequence[int]` in a snippet without imports) keep
  matching as today. A name bound to an import of a *different* module, or to a local definition,
  no longer matches a banned path. Rationale: keeps short snippets and partial files working while
  removing the clear false positives.
- **D2.** No `NodeId` arena yet: attaching context at extraction time removes the measured
  re-walks with less machinery. Revisit if a rule needs arbitrary ancestor queries.
