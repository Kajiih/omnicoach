# Phase 4 — Execution Log

Status: **complete, pending user validation**. Working-copy change: 46 files, +3497 / −2524
(including docs and ~480 lines of relocated tests).

## Steps

| # | Step | Result |
| :--- | :--- | :--- |
| 1 | L1/L2 foundation | `diagnostic.rs` is a pure leaf (owns `RuleName`); `core.rs` owns `Tag`, depends only on `diagnostic`; `Selector::Name(String)`. |
| 2 | L2 grammar encapsulation | `code_lint/ast/{mod,python,rust,statements}.rs`. `ParsedFile` + opaque `AstNode`; raw `ast_grep_core` types are `pub(in crate::code_lint::ast)`. `dispatch_lang!` replaces the per-site `match lang`. |
| 3 | L3 engines, L4 contracts, L6 runner | `bindings`/`calls`/`comments` take `&ParsedFile`; `code_lint.rs` split into `rule.rs` (L4) + `runner.rs` (L6); `command_lint/rule.rs` (L4). |
| 4 | L5 migration | 19 code rules + `jj.rs` + `suppression.rs` take `&ParsedFile`; zero `ast_grep_core`, zero `crate::rules`, zero cross-rule imports. 13 `lint_file` tests moved to `runner.rs`. |
| 5 | Enforcement + docs | `tests/architecture.rs` (`rust_arkitect` 0.3.7); `rule_design_guide.md` §7; ROADMAP pruned. |

## Verification

- `cargo test`: 508 lib + 14 CLI + 4 architecture tests pass; **no snapshot changes**
  (CLI snapshots and self-dogfooding unchanged ⇒ zero diagnostic behavior change).
- `cargo clippy --all-targets`: clean. `cargo fmt --check`: clean.
- Ported unit tests audited by name against the pre-refactor revision: none lost.
- Architecture suite is non-vacuous: a negative-control test, plus a mutation check
  (`scratch/mutate_architecture.py`) injecting one violation per rule into a real rule
  file — all three caught.

## Deviations from `03_plan.md`

| Plan | Actual | Why |
| :--- | :--- | :--- |
| `SyntaxNode` with `location` / `line_column` | `AstNode` with `to_source_location` / `start_coordinate` / `span` | Matches existing vocabulary (`SourceLocation`, `LineColumn`). |
| `command_lint/runner.rs` | Not created | No command-lint orchestration exists in the library; it lives in the `omni-command-lint` binary (L7). |
| `code_lint/mod.rs` | `code_lint.rs` kept as module root | Matches the crate's existing non-`mod.rs` style. |
| 11 `lint_file` tests moved | 13 moved | Plan miscounted. |
| `collect_comment_nodes` returns an iterator | Returns `Vec` | `root().dfs()` borrows a temporary root; cannot escape as an iterator. |
| `find_pattern_calls` arguments unfiltered | Filtered to named, non-extra nodes | Validated adjacent item #1; no snapshot changed. |
| Use `rust_arkitect`'s `Arkitect` engine | Drive its `Rule`s over `src/` directly | The engine recursively walks the whole crate dir, including 12.5k `.rs` files under `scratch/`. |
| — | `ast_dumper` binary keeps raw `ast_grep_core` | Debug tool printing raw CST; exempted explicitly in the architecture test. |

## Findings During Execution

- The `statements.rs` test module had been rewritten (not ported) during the move, with
  4 wrong expectations; the original, verified tests were restored verbatim.
- `rust_arkitect` resolves `crate::` to the crate name in `use` trees but keeps it verbatim in
  inline paths; forbidden modules are registered under both spellings (covered by the
  negative-control test).
- Re-exports removed after review. Module-root facades (`code_lint::{CodeRule, ...}`,
  `command_lint::{CommandRule, ...}`) and compatibility shims (`core::RuleName`,
  `rules::Tag`) gave items two paths and hid the defining layer from `rust_arkitect`
  (attributed to the L7 root). Every item is now imported from its defining module;
  `code_lint::{rule,runner}` and `command_lint::rule` are `pub` for the binaries.
  Macros followed: `violation_template!` and `rule_test!` dropped `#[macro_export]` (which
  put them at the crate root, giving `violation_template!` two paths) for
  `pub(crate) use` in their defining module; `MacroSupportLang` alias removed;
  `dispatch_lang!` narrowed to a private `use`. `test_items_have_a_single_path` bans
  visible `use` except a macro declaring its own path, and bans `#[macro_export]`.
- Out of scope: intra-layer cycles (e.g. between two L3 engines).
