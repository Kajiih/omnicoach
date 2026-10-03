# Phase 2: Gather Resources and References — `DefineBeforeUseRule` (`call-before-definition`)

This document records **Phase 2 (Gather Resources and Reference)** for the legacy linter's `DefineBeforeUseRule` ([legacy_lints.py:L3049-L3133](../../../scratch/legacy_reference/legacy_lints.py#L3049-L3133)). It builds on [01_understand.md](01_understand.md) (decisions D1–D8 and defects F1–F7).

> Status: **DRAFT — Phase 2 Research Complete** (2026-10-03).

Confidence markers: ✅ verified against official docs/source this session · ⚠️ synthesized from tool behavior/ecosystem discussions.

---

## 1. External State of the Art

### 1.1 Deep-Dive on SOTA Linters, Compilers & Style Guides

1. **Ruff `F821` (`undefined-name`) & Pyflakes** ✅
   - **Mechanism**: Builds a lexical scope and binding table per module. At module and class scope (where Python statements execute sequentially at import time), a read reference to a name `x` before any binding of `x` in that scope is flagged as `F821: Undefined name 'x'`.
   - **Deferred scopes (function bodies)**: Because Python `def` / `async def` bodies are only executed when the function is called—not when the `def` statement runs—Ruff resolves unbound names inside function bodies against the *entire* enclosing module/function scope regardless of line order.
   - **Type annotations**: Respects `from __future__ import annotations` (PEP 563) and PEP 649 deferred annotation evaluation, as well as `if TYPE_CHECKING:` blocks, without false positives.
   - **Relevance to Omni**: Completely supersedes the legacy linter's `_check_top_level` (L3070–3079) with true control-flow/scope precision and zero false positives on assignment targets or deferred annotations (**D3**).

2. **Pylint `E0601` (`used-before-assignment`) & `E0602` (`undefined-variable`)** ✅
   - **Mechanism**: Performs control-flow-aware binding checks at module import time (`E0601`). Crucially, Pylint also tracks **import-time transitive calls**:
     ```python
     def caller() -> int:
         return helper()  # Pylint E0601 ONLY because caller() is invoked at line 4 before def helper()!

     caller()

     def helper() -> int:
         return 42
     ```
   - When `caller()` is **not** invoked before `def helper()`, Pylint emits zero warnings because `helper` is bound in the module namespace before `caller()` can ever be called by external importers or an `if __name__ == "__main__":` block at the bottom of the file.

3. **ESLint `no-use-before-define`, `@typescript-eslint/no-use-before-define`, & Oxlint** ✅
   - **Origin**: Created for JavaScript's var/function hoisting vs. ES6 Temporal Dead Zone (`let`, `const`, `class`).
   - **Granular Options**:
     ```json
     "no-use-before-define": ["error", {
       "functions": true,
       "classes": true,
       "variables": true,
       "allowNamedExports": false,
       "ignoreTypeReferences": true
     }]
     ```
   - **Ecosystem Experience with `functions: true`**:
     - Major JavaScript/TypeScript configurations (Airbnb, StandardJS, Google TypeScript Style Guide) explicitly set `{"functions": false, "classes": true, "variables": true}`, and Biome's `noUseBeforeDefine` rule focuses on TDZ / variable hazards rather than hoisted function declarations.
     - **Why `functions: true` is controversial in ESLint**:
       1. ESLint's `functions: true` does **not** exempt mutual recursion (`a()` calls `b()` and `b()` calls `a()`), forcing `// eslint-disable-next-line no-use-before-define` on every mutually recursive parser or state machine.
       2. Type references (`ignoreTypeReferences: true` in `@typescript-eslint`) had to be exempted because types frequently reference definitions declared later in the file.
       3. Many teams follow the top-down Stepdown Rule (exported public functions at the top of the module, internal helpers at the bottom).

4. **`wemake-python-styleguide` `WPS338` (`WrongMethodOrderViolation`)** ✅
   - **What it checks**: Enforces method declaration order inside Python classes by **visibility and role**, *not* by call graph:
     1. `@abstractmethod`
     2. `@classmethod`
     3. Magic/dunder methods (`__new__`, `__init__`, `__call__`, `__str__`, ...)
     4. Public instance methods (`def run(self):`)
     5. `@staticmethod`
     6. Protected/private methods (`def _helper(self):`, `def __helper(self):`)
   - **Direct Conflict with the legacy linter's `DefineBeforeUseRule` in Classes**:
     - Under `WPS338` (and universal Python convention), `__init__` and public methods come **above** `_private` helper methods.
     - Under the legacy linter's `DefineBeforeUseRule`, if `__init__` or `run(self)` calls `self._helper()`, `_helper` must be placed **above** `__init__` and `run(self)`!
     - This explains why applying define-before-use unconditionally to `__init__` (or to public methods calling `_private` methods in classes) clashes with standard Python class organization (**F7**, **D6**).

5. **Robert C. Martin's *Clean Code* ("The Stepdown Rule", Ch. 5) vs. Bottom-Up Declaration Order** ✅
   - **The Stepdown Rule (Top-Down)**:
     - *"We want the code to read like a top-down narrative. We want every function to be followed by those at the next level of abstraction so that we can read the program, descending one level of abstraction at a time as we read down the list of functions."*
     - Widely used in Java, Rust, TypeScript, and object-oriented Python (especially classes).
   - **Bottom-Up ("Define-Before-Use" / Leaf-to-Root)**:
     - Originated in single-pass compilers (Pascal, C without forward prototypes) and remains standard in functional languages (OCaml, F#, Haskell without mutual `rec` blocks, Bash/shell scripts) and many Python scripts where `main()` and `if __name__ == "__main__":` sit at the very bottom of the file.
     - **Why Python scripts often use Bottom-Up at module level**:
       - Because Python has no hoisting at top level, the module entrypoint (`if __name__ == "__main__": main()`) *must* sit at the bottom of the file. Placing `def main():` immediately above `if __name__ == "__main__":` and defining its helper functions above `main()` keeps the entrypoint and the `__main__` guard together.
   - **PEP 8 & Google Python Style Guide** ✅:
     - Neither PEP 8 nor the Google Python Style Guide mandates top-down or bottom-up function ordering at module level; both only require `if __name__ == "__main__":` at the bottom of executable files.

6. **Rust Clippy (`clippy::items_after_statements`)** ✅
   - Rust allows items (`fn`, `struct`, `const`) in any order inside modules and `impl` blocks. Clippy has **no lint** requiring functions to be defined before they are called.
   - `clippy::items_after_statements` (pedantic) only checks that inner `fn`/`struct` declarations inside a block appear before executable statements in that block—never that helper `fn`s appear before caller `fn`s at module or `impl` level.

---

### 1.2 Comparison Table (`R1`–`R7`)

| ID | Reference | Key Ideas | Adopt / Adapt / Reject | Why |
| :--- | :--- | :--- | :--- | :--- |
| **R1** | **Ruff `F821`** (`undefined-name`) & **Pylint `E0601`** (`used-before-assignment`) ✅ | Full lexical scope & import-time execution order check for Python; catches every top-level runtime `NameError` while respecting `from __future__ import annotations` and `TYPE_CHECKING`. | **Adopt** as the reason to **drop `_check_top_level`** (**D3**). | Duplicating `F821` inside a declaration-order rule violates the Split Test ([rule_design_guide.md](../rule_design_guide.md) §1) and produces false positives on assignment targets and annotations. |
| **R2** | **ESLint / `@typescript-eslint` `no-use-before-define`** ✅ | Separate toggles for `functions`, `classes`, `variables`, `ignoreTypeReferences`. `functions: true` is widely disabled because it lacks a mutual-recursion exemption and fights top-down organization. | **Adopt** `ignoreTypeReferences` (never flag type annotations, **C2**).<br>**Improve** on ESLint by exempting mutual recursion via SCC analysis (**D5**). | Fixing the two biggest pain points of ESLint's `functions: true` (type annotations and mutual recursion) eliminates its main sources of false positives. |
| **R3** | **`wemake-python-styleguide` `WPS338`** (`WrongMethodOrderViolation`) ✅ | Orders class methods by role/visibility: `@classmethod` $\to$ `__init__` / magic $\to$ public $\to$ `@staticmethod` $\to$ `_private`. | **Adapt**: exempt constructor lifecycle methods (`__init__`, `__new__`, `__post_init__`, **D6**), or scope the rule to module-level functions (see **Q2**). | Forcing `_private` helpers above `__init__` in a Python class is an anti-pattern in every Python style guide. |
| **R4** | ***Clean Code* Stepdown Rule vs. Bottom-Up Order** ✅ | Top-down (public API first, helpers below) vs. bottom-up (helpers first, callers and `main()` at the bottom). Both are mainstream conventions. | **Adopt** classification `Consensus::Opinionated` and `ImpactedQuality::Maintainability` (**D4**). | Directly satisfies the `tag_guide.md` §2.3 test: top-down Stepdown ordering is an ordinary, valid alternative. |
| **R5** | **The legacy linter `DefineBeforeUseRule`** (`legacy_lints.py` L3049–3133) ✅ | Checks module-level `def`s and class `self.`/`cls.` methods; deduplicates reports per `(caller, target)` pair; exempts direct self-recursion (`target == stmt.name`). | **Adopt** Python-only scope (**D2**) and per-`(caller, callee)` deduplication (**D8**).<br>**Reject** `_check_top_level` (**F1**), raw `ast.Name`/`ast.Attribute` matching (**F3**, **F5**), partial `_collect_local_vars` (**F4**), and missing mutual-recursion exemption (**F2**). | Preserves the legacy linter's intended bottom-up helper ordering while curing all seven defects (F1–F7). |
| **R6** | **Tarjan's / Reachability SCC Exemption for Mutual Recursion** ✅ | Two functions $u, v$ in the same scope are mutually recursive iff $u \to^+ v$ and $v \to^+ u$ in the intra-scope call graph (i.e., same Strongly Connected Component). | **Adopt** (**D5**). | Since a module or class typically has $N < 50$ functions, a simple DFS reachability check `can_reach(callee, caller)` on the adjacency map runs in $O(V + E)$ microseconds and handles cycles of any length ($2, 3, \dots, k$). |
| **R7** | **Rust Clippy & Omni Dogfooding (`src/`)** ✅ | Rust idioms (including Omni's own [runner.rs](../../../src/code_lint/runner.rs) L185–297) place `pub fn` entrypoints above private helpers. | **Adopt** `SupportLang::Python` only (**D2**). | Enforcing bottom-up order on Rust would flag idiomatic Rust (`pub fn` at top of module/`impl`) across Omni's own codebase. |

---

## 2. Internal Codebase Architecture & Tree-Sitter Analysis

### 2.1 Reusable AST & Rule Infrastructure in Omni

| Need | Existing Building Block | Location |
| :--- | :--- | :--- |
| Walking Python classes & methods | `extract_classes`, `PythonClassInfo`, `has_decorator` | [src/code_lint/ast/python.rs](../../../src/code_lint/ast/python.rs) |
| Detecting receiver parameters (`self`, `cls`) & `@staticmethod` | `PythonParameterKind::Receiver`, `extract_function_signatures` | [src/code_lint/ast/python.rs](../../../src/code_lint/ast/python.rs) |
| Transparent module-level blocks (`if`, `try`, `with`) | `CONSTANT_TRANSPARENT_STATEMENTS` pattern | [src/code_lint/ast/python.rs](../../../src/code_lint/ast/python.rs) |
| Write-target & binding detection in Python CST | `is_write_target`, `TARGET_CONTAINER_KINDS` | [src/code_lint/ast/python.rs](../../../src/code_lint/ast/python.rs) |
| Template placeholders `{function}`, `{callee}` and verb `Move` | `PLACEHOLDERS`, `SUGGESTION_VERBS` | [tests/registry.rs:L352-L402](../../../tests/registry.rs#L352-L402) |

### 2.2 Critical Tree-Sitter Python (`tree-sitter-python`) CST Details
If implemented in `src/code_lint/ast/python.rs`, the CST collector must account for these exact `tree-sitter-python` grammar shapes to avoid the legacy linter's defects (**F2–F6**):

1. **Decorated Definitions (`decorated_definition`)**:
   - In `tree-sitter-python`, a function with decorators (`@overload`, `@classmethod`, `@staticmethod`, `@property`, `@foo.setter`) is a `decorated_definition` node whose `definition` field is the inner `function_definition`.
   - When scanning `module` or `class_definition` -> `block` children, both direct `function_definition` and `decorated_definition` (wrapping a `function_definition`) must be recognized as sibling functions.
   - The definition's line/byte position is the start of the `function_definition` (or `decorated_definition`), and `@overload` stubs / `@<prop>.setter` / `@<prop>.deleter` definitions sharing a name with an earlier sibling in the same epoch inherit the first definition's position (**D7**).

2. **Restricting to Call Expressions (`call` -> `function`)**:
   - Instead of the legacy linter's `isinstance(sub, ast.Name)` (which falsely matches type annotations, assignment targets, and keyword arguments) or `isinstance(sub, ast.Attribute)` (which falsely matches `self.attr = 1` and `self.prop`), inspect `call` nodes via `call_node.field("function")`:
     - **Module call (`callee(...)`)**: `function.kind() == "identifier"`, where the identifier text is in the module's `func_defs` and not in the caller's local bindings.
     - **Class method call (`self.callee(...)` / `cls.callee(...)`)**: `function.kind() == "attribute"` where `function.field("object")` is an `identifier` matching the enclosing method's receiver parameter name (first parameter of a non-`@staticmethod` method, typically `self` or `cls`) and `function.field("attribute")` is in the class's `func_defs`.
     - **Module function called from inside a class method (**F6** fix)**: A bare `call` (`function.kind() == "identifier"`) inside a class method that is not locally bound in the method can also be checked against the enclosing **module's** `func_defs`!

3. **Skipping Default Arguments, Decorators, and Type Annotations on `caller`**:
   - Only walk the `body` (`block`) of `function_definition` when collecting calls made by `caller`.
   - Never walk `parameters` (default values and parameter type annotations), `return_type` (`-> Annotation`), or enclosing `decorator` nodes as part of `caller`'s body.
   - Stop descending into nested `class_definition` bodies (methods of a local class have their own class scope), while descending into nested `function_definition` and `lambda` bodies only after unioning their parameter/local bindings into the shadowed-name set.

4. **Complete Local Binding Extraction in `caller` (**F4** fix)**:
   - A bare call `foo(...)` inside `caller` is shadowed (not a module-level function call) if `foo` is bound anywhere in `caller` (or an enclosing nested `def`/`lambda`) by:
     - `parameters` / `lambda_parameters` (all parameter identifiers: `identifier`, `typed_parameter`, `default_parameter`, `typed_default_parameter`, `list_splat_pattern`, `dictionary_splat_pattern`),
     - `assignment` / `augmented_assignment` (`left` field, recursively extracting all `identifier` nodes inside `pattern_list`, `tuple_pattern`, `list_pattern`, `list_splat_pattern` while stopping at `attribute` or `subscript`),
     - `named_expression` (`:=`, `name` field),
     - `for_statement` / `for_in_clause` (`left` field),
     - `with_item` (`as_pattern` -> `as_pattern_target`),
     - `except_clause` (`as_pattern` -> `as_pattern_target`),
     - `case_pattern` (`as_pattern`, capture identifiers),
     - nested `function_definition` / `class_definition` (`name` field),
     - `import_statement` / `import_from_statement` (imported names and `aliased_import` `alias` names).

5. **Mutual Recursion Exemption via Reachability (**F2** fix / **D5`)**:
   - Within each scope epoch (`functions: Vec<ScopeFunction>`), construct the directed call edges `edges: HashMap<&str, HashSet<&str>>` between sibling functions in that scope.
   - Before flagging a forward call from `caller` to `callee` (`def_pos[callee] > def_pos[caller]`), check `can_reach(callee, caller, &edges)` via a simple bounded BFS/DFS with a `visited: HashSet<&str>` set.
   - If `can_reach(callee, caller, &edges)` is `true`, `caller` and `callee` are in a mutual-recursion cycle (SCC of size $\ge 2$, or self-loop if `caller == callee`) $\to$ **exempt**!

6. **Message Template & Registry Compliance ([tests/registry.rs](../../../tests/registry.rs))**:
   - Using existing placeholders `{function}` and `{callee}` and existing suggestion verb `Move`:
     ```rust
     const TEMPLATE: ViolationTemplate = violation_template! {
         summary: "Function `{function}` calls `{callee}()` before `{callee}` is defined.",
         rationale: "Calling a helper before its declaration forces readers scanning top-to-bottom to jump ahead in the file to learn its contract before finishing the caller.",
         suggestion: "Move the definition of `{callee}` above `{function}`.",
     };
     ```
   - Passes every check in `tests/registry.rs` (`test_rule_names_follow_the_naming_grammar`, `test_violation_templates_follow_style_guide`, `PLACEHOLDERS`, `SUGGESTION_VERBS`) without needing any changes to `tests/registry.rs`.

---

## 3. Configuration for Dropped Top-Level Check (`_check_top_level`) (`pyproject.toml`)

The legacy linter's top-level / module-import-time reference-before-assignment check (`_check_top_level`) is dropped in Omni (`D3`) because Ruff `F821` and Pylint `E0601`/`E0602` enforce it with full lexical scope, control-flow, and PEP 563/649 deferred-annotation awareness:

```toml
[tool.ruff.lint]
extend-select = [
    "F821", # undefined-name: flags top-level and class-body names read before they are bound
    "F823", # undefined-local: flags local variables referenced before assignment (`UnboundLocalError`)
]

[tool.pylint."messages control"]
enable = [
    "used-before-assignment", # E0601: also catches import-time calls to functions whose body calls a not-yet-defined function
    "undefined-variable",     # E0602
]
```

---

## 4. Sources

- Ruff `F821` (`undefined-name`): https://docs.astral.sh/ruff/rules/undefined-name/
- Pylint `E0601` (`used-before-assignment`): https://pylint.readthedocs.io/en/stable/user_guide/messages/error/used-before-assignment.html
- ESLint `no-use-before-define`: https://eslint.org/docs/latest/rules/no-use-before-define
- TypeScript-ESLint `@typescript-eslint/no-use-before-define`: https://typescript-eslint.io/rules/no-use-before-define/
- `wemake-python-styleguide` `WPS338` (`WrongMethodOrderViolation`): https://wemake-python-styleguide.readthedocs.io/en/latest/pages/usage/violations/consistency.html
- Robert C. Martin, *Clean Code: A Handbook of Agile Software Craftsmanship* (2008), Chapter 5 ("Formatting — Dependent Functions / The Stepdown Rule").
- Rust Clippy `items_after_statements`: https://rust-lang.github.io/rust-clippy/master/index.html#items_after_statements
