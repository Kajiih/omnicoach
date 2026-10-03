# Phase 2: References & SOTA Analysis — `EmptyInitRule` (`non-empty-init`)

## 1. External SOTA Reference Catalog (`R1`–`R7`)

### R1 — the legacy linter `EmptyInitRule` (`empty-init`)
- **Source**: `scratch/legacy_reference/legacy_lints.py` lines 3619–3638.
- **Severity**: Registered in `_ERROR_RULES`.
- **Mechanism**:
  - Hooks `ast.Module`.
  - Checks `self.visitor.path.name == "__init__.py"`.
  - If `node.body` is non-empty, reports at `getattr(node.body[0], "lineno", 1)`: `"__init__.py must be completely empty."`
- **Nuances**:
  - `#` comments and whitespace are not in `ast.Module.body`, so comment-only `__init__.py` files pass.
  - A module docstring (`"""..."""`) is represented in Python's AST as `ast.Expr(value=ast.Constant(value="..."))` in `node.body[0]`, so the legacy linter flags module docstrings alongside imports, `__all__`, and executable code.
  - Emits only one diagnostic per file (at `node.body[0]`).

---

### R2 — Ruff `RUF067` (`non-empty-init-module`)
- **Source**: `astral-sh/ruff` — `crates/ruff_linter/src/rules/ruff/rules/non_empty_init_module.rs` (introduced in Ruff v0.14.11, `RuleGroup::Preview`, `Category::Pedantic`).
- **Configuration**: `lint.ruff.strictly-empty-init-modules: bool` (default: `false`).
- **Two Operating Modes**:
  1. **Default Mode (`strictly-empty-init-modules = false`)**:
     - **Message**: ``"`__init__` module should only contain docstrings and re-exports"``
     - **Allowed constructs in `__init__.py`**:
       - Module-level docstrings (`in_pep_257_docstring`) and attribute docstrings (`in_attribute_docstring`).
       - `Stmt::Import` and `Stmt::ImportFrom` (re-exports).
       - Module-level PEP 562 `def __getattr__(...)` and `def __dir__(...)` functions (`Stmt::FunctionDef` where `name` is `"__getattr__"` or `"__dir__"`).
       - `if TYPE_CHECKING:` / `if typing.TYPE_CHECKING:` blocks (checks condition via `is_type_checking_block`; notably does *not* allow `elif` or `else` branches on that `if`).
       - Assignments (`Stmt::Assign`, `Stmt::AnnAssign`, `Stmt::AugAssign`) where **all** target identifiers are dunder names (`target.starts_with("__") && target.ends_with("__")`), such as `__all__`, `__path__`, `__version__`, `__author__`, `__submodules__`.
     - **Flagged constructs**: All other statements (`class`, other `def`s, function calls, control flow `if`/`for`/`while`/`try`/`with`, non-dunder assignments, `del`, `assert`, `raise`, `pass`, etc.).
  2. **Strict Mode (`strictly-empty-init-modules = true`)**:
     - **Message**: ``"`__init__` module should not contain any code"``
     - **Behavior**: Flags **every** `Stmt` in `body` unconditionally:
       ```rust
       if !in_init_module(checker) {
           return;
       }
       if checker.settings().ruff.strictly_empty_init_modules {
           for stmt in body {
               checker.report_diagnostic(NonEmptyInitModule { strict: true }, stmt.range());
           }
           return;
       }
       ```
     - **Implementation note / quirk in Ruff**: Although a rustdoc comment at the top of `non_empty_init_module.rs` (line 69) says *"Both modes allow comments and module-level docstrings"*, the actual implementation (lines 982–989) returns early in strict mode before checking `in_pep_257_docstring`, flagging module docstrings too (and the user-facing markdown example at lines 957–963 shows deleting everything in `__init__.py`). Comments (`# ...`) are allowed in both modes because they are trivia outside `body: &[Stmt]`.
  3. **Path matching (`in_init_module`)**:
     - Checks `checker.source_type.is_py()` (explicitly **excludes `.pyi` stub files**!) and `path.file_name() == Some("__init__.py")`.

---

### R3 — Related Ruff Rules Interacting with `__init__.py`
1. **`INP001` (`implicit-namespace-package`, `flake8-no-pep420`)**:
   - Flags `.py` files in directories that lack an `__init__.py` file.
   - **Relationship to `EmptyInitRule`**: Orthogonal and complementary. `INP001` requires `__init__.py` to *exist* (preventing accidental PEP 420 implicit namespace packages); `EmptyInitRule` / `RUF067` requires that `__init__.py` file to be *empty*.
2. **`F401` (`unused-import`, `Pyflakes`) & `PLC0414` (`useless-import-alias`, `Pylint`)**:
   - In regular modules, importing a symbol without using it triggers `F401`, and `from .a import b as b` triggers `PLC0414`.
   - In `__init__.py`, type checkers (Mypy, Pyright per PEP 484) and linters treat `from .a import b as b` or listing `"b"` in `__all__` as an explicit public re-export, exempting it from `PLC0414` and `F401`.
   - When strict empty `__init__.py` is enforced, this entire class of re-export convention ambiguity disappears because `__init__.py` contains no imports at all.
3. **`D104` (`undocumented-public-package`, `pydocstyle`)**:
   - Requires every `__init__.py` to start with a module docstring.
   - **Direct conflict**: `D104` is mutually incompatible with strict `EmptyInitRule` / `RUF067` (`strictly-empty-init-modules = true`) unless module docstrings are exempted in `__init__.py` or `D104` is disabled.

---

### R4 — `flake8-empty-init-modules` (`EIM001` & `EIM002`)
- **Source**: `samueljsb/flake8-empty-init-modules` (the direct inspiration cited by Ruff `RUF067`).
- **Rule Split**:
  - **`EIM001` (enabled by default)**: Disallows *all* code in any `__init__.py` module (equivalent to the legacy linter `EmptyInitRule` and Ruff `RUF067` with `strictly-empty-init-modules = true`).
  - **`EIM002` (disabled by default, opt-in extension)**: Allows `import` / `from ... import` and assignment to `__all__` in `__init__.py`, flagging all other statements (the precursor to Ruff `RUF067`'s default mode).

---

### R5 — `wemake-python-styleguide` (`WPS412` & `WPS411`)
- **Source**: `wemake-services/wemake-python-styleguide` — `wemake_python_styleguide/violations/best_practices.py` (`InitModuleHasLogicViolation`, code `WPS412`) and `wemake_python_styleguide/visitors/ast/modules.py` (`EmptyModuleContentsVisitor`).
- **Evolution & Lessons Learned**:
  - **v0.14.0 – v0.19.x (`--i-control-code` era)**:
    - `WPS412` forbid all AST statements inside `__init__.py` *except* comments and module docstrings when `--i-control-code` was true (the default).
    - Rationale documented by wemake: *"1) Looking at `__init__.py` with contents tells nothing about whether the imports are used here to define a public API or simply due to convenience, 2) Putting anything in `__init__.py` increases the likelihood of circular imports, 3) There's only one place left you can import things from."*
    - Provided `--i-dont-control-code` for library authors who needed re-exports in `__init__.py`.
  - **v1.0.0**: Removed the `--i-control-code` CLI toggle as unnecessary configuration complexity, advising users with public facade `__init__.py` files to use standard `per-file-ignores`.
  - **v1.5.0 (Issue #3569)**: Relaxed `WPS412` to allow imports and docstrings in `__init__.py` while continuing to forbid executable logic, definitions, and control flow.
  - **Companion rule `WPS411` (`EmptyModuleViolation`)**: Enforces the exact inverse on non-`__init__.py` files (regular `.py` files must *not* be empty).

---

### R6 — Pylint
- **Status**: Pylint has **no built-in rule** requiring `__init__.py` to be empty or forbidding logic in `__init__.py`. Teams using Pylint historically wrote custom AST checkers (like the legacy linter's `EmptyInitRule`) or paired Pylint with Flake8/Ruff.

---

### R7 — Rust Clippy (`clippy::mod_module_files` & `clippy::self_named_module_files`)
- **Source**: `rust-lang/rust-clippy` (`clippy_lints/src/module_style.rs`, restriction group).
- **What Clippy does**:
  - `clippy::mod_module_files`: Forbids `foo/mod.rs` in favor of Rust 2018+ `foo.rs` alongside `foo/bar.rs` (note: Omni itself follows this convention across `src/`).
  - `clippy::self_named_module_files`: Enforces the inverse (`foo/mod.rs` instead of `foo.rs`).
- **Why Rust is fundamentally different from Python here**:
  1. **Compile-time module tree vs. runtime import execution**: In Rust, `mod bar;` in `foo.rs` / `foo/mod.rs` is a compile-time declaration telling `rustc` to include `foo/bar.rs` in the crate's AST. There is no runtime import execution order or "partially initialized module" circular import crash.
  2. **Language requirement**: In Rust, a parent module *must* contain `mod bar;` (or `pub mod bar;`) for `foo/bar.rs` to be compiled at all. An "empty `mod.rs`" makes all child files dead, uncompiled files on disk.
  - **Conclusion**: `non-empty-init` is strictly a **Python** rule (`SupportLang::Python`).

---

## 2. SOTA Comparison Matrix

| Construct in `__init__.py` | The legacy linter `EmptyInitRule` | Ruff `RUF067` (strict) | `flake8-empty-init-modules` `EIM001` | `WPS412` (classic `<1.5`) | Ruff `RUF067` (default) | `WPS412` (`>=1.5`) |
| :--- | :---: | :---: | :---: | :---: | :---: | :---: |
| Blank / `#` comments only | Pass | Pass | Pass | Pass | Pass | Pass |
| Module docstring (`"""..."""`) | **Flag** | **Flag** | **Flag** | Pass | Pass | Pass |
| `import` / `from ... import` | **Flag** | **Flag** | **Flag** | **Flag** | Pass | Pass |
| `__all__ = [...]` / dunders | **Flag** | **Flag** | **Flag** | **Flag** | Pass | **Flag** |
| `if TYPE_CHECKING:` | **Flag** | **Flag** | **Flag** | **Flag** | Pass | **Flag** |
| PEP 562 `def __getattr__` | **Flag** | **Flag** | **Flag** | **Flag** | Pass | **Flag** |
| `def` / `class` / logic / calls | **Flag** | **Flag** | **Flag** | **Flag** | **Flag** | **Flag** |
| `__init__.pyi` (type stub) | Pass | Pass | Pass | Pass | Pass | Pass |
| Reporting granularity | First stmt (`body[0]`) | Every top-level stmt | First / per stmt | Module | Every disallowed top-level stmt | Per disallowed node |

---

## 3. Internal Omni Architecture & Integration Analysis

### 3.1 Tree-sitter Python AST Representation of `module`
In `tree-sitter-python`:
- The root node of every Python file has kind `"module"` (`ctx.root.kind() == "module"`).
- Inside `"module"`, direct children (`ctx.root.children(&mut cursor)`) are:
  - **Comments (`# ...`)**: `node.kind() == "comment"` (with `node.is_extra() == true`).
  - **Statements**: Named children where `!node.is_extra()`, such as:
    - `expression_statement` (which wraps module docstrings `string`, function calls `call`, bare expressions, etc.)
    - `import_statement`, `import_from_statement`, `future_import_statement`
    - `function_definition`, `class_definition`, `decorated_definition`
    - `if_statement`, `for_statement`, `while_statement`, `try_statement`, `with_statement`, `match_statement`
    - `pass_statement`, `assert_statement`, `raise_statement`, `delete_statement`, `global_statement`, `type_alias_statement`
- Note: In `tree-sitter-python` v0.23+, top-level assignments (`x = 1`, `x: int = 1`, `x += 1`) are wrapped inside an `expression_statement` whose child is `assignment` or `augmented_assignment` (or at top level depending on grammar version). Iterating non-extra named children of `module` (`ctx.root`) visits every top-level statement cleanly in $O(k)$ where $k$ is the number of top-level statements, without needing a full recursive AST traversal (`AstRule` vs `CodeRule`).
- **Even simpler with `CodeRule`**:
  - Unlike `AstRule` (which walks every descendant node in the file via `Runner`), `CodeRule::check_file(&self, ctx: &FileContext<'_>, options: &RuleOptions)` is called **once per file**!
  - In `CodeRule::check_file`:
    1. If `ctx.file.file_name().and_then(|n| n.to_str()) != Some("__init__.py")`, return `Vec::new()` immediately in $O(1)$ without touching the AST at all!
    2. Otherwise, iterate the direct named, non-extra children of `ctx.root.as_ts_node()` (the `"module"` node) and emit a `RuleMatch` for each disallowed top-level statement.

### 3.2 `CodeRule` vs `AstRule` Choice
Implementing `non-empty-init` directly as a **`CodeRule`** (rather than via the `AstRule` adapter) has two concrete advantages:
1. **Zero per-node dispatch overhead on non-`__init__.py` files**: `AstRule` registers `target_kinds: &["module"]` and runs through `AstRuleAdapter`, whereas `CodeRule::check_file` checks `ctx.file.file_name()` at the start of `check_file` and returns immediately. (Even with `AstRule` targeting `"module"`, `"module"` only appears once at the root, so either is fast, but `CodeRule::check_file` is direct and has no inner node visitor boilerplate).
2. **Direct access to `ctx.file`, `ctx.root`, `ctx.comments`, and `options.enforcement`**: Everything needed is right on `FileContext`.

### 3.3 `docs/dev/naming_and_message_style_guide.md` & `tests/registry.rs` Compliance
Checking `src/rule_declaration/documentation.rs` and `tests/registry.rs`:
- **Approved `PLACEHOLDERS`** (`documentation.rs:11-36`):
  - `"{stmt}"` is already in `PLACEHOLDERS`!
- **Approved `SUGGESTION_VERBS`** (`documentation.rs:38-60`):
  - `"Move"` and `"Remove"` are both in `SUGGESTION_VERBS`!
- **Proposed `RuleDocumentation` fields**:
  - `name`: `"non-empty-init"`
  - `summary`: `"Flags top-level statements inside `__init__.py` files."`
  - `message`: `"{stmt} in `__init__.py` executes on every package import and couples submodules"`
  - `suggestion`: `"Move {stmt} to a dedicated submodule and import from there, or remove it to keep `__init__.py` empty"`
  - Every `tests/registry.rs` assertion (`kebab-case`, $\le 4$ words, no banned prefix, `message` contains `{...}`, `suggestion` starts with approved verb and contains `{...}`, no banned words like `consider`/`should`/`try to`) passes out of the box.

### 3.4 Critical Finding: `src/test_utils.rs` `dummy_filename` & `assert_every_occurrence_reported`
During Phase 2 inspection of `src/test_utils.rs`, we identified two critical harness interactions that **must** be accounted for in Phase 3 (Plan) and Phase 4 (Implementation):

1. **`dummy_filename(lang)` in `src/test_utils.rs` (lines 19–25)**:
   ```rust
   const fn dummy_filename(lang: SupportLang) -> &'static str {
       match lang {
           SupportLang::Rust => "test.rs",
           SupportLang::Python => "test.py",
       }
   }
   ```
   Both `rule_test!` (`check_rule_matches`) and `tests/registry.rs` (`assert_documented_examples`) call `check_rule_with_options(rule, code, lang, ...)`, which passes `Path::new(dummy_filename(lang))` = `"test.py"` into `run_code_rule`!
   - **Impact**: If `non-empty-init` only triggers when `ctx.file.file_name() == "__init__.py"`, then passing `"test.py"` in `rule_test!` and `assert_documented_examples` will cause `non-empty-init` to skip the file and emit **0 matches**, failing all `fail` tests and `registry::documented_examples_match_rule_behavior`!
   - **Clean Solutions (to choose in Phase 3)**:
     - **Option 1 (Trait method on `CodeRule`)**: Add a default method on `CodeRule` in `src/code_lint/contract.rs`:
       ```rust
       fn test_filename(&self, lang: SupportLang) -> &'static str {
           match lang {
               SupportLang::Rust => "test.rs",
               SupportLang::Python => "test.py",
           }
       }
       ```
       And override `fn test_filename(&self, _lang: SupportLang) -> &'static str { "__init__.py" }` on `NonEmptyInit`. Then `test_utils::check_rule_with_options` uses `rule.test_filename(lang)` instead of `dummy_filename(lang)`. Furthermore, in `non_empty_init.rs` unit tests, we can also directly call `run_code_rule` with `Path::new("pkg/module.py")` and `Path::new("pkg/__init__.pyi")` to test that non-`__init__.py` files are ignored!
     - **Option 2 (Rule-level or `test_utils` lookup)**: Have `test_utils.rs` check rule metadata or provide a `file:` key in `rule_test!`, while `assert_documented_examples` uses `rule.test_filename(lang)`.
     - *Assessment*: **Option 1 (`CodeRule::test_filename` with default implementation)** is 6 lines of code, zero boilerplate for all existing/future rules, makes `assert_documented_examples` and `rule_test!` work transparently for path-scoped rules (like `non-empty-init`), and still lets unit tests verify path filtering via `run_code_rule`.

2. **`assert_every_occurrence_reported` in `src/test_utils.rs` (lines 146–188)**:
   - For every `fail` test case, `test_utils.rs` concatenates `format!("{code}\n{code}")` and asserts that the rule emits `2 * N` diagnostics (each original span plus `span + offset`).
   - Because **D5** reports **each top-level statement** in `__init__.py` (matching Ruff `RUF067`), a 1-statement `fail` snippet (`from .service import Client`) emits 1 match normally and 2 matches when doubled (`from .service import Client\nfrom .service import Client`), passing `assert_every_occurrence_reported` cleanly.

---

## 4. Refinements & Recommendations for Phase 3 (`R-D1`–`R-D4`)

Based on the SOTA research and Omni codebase analysis above, here are the refined design recommendations to carry into Phase 3:

- **R-D1 — Strictness & Docstrings**:
  - **Strict on code and re-exports** (flag imports, `__all__`, definitions, and runtime logic in `__init__.py`), matching the legacy linter `EmptyInitRule`, `flake8-empty-init-modules` (`EIM001`), and Ruff `RUF067` (`strictly-empty-init-modules = true`).
  - **Module docstrings (`"""..."""`)**: We should decide whether to follow the legacy linter/Ruff-strict literally (flagging even module docstrings so `__init__.py` is truly 0 AST statements, using `#` comments if file notes are needed) or exempt a leading module docstring (`WPS412` / `D104` compatibility). Note: if we exempt leading module docstrings, `assert_every_occurrence_reported` still passes because module docstrings are `pass` cases (`assert_every_occurrence_reported` only runs on `fail` cases!).
- **R-D2 — Enforcement Mode**:
  - Use **`EnforcementMode::Ban`** (or if `RequireExplanation` is required by project convention, document that explanatory comments excuse individual statements). Wait: let's check how other rules in `src/code_lint/rules/` configure `EnforcementMode`!
  - In `ROADMAP.md`, under "Philosophy: Explain or Fix, Never Silently Ignore", rules where legitimate exceptions exist can use `RequireExplanation`, whereas rules that are unconditional bans use `Ban`. For `__init__.py`, if a user puts `# Public API` above `from .foo import bar`, under `RequireExplanation` that comment would excuse `from .foo import bar`. If the user *wants* explanatory comments above intentional facade exports in `__init__.py` to excuse the warning, `RequireExplanation` enables that out of the box! Conversely, if `__init__.py` must be strictly empty unless `# omni:ignore[non-empty-init] -- reason` is used, `EnforcementMode::Ban` is better.
- **R-D3 — Per-Statement Diagnostic Span & `{stmt}` Formatting**:
  - Report each disallowed top-level statement in `module`, binding `{stmt}` to a concise human-readable description or snippet of the statement (e.g., ``"`from .foo import bar`"`` or statement kind / first-line snippet) so the diagnostic message `"{stmt} in `__init__.py` executes on every package import and couples submodules"` is specific and actionable.
- **R-D4 — Test Harness `test_filename`**:
  - Add `fn test_filename(&self, lang: SupportLang) -> &'static str` with a default implementation (`"test.rs"` / `"test.py"`) to `CodeRule` in `src/code_lint/contract.rs` and use it in `src/test_utils.rs`, overriding it to `"__init__.py"` for `NonEmptyInit`, plus dedicated unit tests in `non_empty_init.rs` verifying that `test.py` and `__init__.pyi` produce zero diagnostics.

---

## 5. Final Decision Record (`VALIDATED — DROPPED`) & Recommended Configuration (`pyproject.toml`)

**Final Decision**: **Drop `EmptyInitRule`** in Omni (`ROADMAP.md` *Not pursued*) and rely on Ruff's built-in **`RUF067` (`non-empty-init-module`)**, which supports both strict the legacy linter parity (`strictly-empty-init-modules = true`) and library facade mode (`strictly-empty-init-modules = false`).

### 5.1 Strict Mode (1:1 Parity with the legacy linter `EmptyInitRule` & `flake8-empty-init-modules` `EIM001`)
Disallows all statements (including imports, `__all__`, and docstrings) in `__init__.py`, allowing only `#` comments and whitespace:

```toml
[tool.ruff.lint]
preview = true # Required while RUF067 is in Ruff's preview group
extend-select = [
    "RUF067", # non-empty-init-module: flags statements inside `__init__.py`
    "INP001", # implicit-namespace-package: requires `__init__.py` to exist in package directories
]
# Optional: if pydocstyle (`D`) is enabled, ignore D104 so empty `__init__.py` files are not flagged for missing docstrings
extend-ignore = ["D104"]

[tool.ruff.lint.ruff]
strictly-empty-init-modules = true
```

### 5.2 Facade-Permissive Mode (Ruff `RUF067` Default / `WPS412`)
Allows module docstrings, re-exports (`import` / `from ... import`), `__all__` / dunder assignments, `if TYPE_CHECKING:` blocks, and PEP 562 `__getattr__` / `__dir__`, while flagging all other definitions and executable logic in `__init__.py`:

```toml
[tool.ruff.lint]
preview = true
extend-select = [
    "RUF067", # non-empty-init-module
    "INP001", # implicit-namespace-package
]

[tool.ruff.lint.ruff]
strictly-empty-init-modules = false # Default
```
