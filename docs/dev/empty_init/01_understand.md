# Phase 1: Understand — `EmptyInitRule` (`non-empty-init`)

> **Status**: **VALIDATED — DROPPED** (2026-10-03).
> **Decision**: **Drop `EmptyInitRule`** in favor of Ruff's built-in [`RUF067` (`non-empty-init-module`)](https://docs.astral.sh/ruff/rules/non-empty-init-module/) paired with [`INP001` (`implicit-namespace-package`)](https://docs.astral.sh/ruff/rules/implicit-namespace-package/). See [02_references.md §5](02_references.md) for the exact `pyproject.toml` configuration (both strict and facade-permissive modes).

## 1. Problem Statement

### 1.1 What the legacy linter's `EmptyInitRule` Does

In `scratch/legacy_reference/legacy_lints.py` (lines 3619–3638), `EmptyInitRule` is registered as an error rule (`_ERROR_RULES`) with rule ID `"empty-init"`:

```python
class EmptyInitRule(LintRule):
    """Flags __init__.py files that are not empty."""

    rule_id = "empty-init"

    def match(self, node: ast.AST) -> None:
        if not isinstance(node, ast.Module):
            return
        if self.visitor.path.name != "__init__.py":
            return
        if node.body:
            self.visitor.report(
                getattr(node.body[0], "lineno", 1),
                "__init__.py must be completely empty.",
                self.rule_id,
            )
```

Exact behavioral breakdown of `EmptyInitRule`:
1. **Path gating**: Runs only when `self.visitor.path.name == "__init__.py"`. All other Python files (`.py`, `.pyi`, `__main__.py`, etc.) are ignored.
2. **AST body check (`if node.body:`)**:
   - **Comments (`# ...`) and whitespace**: Stripped by Python's lexer before constructing `ast.Module`, so `node.body` is `[]`. Comment-only or blank `__init__.py` files **pass**.
   - **Module docstrings (`"""Package docs."""`)**: Parsed by Python's AST as `ast.Expr(value=ast.Constant(value="..."))` at `node.body[0]`. Consequently, the legacy linter **flags even a module docstring**.
   - **Re-exports (`from .foo import bar`), `__all__ = [...]`, `if TYPE_CHECKING:`, and executable logic**: All populate `node.body` and are **flagged**.
3. **Diagnostic emission**: Reports a single diagnostic per file at `node.body[0].lineno` (falling back to line `1`) with the message `"__init__.py must be completely empty."`

---

### 1.2 Why Strict Codebases Enforce Empty `__init__.py`

While Python allows arbitrary code in `__init__.py` (and PEP 420 allows omitting `__init__.py` via implicit namespace packages), large monorepos and application codebases frequently pair **`INP001` (require `__init__.py` to exist)** with **empty `__init__.py` enforcement** for four concrete engineering reasons:

1. **Preventing Circular Import Landmines**:
   - In Python, importing `pkg.sub_a` *always* loads and executes `pkg/__init__.py` first.
   - If `pkg/__init__.py` eagerly imports `pkg.sub_a` and `pkg.sub_b` (to re-export their public symbols), and `pkg.sub_a` imports a helper from `pkg.sub_b`, importing `pkg.sub_b` first triggers `pkg/__init__.py` $\rightarrow$ `pkg.sub_a` $\rightarrow$ `pkg.sub_b` (partially initialized) $\rightarrow$ `ImportError: cannot import name '...' from partially initialized module`.
   - Keeping `__init__.py` empty makes internal packages leaf-only in the import graph, eliminating package-hub import cycles by construction.
2. **Eliminating Eager Transitive Imports & Startup Latency**:
   - When `pkg/__init__.py` acts as a "barrel file" (re-exporting classes/functions from submodules), importing `pkg.lightweight_types` forces the Python interpreter to execute `pkg/__init__.py` and transitively import every heavy dependency (e.g., database drivers, ML frameworks, HTTP clients) re-exported in `pkg/__init__.py`.
   - Empty `__init__.py` guarantees pay-for-what-you-use import cost: importing `pkg.a` never loads `pkg.b`.
3. **Enforcing Explicit, Grep-able Canonical Import Paths ("One Obvious Origin")**:
   - Without re-exports in `__init__.py`, every symbol has a single canonical import path matching its defining file (`from pkg.service import Client` rather than a mix of `from pkg import Client` and `from pkg.service import Client`).
   - Static analysis tools, automated refactorings, and AI coding agents can map an import directly to its file path without chasing re-export chains through `__init__.py`.
4. **Preventing Hidden Import-Time Side Effects**:
   - Code placed in `__init__.py` executes implicitly whenever any submodule under that directory is imported, making side effects (logging configuration, global state mutation, environment checks, registry initialization) order-dependent and difficult to isolate in unit tests.

---

### 1.3 Real-World Trade-offs: Application/Monorepo vs. Published Library

Enforcing empty `__init__.py` files has well-known trade-offs depending on the package's role:

| Dimension | Strict Empty `__init__.py` (the legacy linter / Ruff `RUF067` strict / `EIM001`) | Facade-Permissive `__init__.py` (Ruff `RUF067` default / `EIM002` / `WPS412`) |
| :--- | :--- | :--- |
| **Target codebase** | Internal monorepos, services, applications, and agent-authored codebases where all callers import submodules directly. | Published third-party libraries exposing a curated top-level API (`import requests; requests.get(...)`) while hiding internal file layout. |
| **Re-exports (`from .mod import X`) & `__all__`** | **Banned** (or requires explicit suppression / justification comment). Prevents barrel-file bloat and circular imports. | **Allowed**. Enables ergonomic public package namespaces and decouples public API paths from internal module splits. |
| **Module docstrings (`"""..."""`)** | **Banned in the legacy linter & Ruff strict** (because `node.body` is non-empty); **Allowed in `WPS412`**. Banning docstrings conflicts with `pydocstyle` `D104` (`undocumented-public-package`). | **Allowed**. Supports `help(pkg)`, Sphinx/MkDocs autodoc, and `D104`. |
| **PEP 562 (`__getattr__`, `__dir__`) & `if TYPE_CHECKING:`** | **Banned**. | **Allowed in Ruff `RUF067` default** (enables lazy loading of package attributes without eager import penalties). |
| **Executable logic (`def`, `class`, calls, loops, runtime assignments)** | **Banned**. | **Banned**. Both philosophies agree runtime logic and definitions do not belong in `__init__.py`. |

---

## 2. Requirements & Scope Analysis

### 2.1 Goals

- **G1 — Port the legacy linter's `EmptyInitRule` to Omni**: Prevent `__init__.py` files from becoming barrel files, import-cycle hubs, or containers for hidden import-time logic.
- **G2 — Align with SOTA (`RUF067`, `WPS412`, `EIM001`/`EIM002`)**: Support the strict monorepo posture (the legacy linter's intent) while deliberately deciding how module docstrings, re-exports, and `EnforcementMode` (`Ban` vs `RequireExplanation`) are handled.
- **G3 — Follow Omni Naming, Message, and Tag Standards**:
  - Rule name must describe the *flagged AST pattern* without `no-`/`prefer-`/`enforce-` prefixes (`docs/dev/naming_and_message_style_guide.md` §1): e.g., `non-empty-init` (matching Ruff `RUF067` `non-empty-init-module`, concise at 3 words).
  - Message and suggestion must satisfy `tests/registry.rs` invariants (`{stmt}` placeholder, approved suggestion verb `Move` or `Remove`).
- **G4 — Seamless Compatibility with Omni's Test Harness (`src/test_utils.rs`)**:
  - Account for how `rule_test!` and `assert_documented_examples` invoke `run_code_rule` (which currently passes `dummy_filename(lang) = "test.py"` and checks `assert_every_occurrence_reported` by duplicating `format!("{code}\n{code}")`).
- **G5 — Evaluate Cross-Language Applicability**: Document why this rule targets Python only (`SupportLang::Python`) and how Rust's module system (`mod.rs` vs `foo.rs`, covered by `clippy::mod_module_files`) fundamentally differs.

### 2.2 Non-Goals

- **NG1 — Enforcing presence of `__init__.py` (`INP001`)**: Checking whether a directory is missing an `__init__.py` file is a filesystem/directory-level check (`implicit-namespace-package`), not an AST rule over parsed files.
- **NG2 — Banning empty regular `.py` files (`WPS411`)**: Out of scope for this rule.
- **NG3 — Checking `.pyi` stub files (`__init__.pyi`)**: Type stub files (`__init__.pyi`) exist specifically to declare package-level type signatures and re-exports without runtime execution cost; they must not be flagged.
- **NG4 — Rust `mod.rs` enforcement**: Rust requires `mod` declarations in module roots (`foo.rs` or `foo/mod.rs`) for the compiler to include child modules in compilation; an "empty `mod.rs`" cannot expose submodules at all. Enforcing `foo.rs` over `foo/mod.rs` is a separate file-layout rule (`clippy::mod_module_files`).

---

## 3. Initial Decisions (`D1`–`D9`) & Open Questions (`Q1`–`Q4`)

### 3.1 Decisions

- **D1 — Rule Name**: **`non-empty-init`**
  - *Rationale*: the legacy linter's ID `empty-init` names the *desired state* rather than the *flagged pattern*, which violates `docs/dev/naming_and_message_style_guide.md` §1 (*"The rule name describes the flagged pattern — the thing in the code that triggers the diagnostic"*). Ruff uses `non-empty-init-module` (`RUF067`). `non-empty-init` is 3 words, has no banned prefixes (`no-`, `prefer-`, `enforce-`), and immediately communicates what is flagged. (Alternative: `code-in-init`).
- **D2 — Target Language**: **`SupportLang::Python` only**
  - *Rationale*: `__init__.py` import-time execution and barrel-file circular imports are specific to Python's runtime package loader. Rust's `mod.rs` is compile-time and structural (already covered by `clippy::mod_module_files`).
- **D3 — File Matching**: **Match only when `file.file_name() == Some("__init__.py")`** (with test-harness support; see D8)
  - *Rationale*: `__init__.pyi` type stubs, `__main__.py`, and regular `.py` modules must never be flagged.
- **D4 — Comments (`# ...`) and Whitespace**: **Always allowed**
  - *Rationale*: Both the legacy linter (`if node.body:`) and Ruff (`RUF067` in both default and strict modes) allow comments and blank lines in `__init__.py`. Moreover, Omni's `# omni:ignore` or `RequireExplanation` comments must be valid in `__init__.py`. In Tree-sitter Python, `comment` nodes are `is_extra()` trivia and can be skipped cleanly.
- **D5 — Granularity of Reporting**: **Report each top-level statement in `module` (rather than only `node.body[0]`)**
  - *Rationale*:
    1. **Matches Ruff `RUF067`**: Ruff iterates `for stmt in body` and emits a diagnostic on each disallowed top-level statement.
    2. **Required by Omni's `assert_every_occurrence_reported` test invariant (`src/test_utils.rs:146-188`)**: The test harness automatically verifies every `fail` case by running the rule on `doubled = format!("{code}\n{code}")` and asserting that every diagnostic span from the first half is also reported in the second half (`[span, span + offset]`). If a rule only reports the *first* statement in the file (`node.body[0]`, like the legacy linter), `doubled` still emits only 1 diagnostic instead of 2, causing `assert_every_occurrence_reported` to panic! Reporting per top-level statement satisfies `assert_every_occurrence_reported` naturally and gives users exact line-by-line diagnostics when cleaning up an `__init__.py`.
- **D6 — Diagnostic Message & Suggestion**:
  - `message`: `"{stmt} in `__init__.py` executes on every package import and couples submodules"` (or `"__init__.py contains {stmt}, which executes on package import"`)
  - `suggestion`: `"Move {stmt} to a dedicated submodule and import from that submodule directly, or remove it to keep `__init__.py` empty"`
  - *Rationale*: Complies with `tests/registry.rs`:
    - Uses existing approved placeholder `{stmt}` from `PLACEHOLDERS` (`src/rule_declaration/documentation.rs:15`).
    - Starts `suggestion` with approved verb `"Move"` from `SUGGESTION_VERBS` (`src/rule_declaration/documentation.rs:43`).
    - Explains *why* (executes on package import / couples submodules) and *how to fix* without banned phrases (`consider`, `try to`, `should`).
- **D7 — Taxonomy & Tags**:
  - **Primary `RuleTopic`**: `RuleTopic::ArchitectureAndBoundaries` (module boundaries, import graph hygiene) or `RuleTopic::IdiomaticSimplification` (following `docs/dev/tag_guide.md`).
  - **Tags**:
    - `RuleTopic::ArchitectureAndBoundaries`
    - `Origin::the legacy linter` (and `Origin::Ruff` given direct alignment with `RUF067` `strictly-empty-init-modules`)
    - `Utility::AgentGuardrail` (AI agents frequently dump helper classes, re-exports, or ` __all__` into `__init__.py` out of habit)
    - `Priority::Medium`
    - `Performance::UltraFast` (only inspects top-level children of root `module` node in `__init__.py` files; immediate path bailout for 99% of files)
    - `Precision::ZeroFp`
    - `FixSafety::ContextualRefactor` (moving exports or definitions out of `__init__.py` requires updating caller import paths across the codebase)
- **D8 — Test Harness Filename Support (`src/test_utils.rs`)**:
  - *Observation*: `src/test_utils.rs` currently hardcodes `dummy_filename(SupportLang::Python) -> "test.py"`, which is used by both `rule_test!` and `assert_documented_examples` (`tests/registry.rs`).
  - *Requirement*: If `non-empty-init` checks `ctx.file.file_name() == "__init__.py"`, then `rule_test!` and `assert_documented_examples` would pass `"test.py"` and get 0 diagnostics on `fail` examples unless `CodeRule` can specify its default test filename (or `test_utils.rs` / `rule_test!` supports it). We detail two minimal, surgical solutions in `02_references.md` §3.4.
- **D9 — Enforcement Mode (`Ban` vs `RequireExplanation`)**:
  - See Open Question `Q3` below.

---

### 3.2 Open Questions for Alignment (`Q1`–`Q4`)

- **Q1 — Strict Empty vs. Facade-Permissive vs. Configurable Mode?**
  - **Option A (Strict Empty — the legacy linter & Ruff `RUF067` with `strictly-empty-init-modules = true`)**: Flag *all* statements in `__init__.py` (including imports, `__all__`, and logic; see Q2 for docstrings). Forces explicit submodule imports everywhere.
  - **Option B (Facade-Permissive — Ruff `RUF067` default / `WPS412`)**: Allow re-exports (`import` / `from ... import`), `__all__`, `if TYPE_CHECKING:`, and docstrings; flag only executable logic, function/class definitions (except PEP 562 `__getattr__`/`__dir__`), and non-dunder assignments.
  - **Option C (Strict by default, or `EnforcementMode::RequireExplanation`)**: Flag all statements by default (matching the legacy linter), or use `RequireExplanation` so intentional public facade `__init__.py` files can be justified with a comment.
  - *Recommendation*: **Option A (Strict Empty)** as the core detection rule (preserving the legacy linter's exact architectural guarantee against barrel files and import cycles), paired with a clear decision on Q2 (docstrings) and Q3 (`EnforcementMode`).
- **Q2 — Should a standalone module docstring (`"""Package docstring."""`) in `__init__.py` be exempted or flagged?**
  - **Strict (the legacy linter / Ruff `RUF067` strict mode)**: Flagged (`node.body` is non-empty). If someone wants to document the package in `__init__.py`, they use `#` comments.
  - **Docstring-exempt (`WPS412` / `D104`-compatible)**: Exempt a single leading string literal expression (`expression_statement` -> `string`) at the start of `module` because module docstrings have zero import-cycle or transitive-import risk and are required by `pydocstyle` `D104`.
  - *Recommendation*: Confirm whether strict the legacy linter parity (flagging even module docstrings) or exempting a leading module docstring is preferred.
- **Q3 — Should `non-empty-init` use `EnforcementMode::Ban` or `EnforcementMode::RequireExplanation`?**
  - **`EnforcementMode::Ban`**: Never allows code in `__init__.py` via ordinary comments; requires `# omni:ignore[non-empty-init] -- reason` (or config exclusion) for intentional library facades.
  - **`EnforcementMode::RequireExplanation`**: Allows an explanatory comment above a statement (e.g., `# Re-export public API for external package consumers`) to excuse it, matching Omni's philosophy on `type-cast`, `suppressed-exception`, and `mutable-collection-*`. However, note a subtlety: in `RequireExplanation` mode, *any* comment immediately preceding a statement in `__init__.py` (such as a section header `# Public API`) would automatically excuse that statement!
  - *Recommendation*: **`EnforcementMode::Ban`** is safer and cleaner here because section/header comments (`# Exports`, `# Constants`) are very common above imports/assignments in `__init__.py` and would silently suppress the lint under `RequireExplanation`.
- **Q4 — Rule Name Preference: `non-empty-init` vs `code-in-init`?**
  - Both obey `docs/dev/naming_and_message_style_guide.md`. `non-empty-init` mirrors Ruff's `non-empty-init-module` (`RUF067`); `code-in-init` is slightly more natural if module docstrings are exempted.
