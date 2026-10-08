# Phase 3: Design Plan — Declaration Ordering Rule Suite

This document records **Phase 3 (Design Plan)** for the **Declaration Ordering** rule suite (`Topic::DECLARATION_ORDER`) in [ROADMAP.md](../../../ROADMAP.md), building on the first-principles derivations and 5-corpus empirical measurements in [01_understand.md](01_understand.md) and the cross-ecosystem reference survey in [02_references.md](02_references.md).

> Status: **VALIDATED — Ready for Phase 4 Implementation** (2026-10-08).

---

## 1. Summary of Validated Decisions (`D1`–`D6`)

| ID | Decision | Rationale |
| :--- | :--- | :--- |
| **`D1`** | **Retire `call-before-definition`** (`src/code_lint/rules/call_before_definition.rs`, `collect_function_scopes` in `src/code_lint/ast/python/scopes.rs`, and `docs/dev/define_before_use/`). | Directly contradicts `private-before-public-method` on `99.4%` (`163 / 164`) of multi-visibility Python classes with intra-class calls in CPython, and fires on `66.1%` of CPython files (`1,729` violations) and `56 / 84` (`66.7%`) of Omni's own Rust files. |
| **`D2`** | **Implement 5 targeted, orthogonal declaration-ordering rules** under `Topic::DECLARATION_ORDER`: `constructor-after-method` (Python, Rust), `private-before-public-method` (Python, Rust), `field-after-method` (Python), `associated-item-after-method` (Rust), and `statement-after-main-guard` (Python). | Each rule targets a single structural boundary with an orthogonal failure mode and a deterministic, conflict-free fix. |
| **`D3`** | **Reject global module-level kind/visibility bucketing** (and defer Python import-order `E402` to Ruff). | Global module-level `pub fn` before `fn` or `struct` before `fn` breaks feature-cluster cohesion (`56 / 84` Omni files, `1,326` CPython classes). |
| **`D4`** | **Enforce 2-tier visibility in `private-before-public-method`**: Exported (`pub`, `pub(crate)`, `pub(super)`, `pub(in ...)`) before Private (bare `fn`) in Rust inherent `impl` blocks; Public & dunder (`name`, `__dunder__`) before Private (`_name`, `__mangled`) in Python classes. | A 3-tier split (`pub` > `pub(crate)` > `fn`) falsely flags the idiomatic Rust restricted-constructor / public-accessor pattern (`AstNode::from_span`, `ripgrep::Error::new`, `cargo::Artifact::parse`), whereas a 2-tier split preserves it and achieves `90.9%`–`91.0%` compliance across Omni and ripgrep. |
| **`D5`** | **Uncomment `declaration-order` in [tag_guide.md](../tag_guide.md)** and register all 5 rules in `CODE_RULES` (`src/code_lint/rules.rs`). | Activates `Topic::DECLARATION_ORDER` in the rule taxonomy and CLI `--list-rules` output. |
| **`D6`** | **Reorder the 3 misplaced private methods in Omni's own `src/`** (`LiteralValue::float` in `src/code_lint/ast.rs`, `SuppressionTracker::parse_comment_text` in `src/code_lint/suppression.rs`, and `InterceptedCommand::parse_single` in `src/command_lint/command.rs`). | Ensures `test_self_dogfooding_code_lint` passes with zero suppressions when all 5 rules are enabled. |

### Formal Consistency Invariant Between `constructor-after-method` and `private-before-public-method`
To guarantee that `constructor-after-method` (which moves constructors *above* non-constructor methods) and `private-before-public-method` (which moves private methods *below* public/exported methods) can **never** conflict on any method $m$:
$$\text{is\_constructor}(m) \implies \text{visibility}(m) = \text{Public}$$
- **In Python**: Every lifecycle constructor (`__prepare__`, `__init_subclass__`, `__new__`, `__init__`, `__post_init__`, `__attrs_pre_init__`, `__attrs_post_init__`) is a `__dunder__` method, which belongs to `MethodVisibility::Public`.
- **In Rust**: A constructor in an inherent `impl` block must carry an explicit visibility qualifier (`pub`, `pub(crate)`, `pub(super)`, or `pub(in ...)`), so it always belongs to `MethodVisibility::Public`. A bare private `fn new_helper()` without `pub` is classified as a private non-constructor helper and belongs in the private section below exported methods.

---

## 2. Shared AST Helper Inventory

Following `src/architecture.rs` (`CodeLintRules` depends on `CodeLintContract`, `CodeLintPolicy`, `RuleDeclaration`, and transitively `CodeLintAst`), all AST traversal lives in `src/code_lint/ast/`:

| Helper | Location | Used By | Purpose |
| :--- | :--- | :--- | :--- |
| `MethodVisibility`, `TypeMethod<'a>`, `TypeMethodScope<'a>`, `collect_type_method_scopes(file)` | `src/code_lint/ast.rs` (dispatching to `ast::python::collect_type_method_scopes` in `src/code_lint/ast/python/classes.rs` and `ast::rust::collect_type_method_scopes` in `src/code_lint/ast/rust.rs`) | `constructor-after-method`, `private-before-public-method` | Extracts ordered direct methods per Python `class` body and Rust inherent `impl` block, classifying each method's `MethodVisibility` (`Public` vs. `Private`) and `is_constructor: bool`. Groups Python `@overload` series and `@<prop>.setter` / `@<prop>.deleter` accessors with their primary definition. |
| `PythonFieldAfterMethod<'a>`, `collect_fields_after_methods(file)` | `src/code_lint/ast/python/classes.rs` (re-exported by `src/code_lint/ast/python.rs`) | `field-after-method` | Walks each Python `Stmt::ClassDef` body in source order and collects direct `Stmt::AnnAssign` statements with `Expr::Name` targets that appear after at least one direct `Stmt::FunctionDef`. |
| `RustAssociatedItemAfterMethod<'a>`, `collect_associated_items_after_methods(file)` | `src/code_lint/ast/rust.rs` | `associated-item-after-method` | Walks each Rust `ast::Impl` (inherent or trait) and `ast::Trait` `AssocItemList` in source order and collects `ast::AssocItem::TypeAlias` and `ast::AssocItem::Const` items that appear after at least one `ast::AssocItem::Fn`. |
| `collect_statements_after_main_guard(file)` | `src/code_lint/ast/python.rs` | `statement-after-main-guard` | Scans `module.body` for a top-level `if __name__ == "__main__":` (or `if "__main__" == __name__:`) statement and returns every subsequent top-level `Stmt` as an `AstNode`. |

---

## 3. Per-Rule Specifications

### 3.1 `constructor-after-method` (Python, Rust)
- **File**: `src/code_lint/rules/constructor_after_method.rs`
- **Target**: `RuleTarget::SourceOnly`
- **Languages**: `&[Language::Python, Language::Rust]`
- **Options**: `RuleOptions::code_rule(())`
- **Classification**:
  - `topics`: `&[Topic::DECLARATION_ORDER]`
  - `precision`: `Precision::Exact`
  - `consensus`: `Consensus::Unopinionated`
  - `impacted_quality`: `ImpactedQuality::Maintainability`
- **`ViolationTemplate`**:
  - `summary`: `"Constructor `{function}` of `{class}` is defined after a non-constructor method."`
  - `rationale`: `"Burying initialization logic below regular methods forces readers to scan the body of `{class}` to find how instances are constructed."`
  - `suggestion`:
    - `base`: `"Move `{function}` above all non-constructor methods in `{class}`."`
    - `Python =>` `"Move `{function}` to the top of `{class}`, before any regular methods."`
    - `Rust =>` `"Move `{function}` to the top of the `impl {class}` block, before any methods."`
- **`RuleDoc`**:
  - `summary`: `"Flags class and inherent `impl` constructors defined after regular methods."`
- **Diagnostic Anchor**: `&method.name_node` with placeholders `[("function", &method.name), ("class", &scope.type_name)]`.
- **Named Exemptions**:
  - `E1` (**Multiple lifecycle constructors in any order**): Python `__prepare__`, `__init_subclass__`, `__new__`, `__init__`, `__post_init__`, `__attrs_pre_init__`, `__attrs_post_init__` and Rust exported `new`, `try_new`, `new_*`, `try_new_*` associated functions do not set `seen_non_constructor`, so multiple constructors at the top of a type never flag one another.
  - `E2` (**Python `@overload` constructor signatures**): `@overload` stubs for `__init__` or `__new__` are grouped with their implementation at the first declaration position.
  - `E3` (**Rust trait `impl` blocks**): `impl Trait for Type` blocks are not checked (`impl_item.trait_().is_none()`), as trait method order follows the trait definition.
  - `E4` (**Rust private `fn new*` helpers and associated functions returning non-`Self`**): Bare `fn new_helper()` without a `pub` visibility qualifier and associated functions whose return type does not reference `Self` or the enclosing type (`pub fn new_request_id() -> u64`) are not constructors of `Self`.
  - `E5` (**Rust methods with a `self` receiver named `new_*`**): Methods taking `self`, `&self`, or `&mut self` (such as `pub fn new_child(&self)`) are instance methods, not type constructors (`self_param().is_none()`).
  - `E6` (**Rust `#[test]` / `#[cfg(test)]` functions in `impl` blocks**): Test functions inside an `impl` block are ignored.
  - `E7` (**Independent scopes for nested classes / multiple `impl` blocks**): Each Python `class` and each Rust inherent `impl` block tracks constructor ordering independently.

---

### 3.2 `private-before-public-method` (Python, Rust)
- **File**: `src/code_lint/rules/private_before_public_method.rs`
- **Target**: `RuleTarget::SourceOnly`
- **Languages**: `&[Language::Python, Language::Rust]`
- **Options**: `RuleOptions::code_rule(())`
- **Classification**:
  - `topics`: `&[Topic::DECLARATION_ORDER]`
  - `precision`: `Precision::Exact`
  - `consensus`: `Consensus::Opinionated`
  - `impacted_quality`: `ImpactedQuality::Maintainability`
- **`ViolationTemplate`**:
  - `summary`: `"Private method `{function}` of `{class}` is defined before a public method."`
  - `rationale`: `"Placing internal helpers above public methods buries the external interface of `{class}` behind implementation details."`
  - `suggestion`:
    - `base`: `"Move `{function}` below all public methods in `{class}`."`
    - `Python =>` `"Move `{function}` below all public and dunder methods in `{class}`."`
    - `Rust =>` `"Move `{function}` below all `pub` methods in the `impl {class}` block."`
- **`RuleDoc`**:
  - `summary`: `"Flags private helper methods defined before public methods in a class or inherent `impl` block."`
- **Diagnostic Anchor**: `&method.name_node` of each `MethodVisibility::Private` method that precedes at least one `MethodVisibility::Public` method in the same `TypeMethodScope`, with placeholders `[("function", &method.name), ("class", &scope.type_name)]`.
- **Named Exemptions**:
  - `E1` (**Python dunder methods `__name__` are public/contract tier**): Special methods starting and ending with `__` (`len > 4`) are treated as `MethodVisibility::Public`, so `_helper` after `__init__` and `__repr__` is valid, while `_helper` before `__repr__` or `run` is flagged.
  - `E2` (**Python `@overload` and `@<prop>.getter` / `@<prop>.setter` / `@<prop>.deleter` grouping**): Overloaded signatures and property getter/setter/deleter methods (including overloaded setters) are grouped with their primary definition at its first declaration position, avoiding duplicate findings or false splits.
  - `E3` (**Rust restricted visibility `pub(crate)` / `pub(super)` / `pub(in ...)` is exported tier**): Any `fn` with `visibility().is_some()` is treated as `MethodVisibility::Public` (2-tier model `D4`), so `pub(crate) fn from_span` before `pub fn span` is not flagged.
  - `E4` (**Rust trait `impl` blocks**): `impl Trait for Type` blocks are not checked.
  - `E5` (**Rust `#[test]` / `#[cfg(test)]` functions in `impl` blocks**): Test functions inside an `impl` block are ignored.
  - `E6` (**Independent scopes for nested classes / multiple `impl` blocks**): Each Python `class` and each Rust inherent `impl` block is checked independently; module-level free functions are not checked.

---

### 3.3 `field-after-method` (Python)
- **File**: `src/code_lint/rules/field_after_method.rs`
- **Target**: `RuleTarget::SourceOnly`
- **Languages**: `&[Language::Python]`
- **Options**: `RuleOptions::code_rule(())`
- **Classification**:
  - `topics`: `&[Topic::DECLARATION_ORDER]`
  - `precision`: `Precision::Exact`
  - `consensus`: `Consensus::Unopinionated`
  - `impacted_quality`: `ImpactedQuality::Maintainability`
- **`ViolationTemplate`**:
  - `summary`: `"Attribute `{name}` of `{class}` is declared after a method definition."`
  - `rationale`: `"Declaring class or instance attributes after methods scatters the data layout of `{class}` across its body and obscures the synthesized `__init__` parameter order in `@dataclass`, `attrs`, and `NamedTuple` classes."`
  - `suggestion`: `"Move the `{name}` attribute declaration to the top of `{class}`, before any method definitions."`
- **`RuleDoc`**:
  - `summary`: `"Flags type-annotated class and instance attributes declared after methods in a Python class body."`
- **Diagnostic Anchor**: `&field.node` (the `Stmt::AnnAssign` node) with placeholders `[("name", &field.name), ("class", &field.class_name)]`.
- **Named Exemptions**:
  - `E1` (**Unannotated `Stmt::Assign` after `def`**): Unannotated class-body assignments such as method aliases (`__repr__ = __str__`) and `property(get_x)` bindings must follow the `def` they reference to avoid `NameError`, so only `Stmt::AnnAssign` is checked.
  - `E2` (**Non-identifier `Stmt::AnnAssign` targets**): Subscript or attribute targets (`cls.attr: int = 1`, `items[0]: int = 1`) are not class attribute declarations (`Expr::Name`).
  - `E3` (**Nested classes and method-local annotations**): Each `Stmt::ClassDef` has its own independent method-seen state, and annotated assignments inside method bodies (`x: int = 1` or `self.x: int = 1`) are not class-body statements.

---

### 3.4 `associated-item-after-method` (Rust)
- **File**: `src/code_lint/rules/associated_item_after_method.rs`
- **Target**: `RuleTarget::SourceOnly`
- **Languages**: `&[Language::Rust]`
- **Options**: `RuleOptions::code_rule(())`
- **Classification**:
  - `topics`: `&[Topic::DECLARATION_ORDER]`
  - `precision`: `Precision::Exact`
  - `consensus`: `Consensus::Unopinionated`
  - `impacted_quality`: `ImpactedQuality::Maintainability`
- **`ViolationTemplate`**:
  - `summary`: `"Associated item `{name}` of `{class}` is declared after a `fn` item."`
  - `rationale`: `"Placing associated types or constants after `fn` items hides the type-level parameters and constants that method signatures in `{class}` depend on."`
  - `suggestion`: `"Move `{name}` to the top of the `{class}` `trait` or `impl` block, before any `fn` items."`
- **`RuleDoc`**:
  - `summary`: `"Flags Rust associated `type` and `const` items declared after `fn` items in an `impl` or `trait` block."`
- **Diagnostic Anchor**: `&item.name_node` with placeholders `[("name", &item.name), ("class", &item.container_name)]`.
- **Named Exemptions**:
  - `E1` (**Macro invocations inside `impl` / `trait` blocks**): `ast::AssocItem::MacroCall` items neither set `seen_fn` nor get flagged after a `fn`.
  - `E2` (**Local `const` / `type` items inside a `fn` body**): Items inside a method's `BlockExpr` are not associated items of the enclosing `impl` or `trait`.
  - `E3` (**Independent `impl` and `trait` blocks**): Each `impl` or `trait` block tracks its own `seen_fn` state independently.

---

### 3.5 `statement-after-main-guard` (Python)
- **File**: `src/code_lint/rules/statement_after_main_guard.rs`
- **Target**: `RuleTarget::SourceOnly`
- **Languages**: `&[Language::Python]`
- **Options**: `RuleOptions::code_rule(())`
- **Classification**:
  - `topics`: `&[Topic::DECLARATION_ORDER]`
  - `precision`: `Precision::Exact`
  - `consensus`: `Consensus::Unopinionated`
  - `impacted_quality`: `ImpactedQuality::Reliability`
- **`ViolationTemplate`**:
  - `summary`: `"Top-level statement appears after the `if __name__ == \"__main__\":` guard."`
  - `rationale`: `"Code inside the `__main__` block executes before any declarations below it are bound, so calling a function or referencing a binding defined below the guard raises `NameError` at runtime when the module is run as a script."`
  - `suggestion`: `"Move all module-level definitions above the `if __name__ == \"__main__\":` block, or move script-only cleanup inside the guard."`
- **`RuleDoc`**:
  - `summary`: `"Flags top-level Python statements placed after the `if __name__ == \"__main__\":` guard."`
- **Diagnostic Anchor**: `&statement_node` with placeholders `&[]`.
- **Named Exemptions**:
  - `E1` (**Both comparison orders recognized**): Both `if __name__ == "__main__":` and `if "__main__" == __name__:` act as the main-guard boundary.
  - `E2` (**Statements inside the `if` / `elif` / `else` suite of the main guard and duplicate main guards**): Statements inside the guard's own branches and subsequent `if __name__ == "__main__":` guards are not flagged.
  - `E3` (**Non-guard `if` statements comparing `__name__`**): `if __name__ != "__main__":` or `if __name__ == "pkg.mod":` or multi-comparator chains are not main guards.
  - `E4` (**Nested `if __name__ == "__main__":` inside a function or class**): Only top-level statements in `module.body` are checked.
