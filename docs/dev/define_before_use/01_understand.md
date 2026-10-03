# Phase 1: Understand — `DefineBeforeUseRule` (`use-before-definition`)

This document records **Phase 1 (Understand)** for the legacy linter's `DefineBeforeUseRule` ([legacy_lints.py:L3049-L3133](../../../scratch/legacy_reference/legacy_lints.py#L3049-L3133)).

> Status: **DRAFT — Phase 1 & 2 Research Complete** (2026-10-03).

---

## 1. What the legacy linter's `DefineBeforeUseRule` Checks

In [legacy_lints.py:L3049-L3133](../../../scratch/legacy_reference/legacy_lints.py#L3049-L3133), `DefineBeforeUseRule` is a warning-level AST rule (`_WARNING_RULES`) that inspects `ast.Module` and `ast.ClassDef` nodes. For each scope (`node.body`), it first builds a map of direct function/method definitions to their first definition line:

```python
func_defs: dict[str, int] = {}
for stmt in node.body:
    if isinstance(stmt, (ast.FunctionDef, ast.AsyncFunctionDef)):
        func_defs.setdefault(stmt.name, stmt.lineno)
```

It then performs two distinct checks over `node.body`:

### 1.1 Sub-Check A: Top-Level Module Statements (`_check_top_level`, L3070–3079)
- **Trigger**: Any statement `stmt` directly in `ast.Module.body` that is **not** a `FunctionDef`, `AsyncFunctionDef`, or `ClassDef`.
- **Mechanism**: Walks `ast.walk(stmt)` and flags any `ast.Name` node whose `sub.id` is in `func_defs` with `func_defs[sub.id] > stmt.lineno`.
- **Message**: `"Function '{sub.id}' (defined at line {func_defs[sub.id]}) is used before its definition."`
- **What this actually represents**:
  - When a top-level statement executes at module import time (for example, `RESULT = compute()` or `HANDLERS = [handle]` before `def compute()` / `def handle()`), Python has not yet executed the `def` statement, raising a **runtime `NameError: name '...' is not defined`**.
  - This runtime error is **already detected by Ruff `F821` (`undefined-name`)**, Pyflakes, Pylint `E0601` (`used-before-assignment`) / `E0602` (`undefined-variable`), and every Python type checker (`ty`, `pyright`, `mypy`).

### 1.2 Sub-Check B: Intra-Scope Function & Method Ordering (`_check_function`, L3081–3133)
- **Trigger**: Any `FunctionDef` or `AsyncFunctionDef` `stmt` directly in `ast.Module.body` (`is_class=False`) or `ast.ClassDef.body` (`is_class=True`).
- **Mechanism**:
  1. Collects a partial set of local variables (`_collect_local_vars`): parameter names (`args`, `kwonlyargs`, `posonlyargs`, `vararg`, `kwarg`) plus direct `ast.Name` targets of `ast.Assign` and `ast.AnnAssign`.
  2. Walks `ast.walk(stmt)` and extracts a `target` name via `_extract_target(sub, is_class=is_class)`:
     - When `is_class=False` (module function): matches **any** `ast.Name` node (`sub.id`).
     - When `is_class=True` (class method): matches **only** `ast.Attribute` nodes whose receiver is `ast.Name` with `id in {"self", "cls"}`, returning `sub.attr`.
  3. If `target in func_defs`, `target not in local_vars`, `target != stmt.name` (direct self-recursion), and `func_defs[target] > stmt.lineno`, reports at most once per `(stmt, target)` pair:
     `"Function/method '{target}' (defined at line {func_defs[target]}) is used in '{stmt.name}' before its definition."`

---

## 2. Defects & False Positives in the legacy linter's Implementation

The legacy linter's 85-line implementation suffers from **seven structural defects** that cause false positives on valid, idiomatic Python or conflate unrelated antipatterns:

| # | Defect | Concrete Failure / False Positive |
| :--- | :--- | :--- |
| **F1** | **Conflates runtime `NameError` (`_check_top_level`) with stylistic declaration order (`_check_function`)** | Violates [rule_design_guide.md](../rule_design_guide.md) §1 (*The Split Test*). Top-level use before `def` is an unopinionated runtime `NameError` (`ImpactedQuality::Reliability`, already covered by Ruff `F821`), whereas calling a sibling function defined below inside a `def` body is 100% valid at runtime and purely concerns reading order (`ImpactedQuality::Maintainability`, `Consensus::Opinionated`). |
| **F2** | **Mutual recursion (`a()` $\leftrightarrow$ `b()`) is impossible to satisfy** | Only direct self-recursion (`target == stmt.name`) is exempted. When `a()` calls `b()` and `b()` calls `a()` (or in a 3-cycle `a -> b -> c -> a`, common in recursive-descent parsers and AST walkers), **one function must be defined before the other**. Reordering `a` and `b` merely moves the warning from `a` to `b`; the rule cannot be satisfied without suppressing it. |
| **F3** | **All `ast.Name` nodes matched indiscriminately (not just calls or `Load` expressions)** | In module scope (`is_class=False`) and `_check_top_level`, `isinstance(sub, ast.Name)` matches every `ast.Name` regardless of context:<br>• **Top-level store targets**: `helper = None` or `for helper in items:` before `def helper():` is flagged as using `helper` before its definition.<br>• **Type annotations & return types**: `def f(x: helper) -> MyResult:` walks `stmt.args` and `stmt.returns`, flagging annotations even under `from __future__ import annotations`.<br>• **Default arguments & decorators**: walked as part of `ast.walk(stmt)` with the same message as body calls. |
| **F4** | **Incomplete local binding collection (`_collect_local_vars`) causes false shadowing hits** | `_collect_local_vars` only checks outer `stmt.args` and flat `Assign`/`AnnAssign` `ast.Name` targets. It misses:<br>• **Unpacking assignments**: `first, helper = pair` (`ast.Tuple` / `ast.List` target)<br>• **Loops & comprehensions**: `for helper in items:` and `[helper for helper in items]`<br>• **Context managers & exception handlers**: `with open(...) as helper:` and `except Err as helper:`<br>• **Walrus operator**: `if (helper := get()):`<br>• **`match`/`case` bindings**: `case [helper, *rest]:`<br>• **Nested `def` / `lambda` parameters**: `map(lambda helper: helper(), items)` inside a function flags the lambda parameter `helper` if a module-level `def helper()` exists below!<br>• **Function-local imports**: `from pkg import helper`. |
| **F5** | **`self.<attr>` in classes flags instance attributes & properties, and ignores `@staticmethod`** | In `is_class=True`, `_extract_target` matches **every** `self.<attr>` and `cls.<attr>` `ast.Attribute` node (even `Store` targets!):<br>• If `__init__` assigns `self.value = 0` and the class later defines `@property def value(self):` (or a method `def value(self):`), `__init__` is flagged for "using method `value` before its definition".<br>• Hardcodes `{"self", "cls"}` instead of checking the method's actual receiver parameter (e.g., a `@staticmethod` has no `self`/`cls`, and calling `ClassName.helper()` is missed). |
| **F6** | **Class methods calling module-level functions defined below are completely ignored** | When `is_class=True`, `func_defs` only contains methods of that class and `_extract_target` returns `None` for all `ast.Name` nodes. A class method at line 20 calling a module-level function `helper()` defined at line 200 is silently ignored. |
| **F7** | **Fights standard Python class layout (`__init__` and public methods first)** | In Python (PEP 8, Google Python Style Guide, `wemake-python-styleguide` `WPS338`), classes place `__init__` / `__new__` / `__post_init__` first, followed by public methods, followed by `_private` helper methods. If `__init__` calls `self._validate()`, the legacy linter forces `def _validate(self)` to be placed **above `__init__`**, inverting idiomatic Python class structure. |

---

## 3. Does This Rule Add Value Beyond Ruff `F821`?

### 3.1 What Ruff `F821` Already Covers (Better and With Zero False Positives)
- **Top-level runtime `NameError`s**: Ruff `F821` (`undefined-name`) tracks Python's exact execution model at module import time (top-level statements, class body statements, decorators, default parameter values, and `from __future__ import annotations`).
- **Conclusion on `_check_top_level`**: the legacy linter's `_check_top_level` adds **zero value** beyond Ruff `F821` while introducing false positives on top-level assignments and deferred annotations. **Top-level statement checking should not be ported.**

### 3.2 What Ruff `F821` Does *Not* Cover: Declaration Order (Stepdown vs. Bottom-Up)
Because Python function bodies are evaluated when called rather than when defined, Ruff `F821` intentionally allows functions and methods within a module or class to be declared in any order.

In software engineering, there are **two competing, equally coherent conventions** for ordering functions within a file:

1. **Bottom-Up ("Define-Before-Use" / Callee-Before-Caller — the legacy linter, C/Pascal tradition)**:
   - Low-level leaf helpers are defined first; higher-level callers and `main()` are defined after the helpers they call (culminating in `if __name__ == "__main__": main()` at the bottom of the file).
   - **Advantage**: A reader scanning sequentially from line 1 downward has already seen the signature and implementation of every helper before encountering a call to it.
2. **Top-Down ("The Stepdown Rule" / Newspaper Metaphor — Robert C. Martin's *Clean Code*, standard Python class layout)**:
   - High-level entrypoints (`main()`, public API functions, `__init__`, public class methods) appear at the **top** of the module or class, and private implementation helpers (`_helper()`) appear **below** the functions that call them.
   - **Advantage**: A reader opening a file or class immediately sees its public contract and high-level workflow at the top without scrolling past dozens of low-level private utilities.

### 3.3 Value Assessment & Tradeoff
- **Is declaration order opinionated?** **Yes (`Consensus::Opinionated`)**. By [tag_guide.md](../tag_guide.md) §2.3 (*"name one ordinary situation where the flagged code is correct and appropriate"*), top-down / Stepdown ordering is ordinary, idiomatic, and often preferred—especially inside classes (`__init__` before `self._helper()`) and in modules that place their public API at the top (including Omni's own [runner.rs](../../../src/code_lint/runner.rs), where `pub fn run_code_lint` at L185 precedes private helpers `collect_targets` at L197 and `lint_single_file` at L274).
- **When does a `use-before-definition` rule add genuine value?**
  - For Python codebases that standardize on **bottom-up (callee-before-caller) ordering** (like the legacy linter, where scripts and modules culminate in public entrypoints / `main()` at the bottom), no rule in Ruff, Pylint, or Flake8 enforces callee-before-caller declaration order.
  - However, to be high-signal and usable without false positives, the rule **must fix all seven defects (F1–F7)** of the legacy linter's implementation.

---

## 4. Clean, High-Signal Rule Design (If Ported to Omni)

### 4.1 Rule Principle
> **Within a module or class scope, a helper function or method should be defined before the sibling functions or methods that call it, unless the functions are mutually recursive.**

A call expression inside a function or method `caller` is flagged when all four conditions hold:
1. **Intra-scope sibling call (`C1`)**:
   - In a **module**, `caller` is a module-level function (`def` / `async def`) and the call expression invokes a bare identifier `callee(...)` that resolves to a sibling module-level function defined at a later line in the same module.
   - In a **class**, `caller` is a method (`def` / `async def`) with a receiver parameter (`self` or `cls`, i.e., not a `@staticmethod`) and the call expression invokes `<receiver>.callee(...)` where `callee` is a sibling method defined at a later line in the same class.
2. **Call-site only, not attribute/variable/type access (`C2`)**:
   - Only actual **call expressions** (`callee(...)` or `<receiver>.callee(...)`)—or optionally value references outside annotations/assignments, though restricting to calls `call-before-definition` is sharpest—are inspected. Attribute assignments (`self.x = 1`), bare property reads (`self.x`), type annotations (`-> MyType`), and keyword argument names (`f(helper=1)`) are never flagged.
3. **Not locally bound or shadowed (`C3`)**:
   - For module-level calls `callee(...)`, `callee` is not bound locally in `caller` (or in an enclosing nested `def`/`lambda`/comprehension scope) via parameters, assignments (`=`, `:=`, augmented `+=`), tuple/list unpacking, `for` targets, `with ... as` targets, `except ... as` bindings, `match`/`case` capture patterns, or local `def`/`class`/`import` statements.
4. **Not mutually recursive (`C4`)**:
   - `caller` and `callee` do not belong to the same cycle (Strongly Connected Component) in the scope's intra-scope call graph (exempting both direct recursion `f -> f` and mutual recursion `a <-> b` or `a -> b -> c -> a`).

### 4.2 Numbered Design Decisions

| ID | Decision | Rationale |
| :--- | :--- | :--- |
| **D1** | **Rule name**: `call-before-definition` (or `use-before-definition`). File `src/code_lint/rules/call_before_definition.rs`, const `RULE`. | Follows [naming_and_message_style_guide.md](../naming_and_message_style_guide.md) §1: names the flagged pattern (not the policy `define-before-use`), kebab-case, 3 words, no polarity prefix. |
| **D2** | **Language & target scope**: `SupportLang::Python` only, `RuleTarget::SourceOnly`. | The legacy linter rule is Python-only; Rust idioms (and Omni's own `runner.rs`, `ast/python.rs`, `ast/rust.rs`) place `pub fn` above private helpers. `SourceOnly` avoids churning test files where `test_*` functions precede bottom-of-file test helpers. |
| **D3** | **Drop `_check_top_level` (defer to Ruff `F821`)**. | Top-level use before definition is a runtime `NameError` already caught by Ruff `F821` and type checkers; keeping it violates the Split Test ([rule_design_guide.md](../rule_design_guide.md) §1). |
| **D4** | **Classification**: `topics: &[Topic::DECLARATION_ORDER]` (new root topic in `taxonomy.rs`), `Precision::Heuristic`, `Consensus::Opinionated`, `ImpactedQuality::Maintainability`. | Ordering is stylistic (`Maintainability`), top-down Stepdown order is a valid competing convention (`Opinionated`), and syntactic name/receiver matching without cross-module type resolution is a proxy (`Heuristic`). |
| **D5** | **Mutual recursion exemption via Strongly Connected Components (SCCs)**. | Build the directed call graph among sibling functions/methods in each scope. If `callee` can reach `caller` (same SCC), exempt the call—making mutual recursion (`a <-> b`, `a -> b -> c -> a`) pass with zero false positives. |
| **D6** | **Constructor exemption (`__init__`, `__new__`, `__post_init__`) in classes**. | In Python, `__init__` / `__new__` / `__post_init__` are universally placed at the top of a class (PEP 8, `WPS338`). Exempting constructor methods from having to sit below `self._helper()` methods avoids forcing private helpers above `__init__`. |
| **D7** | **Overload & property grouping + `rule_test!` epoch handling**. | Group `@overload` stubs with their implementation and `@property` with `@<name>.setter` / `@<name>.deleter`. Start a new declaration epoch only if a non-`@overload`, non-accessor `def` redefines an already-seen name in the same scope, so `rule_test!`'s `format!("{code}\n{code}")` repetition check works out of the box with `RepeatCheck::SameCode`. |
| **D8** | **Report at most once per `(caller, callee)` pair at the first forward call site (`node` = the `call` or callee node)**. | Matches the legacy linter's deduplication (`reported.add(target)`) so a function calling the same later helper 3 times emits 1 actionable diagnostic ("move `callee` above `caller`") rather than 3 redundant ones. |

---

## 5. Open Design Questions to Confirm Before Phase 3

1. **Q1 (Adoption vs. Rejection / Opt-In)**: Given that top-level `NameError`s are covered by Ruff `F821` and intra-scope declaration order is opinionated (bottom-up vs. *Clean Code*'s top-down Stepdown Rule), should Omni:
   - **(Option A — Recommended if full the legacy linter parity is desired)**: Implement `call-before-definition` for Python (`SupportLang::Python`, `RuleTarget::SourceOnly`, `Consensus::Opinionated`) with the clean design above (SCC mutual-recursion exemption, constructor exemption, call-only matching, full local-binding awareness)?
   - **(Option B)**: Reject/retire `DefineBeforeUseRule` from Omni's rule catalog on the grounds that Ruff `F821` covers runtime `NameError`s and declaration order is too stylistic/opinionated?
2. **Q2 (Class Methods vs. Module Functions)**: If implemented (Option A), should class bodies:
   - Check non-constructor methods calling `self.helper()` / `cls.helper()` (exempting `__init__`, `__new__`, `__post_init__`, **D6**), or
   - Check module-level functions only (since `wemake-python-styleguide` `WPS338` and PEP 8 conventions order class methods by visibility—public before `_private`—rather than call order)?
3. **Q3 (Calls Only vs. All Value References)**: Should the rule inspect only **call sites** (`helper(...)`, `self.helper(...)`) or also **first-class function value references** passed as arguments/callbacks (`register(helper)`, ` map(self.helper, items)`) in `Load` position outside type annotations?
