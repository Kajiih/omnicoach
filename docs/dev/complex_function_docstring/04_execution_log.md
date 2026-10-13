# Complex function docstring: 04 Execution log

> **Status: VALIDATED** (lead, 2026-10-12; implemented). Scope: `undocumented-nontrivial-function`, `unexplained-parameter` and `undocumented-tuple-return` (removed 2026-10-13, superseded by `positional-tuple-return` and `unexplained-return`; see [../return_contracts/03_design.md](../return_contracts/03_design.md)).

## What was built

- **Rule** `undocumented-nontrivial-function` (`src/code_lint/rules/undocumented_nontrivial_function.rs`): Python only, topic `docstrings`, `Heuristic` / `Opinionated` / `Maintainability`, default enforcement `ban` (`RuleOptions::code_rule`), target `SourceOnly` (D8). It flags a function without a docstring when `statements >= min-statements` (default 12) or `cognitive complexity >= min-cognitive-complexity` (default 6); 0 disables a trigger (03b D-gate1, D-gate6). The span is the function name (D15). The message names the trigger: statements are checked first, and cognitive complexity is computed only when the statement trigger does not fire.
- **Rule** `unexplained-parameter` (`src/code_lint/rules/unexplained_parameter.rs`): Python only, topic `docstrings`, `Heuristic` / `Opinionated` / `Maintainability`, default enforcement `ban` (`RuleOptions::code_rule` with `min-statements = 12` and `min-cognitive-complexity = 6`), target `SourceOnly`. Flags a documented nontrivial function whose docstring has no `Args:` section (`Args:`, `Arguments:`, `Parameters:`, `Keyword Args:`) and has at least one parameter with an `Optional` / `... | None` annotation or `= None` default, a `bool` annotation or `= True` / `= False` default, a `Callable` annotation, or `*args` / `**kwargs`. One finding per function with `{parameters}` listing every such parameter in declaration order. Incomplete `Args:` sections are delegated to Ruff `D417`. For `__init__` and `__new__`, an `Args:` section in the class docstring satisfies the rule, and for `__init__`, parameters stored unchanged on `self` (`self.<name> = <name>`) and documented under `Attributes:` in the class docstring are exempt.
- **Rule** `undocumented-tuple-return` (`src/code_lint/rules/undocumented_tuple_return.rs`): Python only, topic `docstrings`, `Heuristic` / `Opinionated` / `Maintainability`, default enforcement `ban` (`RuleOptions::code_rule(())`), target `SourceOnly`. Flags a documented function at any size returning or yielding a positional tuple of `>= 2` elements without `...` (annotated `tuple[A, B, ...]` / `Tuple[A, B, ...]`, including inside unions, `Awaitable`/`Coroutine` and generator yield types, or unannotated `return` / `yield` of a tuple display of `>= 2` elements in the function's own scope) when the docstring has neither a `Returns:` / `Yields:` section nor a `Return(s)` / `Yield(s)` summary.
- **Helpers**:
  - `src/code_lint/ast/python/complexity.rs`: `PythonFunctionSignature::statement_count()` (03b D-gate2) and `PythonFunctionSignature::cognitive_complexity()` (03b D-gate3 with the D-gate7 notes).
  - `src/code_lint/ast/python/annotations.rs`: `is_non_obvious_parameter_type()` and `is_positional_tuple_return_type()`.
  - `src/code_lint/ast/python/functions.rs`: `PythonFunctionSignature::is_nested()`, `PythonFunctionSignature::is_exempt_from_docstring_rules()`, `PythonParameterInfo::is_non_obvious()`, `PythonParameterInfo::display_name()`, and `PythonFunctionSignature::returns_positional_tuple()`.
  - `src/code_lint/ast/python/classes.rs`: `PythonClassInfo::unchanged_constructor_parameters()`.

## Deviations, with reasons

| # | Deviation | Reason |
| :-- | :--- | :--- |
| X1 | Name `undocumented-nontrivial-function` instead of `undocumented-long-function`. | Lead-approved: a function flagged by complexity alone need not be long; "nontrivial" mirrors Google §3.8.3 and covers both triggers. |
| X2 | Two walks (`statement_count`, `cognitive_complexity`) instead of D-gate7's single walk. | The two measures are exposed as separate helpers; the statement count is a small recursive function, and the rule skips the complexity walk when the statement trigger fires. Both stay linear. |
| X3 | `else` and `except` clauses, and `match` cases, do not count as statements; their bodies do. | They are not statement nodes; D-gate2 counts only `elif` as an extra clause. |
| X4 | Bare `<name>()` counts as recursion only in a function without a receiver; in a method only `self.<name>` / `cls.<name>` does. | In a method, a bare call to the method's name resolves to a module-level function, not the method. |
| X5 | Nested `def` decorators, defaults and annotations are scored at the enclosing nesting; nested class bodies get nesting + 1 (03b table), not SonarSource's reset to 0. | 03b D-gate3. |
| X6 | `is_nested()` stores the enclosing `def` instead of a fourth `bool`. | `clippy::struct_excessive_bools` (pedantic, denied); the repository uses no lint allowances. |
| X7 | E1 (blank/comment lines) has no case or mutation. | It belonged to the dropped line gate; the docstring exclusion of `statement_count()` cannot change the rule's result (a function with a docstring is never flagged), so it is pinned by a unit test instead. |
| X8 | The test-file target and the `0 disables` option behaviour have no `rule_test!` case. | The harness runs with default options on a non-test path. |
| X9 | `undocumented-parameter` narrowed to `unexplained-parameter` (P2 of `03c_section_requirements.md`, without prose-mention exemption or `min-parameters` trigger). | User decision: Ruff `D417` covers incomplete `Args:` sections; only non-obvious parameters (`Optional`/`= None`, `bool`/`= True`/`= False`, `Callable`, `*args`/`**kwargs`) require an `Args:` section on a nontrivial function. |
| X10 | `undocumented-return-value` narrowed to `undocumented-tuple-return` (P5 of `03c_section_requirements.md`). | User decision: Ruff `DOC201`/`DOC402` cover structured docstrings missing `Returns:`/`Yields:`; only positional tuple returns (`>= 2` elements, no `...`) require docstring explanation even on a one-line docstring. |

## Tests & Exemption mutations

- `complexity.rs` unit tests: 33 (`statement_count` and `cognitive_complexity` cases).
- `undocumented-nontrivial-function`: 7 `pass`, 6 `fail`, 5/5 exemption mutations killed.
- `unexplained-parameter`: 9 `pass`, 7 `fail`, 6/6 exemption mutations killed.
- `undocumented-tuple-return`: 8 `pass`, 5 `fail`, 6/6 exemption mutations killed.
- Full suite (`cargo fmt --check`, `cargo clippy --all-targets -- -D warnings -D clippy::pedantic`, `cargo test`, `RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --document-private-items`) passes cleanly.
