# Phase 2 — Resources, References & Architecture Analysis

This document records the research on external state of the art (SOTA) projects, internal codebase
patterns, concrete prototypes for the Q1 rule-vs-grammar abstraction, and candidate enforcement
mechanisms.

---

## 1. External State-of-the-Art (SOTA) Reference Analysis

We surveyed how leading production linters and compiler toolchains structure the boundary between
rule logic and compiler/grammar internals, and how they enforce architectural constraints.

### 1.1 Ruff (Astral)
* **Separation of Concerns**: Ruff enforces a strict multi-crate hierarchy within its Cargo workspace:
  * `ruff_python_parser`: Lexer, parser, and raw CST.
  * `ruff_python_ast`: Strongly-typed AST structs (`ast::ExprCall`, `ast::StmtFunctionDef`).
  * `ruff_python_semantic`: Semantic model (scopes, symbol bindings, definition resolution, import map).
  * `ruff_linter/src/rules/`: Declarative lint rules organized by plugin (`flake8_bugbear`, `pyflakes`).
* **Rule Design Principle**: Rules **never** inspect raw token kinds or perform manual AST pointer
  arithmetic. Rules receive a `&Checker` or typed AST node, and query high-level semantic helpers
  (e.g., `checker.semantic().match_typing_expr(func, "cast")`, `scope.kind.is_nested()`).
* **Enforcement**: Crate boundaries in Cargo enforce layer direction at compile time (a module in
  `ruff_python_ast` cannot physically import from `ruff_linter`). Internal conventions (like forbidden
  APIs) are enforced via `flake8-tidy-imports` (`banned-api`).

### 1.2 Clippy (Rust compiler team)
* **Separation of Concerns**:
  * `clippy_lints`: Houses all rule implementations.
  * `clippy_utils`: The canonical vocabulary and AST/HIR traversal library (`match_def_path`,
    `is_expr_path_def_path`, `get_parent_expr`, `is_in_test_function`).
* **Dogfooding & Internal Lints**: Clippy defines custom *internal lints* (`clippy_dev` and dogfooding)
  that trigger during `cargo test` if a rule file attempts to manipulate raw compiler HIR structures
  directly instead of using the standardized `clippy_utils` helper functions.

### 1.3 Biome (former Rome)
* **Separation of Concerns**:
  * `biome_js_syntax`: Strongly-typed CST generated from grammar specifications. No stringly-typed
    grammar tokens exist in application code.
  * `biome_js_analyze`: Rule implementations.
* **Queryable Model**: Rules declare what node type they subscribe to via typed markers
  (`Rule<Query = Ast<JsCallExpression>>`). Traversal is completely hidden from the rule; the rule only
  receives the already-matched, strongly-typed node wrapper.

### 1.4 rustc `tidy`
* **Custom Invariant Verification**: `rustc` uses an internal tool called `tidy` (invoked during `x.py test`).
  It performs fast, zero-dependency source code scanning to enforce intra-crate invariants: license headers,
  banned dependencies, forbidden imports, and feature gate consistency.

### 1.5 Enforcement Tools & Crates in the Rust Ecosystem
We evaluated five mechanisms across compiler lints, external crates, and CLI tools (detailed in §6):
* **`clippy::disallowed_methods` (`clippy.toml`)**: Built into `rustc`/`clippy` (zero dependencies). Uses
  full compiler type resolution (`LateLintPass`) to ban `ast_grep_core::Node` traversal/grammar methods
  (`kind`, `field`, `children`, `dfs`, `find`, `find_all`, etc.) everywhere except modules with an explicit
  `#![allow(clippy::disallowed_methods)]` (`ast_python.rs` and `ast_rust.rs`).
* **`rust_arkitect` (`0.3.7`, MIT)**: Lightweight `#[test]` fitness-function crate using `syn::visit::Visit`
  (already in `Cargo.lock` transitively). Resolves both `use` statements and inline `ExprPath`/`TypePath`
  references (`crate::...`, `super::...`), with built-in cycle detection and `.it_may_depend_on(&[...])` rules.
* **`arch_test_core` (`0.1.5`, AGPL-3.0)**: Relies on outdated `ra_ap_syntax 0.0.59` (2021) and AGPL-3.0 license.
* **`cargo-modules` (CLI)**: Uses `ra_ap_hir` for `cargo modules dependencies --acyclic` and DOT graph export,
  but requires an external binary outside `cargo test` and only checks acyclicity, not custom layer matrices.
* **`dylint`**: Custom `rustc_private` lints; requires nightly toolchain pinning and separate dylib builds.

---

## 2. Internal Codebase Patterns & Precedents

Omni already has successful precedents for declarative architecture checking:

1. **`src/rules.rs:265` (`test_rule_sources_do_not_sort_diagnostics`)**:
   Iterates through `src/code_lint/rules/*.rs` and verifies that no rule source contains `.sort`.
2. **`src/rules.rs:315` (`test_rule_files_use_rule_test`)**:
   Verifies that no rule file creates a bespoke `mod tests` or `#[test]` function.
3. **`src/code_lint/bindings.rs` & `src/code_lint/calls.rs`**:
   Demonstrate effective L3 shared engines: rules delegate high-level call and binding checks (`check_banned_calls`,
   `filter_bindings`) without knowing how calls or identifiers are extracted from the syntax tree.

---

## 3. Concrete Prototype for Q1: Eliminating Grammar Leaks from Rules

In Phase 1, the user set the target: **no grammar text in rule files** if named helpers maintain high
readability and precision. We prototyped this on two rules exhibiting the worst grammar leaks.

### Prototype A: `flat_scope_enforced.rs`

* **Current Implementation (with grammar leaks)**:
  ```rust
  // Rule file directly parses ast-grep pattern strings and inspects Tree-sitter field:
  grep.root()
      .find_all("def $NAME($$$ARGS): $$$BODY")
      .filter(|func| crate::code_lint::ast_python::is_nested_function(func))
      .map(|func| {
          let func_name = func
              .field("name")
              .map(|name_node| name_node.text())
              .unwrap_or_default();
          self.diagnostic_at_node(path, &func, &[("func_name", &func_name)])
      })
      .collect()
  ```
  *Flaws*:
  - Re-parses the pattern string on every file check at runtime.
  - Hardcodes the Tree-sitter field literal `"name"`.
  - Mixes grammar traversal with lint policy.

* **Proposed Refactoring**:
  In `src/code_lint/ast_python.rs` (L2):
  ```rust
  /// Discovers all nested function definitions inside `root`, yielding the function AST node
  /// and its declared name.
  pub fn find_nested_function_definitions<'a>(
      root: &AstNode<'a>,
  ) -> impl Iterator<Item = (AstNode<'a>, std::borrow::Cow<'a, str>)> {
      root.dfs()
          .filter(|node| node.kind() == "function_definition" && is_nested_function(node))
          .map(|node| {
              let name = node
                  .field("name")
                  .map(|name_node| name_node.text())
                  .unwrap_or_default();
              (node, name)
          })
  }
  ```
  In `src/code_lint/rules/flat_scope_enforced.rs` (L5):
  ```rust
  impl CodeRule for FlatScopeEnforced {
      fn target(&self) -> RuleTarget {
          RuleTarget::SourceOnly
      }

      fn check_file(
          &self,
          path: &Path,
          grep: &AstGrep<SourceDoc>,
          _config: &Config,
      ) -> Vec<Diagnostic> {
          ast_python::find_nested_function_definitions(&grep.root())
              .map(|(func, func_name)| {
                  self.diagnostic_at_node(path, &func, &[("func_name", &func_name)])
              })
              .collect()
      }
  }
  ```
  *Verdict on Q1*: **Immense win.** The rule becomes 3 lines of pure domain policy. Readability and
  clarity increase dramatically. Runtime pattern parsing is eliminated.

---

### Prototype B: `no_assertion_packing.rs`

* **Current Implementation**:
  Rust checks are already encapsulated in `ast_rust.rs`:
  ```rust
  if (macro_name == "assert" || macro_name == "debug_assert")
      && ast_rust::has_top_level_logical_and(macro_node)
  { ... }
  ```
  Python checks leak raw Tree-sitter kind string literals and children iterations:
  ```rust
  // Leaks kind literals "boolean_operator", "and", "comparison_operator":
  let has_and = assert_node
      .children()
      .any(|c| c.kind() == "boolean_operator" && c.children().any(|op| op.kind() == "and"));
  let comparison = assert_node
      .children()
      .find(|c| c.kind() == "comparison_operator")?;
  ```

* **Proposed Refactoring**:
  Add `ast_python::has_top_level_logical_and` and `ast_python::has_boolean_literal_comparison` to `ast_python.rs`.
  Then the Python check in `no_assertion_packing.rs` becomes perfectly symmetric with Rust:
  ```rust
  if ast_python::has_top_level_logical_and(assert_node) {
      return Some(NoAssertionPacking.diagnostic_at_node(
          path,
          assert_node,
          &[("construct", "Compound boolean condition (`and`)")],
      ));
  }
  if ast_python::has_boolean_literal_comparison(assert_node) {
      return Some(NoAssertionPacking.diagnostic_at_node(
          path,
          assert_node,
          &[("construct", "Boolean tuple/collection equality")],
      ));
  }
  ```
  *Verdict*: Zero grammar kind literals remain in the rule; perfect symmetry between supported languages.

---

## 4. Per-Language Dispatch & Punctuation Leak Resolution

### 4.1 Punctuation Leak in `calls.rs` (Roadmap Item)
In `src/code_lint/calls.rs:44`:
```rust
fn call_argument_nodes<'a>(call_node: &AstNode<'a>) -> Vec<AstNode<'a>> {
    call_node.field("arguments").map_or_else(Vec::new, |args| {
        args.children()
            .filter(|child| !matches!(child.kind().as_ref(), "(" | ")" | ","))
            .collect()
    })
}
```
This contains two grammar leaks in an L3 engine: `.field("arguments")` and string literals `"("`, `")"`, `","`.
*Resolution*:
Delegate argument extraction to `ast_python::call_argument_nodes` and `ast_rust::call_argument_nodes`.
`ast_*.rs` owns the field name and filters non-semantic punctuation tokens (using `child.is_named()`).
`calls.rs` (L3) operates strictly on semantic `AstNode` lists.

### 4.2 Standardizing Language Dispatch (`dispatch_lang!`)
Currently, 10 sites across `bindings.rs`, `calls.rs`, `comments.rs`, `statements.rs`, and `code_lint.rs`
repeat the identical 6-line `match lang { Python => ..., Rust => ..., _ => fallback }`.
*Resolution*:
Introduce a macro `dispatch_lang!` in `code_lint`:
```rust
macro_rules! dispatch_lang {
    ($lang:expr, $func:ident ( $($arg:expr),* $(,)? ), $fallback:expr) => {
        match $lang {
            ast_grep_language::SupportLang::Python => $crate::code_lint::ast_python::$func($($arg),*),
            ast_grep_language::SupportLang::Rust => $crate::code_lint::ast_rust::$func($($arg),*),
            _ => $fallback,
        }
    };
}
```
*Benefits*:
1. Enforces compile-time naming and signature symmetry between `ast_python` and `ast_rust`.
2. Collapses 10 multi-line match blocks into single-line calls.
3. Adding a 3rd language requires updating the single macro definition, after which the compiler will
   immediately point to all functions that must be implemented in the new language.

---

## 5. Comprehensive Whole-Crate Layering Matrix (Resolving All Cycles)

In Phase 1, our dependency audit revealed several circular dependencies and layer inversions.
Here is the concrete analysis of why they exist and how to eliminate them:

### 5.1 Cycle & Leak Analysis & Resolution

1. **Encapsulating `ast_grep_core` inside L2 (`ParsedFile` & `SyntaxNode`)**:
   * *Problem*: `core.rs` (L1) defined `pub type AstNode<'a> = ast_grep_core::Node<'a, SourceDoc>;`,
     `diagnostic.rs` imported `AstNode`, and `CodeRule::check_file` (L4/L5) took `&AstGrep<SourceDoc>`.
     This leaked `ast_grep_core` and all 40 raw Tree-sitter traversal methods (`.kind()`, `.field()`,
     `.children()`, `.dfs()`, `.find_all()`) into L1, L3, L4, and all 19 L5 rule files.
   * *Solution*:
     - Remove `AstNode` and `SourceDoc` from `core.rs` and `diagnostic.rs`.
     - Define `ParsedFile` and `SyntaxNode<'a>` in `crate::code_lint::ast` (L2), wrapping `AstGrep<SourceDoc>`
       and `ast_grep_core::Node<'a, SourceDoc>` with fields restricted to `pub(in crate::code_lint::ast)`.
     - Outside `code_lint::ast`, `ParsedFile` exposes `new(source, lang)` and `lang()`, and `SyntaxNode<'a>`
       exposes only text and span/location methods (`text()`, `lang()`, `span()`, `line_column()`,
       `location(path)`, `start_line()`, `end_line()`).
     - `CodeRule::check_file` takes `&ParsedFile` instead of `&AstGrep<SourceDoc>`. Zero rule files import
       `ast_grep_core`, and calling `.kind()`, `.field()`, or `.dfs()` outside L2 is a **compile error**.

2. **Cycle C (`core.rs` ↔ `rules.rs`)**:
   * *Problem*: `core.rs` imports `crate::rules::Tag` and scans `crate::rules::CODE_RULES` and
     `crate::rules::COMMAND_RULES` inside `Selector::deserialize`.
   * *Solution*:
     - Move `Tag` from `rules.rs` to `core.rs` (L2), since `Tag` is part of the `Rule` trait contract.
     - Change `Selector::Name` to hold `String` (parsing tags into `Selector::Tag(Tag)` and rule names into
       `Selector::Name(String)` purely from text without scanning static rule registries).

3. **Cycle D (`core.rs` ↔ `diagnostic.rs`)**:
   * *Problem*: `core.rs` uses `Diagnostic`, `ViolationTemplate`, `SourceLocation` for `Rule`; `diagnostic.rs`
     imported `AstNode` and `RuleName` from `core.rs`.
   * *Solution*:
     - Move `RuleName` definition into `diagnostic.rs` (L1, re-exported in `core.rs`), and move `from_node`
       to `SyntaxNode` in `code_lint::ast` (L2).
     - `diagnostic.rs` (L1) and `diff.rs` (L1) now have **zero** `crate::*` dependencies!
     - `core.rs` (L2) depends strictly downward on `diagnostic.rs` (L1).

4. **Inversion B, E & F (`code_lint.rs` sandwich, `bindings.rs` → `CodeRule`, `suppression.rs` → `rules.rs`)**:
   * *Solution*:
     - Split `code_lint.rs` into `code_lint/rule.rs` (L4 `CodeRule`, `RuleTarget`) and `code_lint/runner.rs`
       (L6 `lint_file`, `run_code_lint`, `LintOptions`).
     - Move `check_banned_suffixes` onto `CodeRule` as a default method in `code_lint/rule.rs` (symmetric with
       `check_banned_calls`), removing `CodeRule` from `bindings.rs` (L3).
     - Pass `suppressible_rules: &HashSet<&str>` into `SuppressionTracker::audit` from `lint_file` (L6), and
       move the end-to-end `lint_file` tests from `suppression.rs` to `code_lint/runner.rs` (L6).

### 5.2 The Enforced Crate Layers (L0–L7)

Each layer may **only** depend on layers strictly below it (`<`).

| Layer | Level Name | Modules Included | Allowed Dependencies |
|:---|:---|:---|:---|
| **L0** | External Crates | `serde`, `toml`, `anyhow`, `ast_grep_language`, `ast_grep_core` (L2 only) | None |
| **L1** | Foundation Domain | `diagnostic.rs` (`RuleName`, `Diagnostic`, `ViolationTemplate`, `SourceLocation`), `diff.rs` | L0 (no `ast_grep_core`, no `crate::*`) |
| **L2** | Core Contracts & Grammars | `core.rs` (`Rule`, `Tag`, `Config`, `Selector`), `code_lint/ast/{mod,python,rust,statements}.rs` (`ParsedFile`, `SyntaxNode`), `command_lint/vcs.rs` | L1, L0 |
| **L3** | Semantic Engines | `code_lint/{bindings,calls,comments}.rs` | L2, L1, L0 |
| **L4** | Linter Contracts & Test Harness | `code_lint/rule.rs` (`CodeRule`), `command_lint/rule.rs` (`CommandRule`, `InterceptedCommand`), `test_utils.rs` | L3, L2, L1, L0 |
| **L5** | Rule Implementations | `code_lint/rules/*.rs`, `code_lint/suppression.rs`, `command_lint/rules/*.rs` | L4, L3, L2, L1, L0 (no cross-rule imports) |
| **L6** | Registries & Runners | `rules.rs` (`CODE_RULES`, `COMMAND_RULES`), `code_lint/runner.rs`, `command_lint/runner.rs` | L5, L4, L3, L2, L1, L0 |
| **L7** | Application Entrypoint | `lib.rs`, `bin/*.rs` | L6, L5, L4, L3, L2, L1, L0 |

---

## 6. Validated Enforcement Strategy: Type Privacy + `rust_arkitect`

By encapsulating `ast_grep_core::AstGrep` and `ast_grep_core::Node` inside `ParsedFile` and `SyntaxNode<'a>`
with `pub(in crate::code_lint::ast)` visibility:
1. **Rust's type system** enforces grammar/CST method isolation at compile time: `.kind()`, `.field()`,
   `.children()`, `.dfs()`, `.find_all()`, and `.parent()` do not exist on `ParsedFile` or `SyntaxNode`
   outside `crate::code_lint::ast`.
2. **`rust_arkitect` (`0.3.7` dev-dependency)** in `tests/architecture.rs` enforces:
   - Zero circular module dependencies across `src/`.
   - Strict monotonic layer dependencies (`L1` through `L7`).
   - Isolation of `ast_grep_core` so that L3–L7 cannot bypass `ParsedFile`/`SyntaxNode` by importing
     `ast_grep_core` directly.


