# Phase 2: Gather Resources and References — Declaration Ordering

This document records **Phase 2 (Gather Resources and References)** for Declaration Ordering across Python and Rust, building on [01_understand.md](01_understand.md) and [decisions/011_colocated_abstraction_ordering.md](../../../decisions/011_colocated_abstraction_ordering.md).

> Status: **COMPLETE** (2026-10-08).

Confidence markers: ✅ verified against official docs/source and empirical corpus runs this session.

---

## 1. Program Comprehension, Graph Cohesion & Cognitive Science Literature

1. **Soloway, E. & Ehrlich, K. (1984), *"Empirical Studies of Programming Knowledge"*, IEEE Transactions on Software Engineering, SE-10(5), 595–609** ✅
   - Establishes that experienced programmers rely on **programming plans** and **rules of programming discourse** (structural conventions for where initialization, public contracts, and helpers appear).
   - When code violates discourse rules (for example, burying initialization or placing incidental helpers above public entrypoints), expert recall and comprehension drop to the level of novices because top-down hypothesis verification fails.

2. **Pennington, N. (1987) & Von Mayrhauser, A. & Vans, A. M. (1995), *"Program Comprehension During Software Maintenance and Evolution"*, IEEE Computer, 28(8), 44–55** ✅
   - Synthesizes **Top-Down (Domain/Goal-Driven)** and **Bottom-Up (Control-Flow/Chunking)** comprehension into an integrated model: developers orient first via the top-level structural beacons of a class or module (constructors, public signatures, field schema) before tracing into private helper implementations on demand.

3. **Hitz, M. & Montazeri, B. (1995), *"Measuring Coupling and Cohesion in Object-Oriented Systems"* (`LCOM4`), Proc. Int. Symp. Applied Corporate Computing** ✅
   - Models a class or module as a graph whose vertices are functions/methods and whose edges are direct call and attribute-access relationships; connected components represent independent functional clusters inside the scope.
   - Motivates **colocated component units**: when a scope has multiple public entrypoints with disjoint private helper subgraphs, separating an exclusive helper from its owning public entrypoint fractures the component unit across the file.
   - Also reveals why naive undirected `LCOM4` is insufficient when a low-level utility is shared (`|Roots(h)| >= 2`): an undirected connected-components check merges all callers of a shared utility into a single component, whereas **Private-Subgraph Root Ownership (`Roots(h)`)** cleanly distinguishes **Exclusive Helpers (`|Roots(h)| = 1`)** from **Shared Helpers (`|Roots(h)| >= 2`)**.

4. **Busjahn, T. et al. (2015), *"Eye Movements in Code Reading: Relaxing the Linear Order"*, IEEE ICPC 2015** ✅
   - Eye-tracking experiments show that programmers scan structural headers first and jump along call edges from caller to callee.
   - Within a component unit or shared helper layer, top-down (`caller` before `callee`) ordering aligns declaration order with the downward abstraction gradient.

5. **Martin, R. C. (2008), *Clean Code: A Handbook of Agile Software Craftsmanship*, Ch. 5 & Ch. 10 ("The Newspaper Metaphor", "Vertical Distance", "The Stepdown Rule", "Class Organization")** ✅
   - Defines **Vertical Affinity / Vertical Distance** (callers and their exclusive callees belong close together so readers do not scroll across unrelated functions) and **The Stepdown Rule** (code reads top-down from high-level public entrypoints to lower-level private helpers).
   - Highlights the classic tension with strict visibility bucketing (`StyleCop SA1202`): forcing all public functions above all private functions separates `pub_a` from its exclusive helper `_a1` whenever `pub_b` sits between them. Resolving this tension requires distinguishing **exclusive helpers** (`|Roots(h)| = 1`, colocated right below their owner `p`) from **shared helpers** (`|Roots(h)| >= 2`, placed in the trailing shared helper layer).

---

## 2. External Linter State of the Art

### 2.1 SOTA Rules Catalog Across Ecosystems

| Ecosystem / Tool | Rule Name | What It Enforces | Relevance to Omni |
| :--- | :--- | :--- | :--- |
| **`wemake-python-styleguide`** | `WPS338` (`WrongMethodOrderViolation`) ✅ | Orders Python class methods: `__init_subclass__` -> `__new__` -> `__init__` -> `__call__`/`__await__` -> public & magic -> `_protected` -> `__private`. | Precedent for `constructor-after-method` and visibility ordering, but uses flat visibility bucketing that breaks exclusive helper colocation. Unimplemented in Ruff. |
| **`flake8-class-attributes-order`** | `CCE001` (`WrongClassAttributesOrder`) ✅ | Orders Python class body: fields/attributes -> `__new__`/`__init__`/`__post_init__` -> public/magic/property/classmethod/staticmethod -> `_protected` -> `__private`. | Direct precedent for `field-after-method` and `constructor-after-method`. |
| **C# StyleCop Analyzers** | `SA1202` (`ElementsMustBeOrderedByAccess`) ✅ | Enforces strict access bucketing (`public` -> `internal` -> `protected` -> `private`). Frequently disabled by teams following *Clean Code* Vertical Distance because it prevents colocating an exclusive private helper under its public caller. | Demonstrates why `private-before-public-function` must use **Root-Ownership (`Roots(h)`)** rather than naive global access bucketing. |
| **ESLint (`eslint-plugin-the-step-down-rule`)** | `the-step-down-rule/the-step-down-rule` ✅ | Enforces top-down caller-before-callee ordering within a scope, reversing ESLint's `no-use-before-define`. | Precedent for `callee-before-caller`, which Omni scopes to same-visibility, same-cluster edges so it never conflicts with colocation or visibility tiers. |
| **Java Checkstyle** | `DeclarationOrder`, `OverloadMethodsDeclarationOrder`, `VariableDeclarationUsageDistance` ✅ | Enforces static/instance fields -> constructors -> methods (`DeclarationOrder`), contiguous overloads, and bounded declaration-to-usage distance. | Confirms the universal OOP header layout (**Fields -> Constructors -> Methods**) and vertical locality measurement. |
| **Kotlin Detekt / Ruby RuboCop / SwiftLint** | `style:ClassOrdering`, `Layout/ClassStructure`, `type_contents_order` ✅ | Enforce properties/attributes -> initializers -> methods. | Confirm universal structural header ordering. |
| **Rust Clippy** | `clippy::items_after_test_module` (warn), `clippy::arbitrary_source_item_ordering` (restriction) ✅ | `items_after_test_module` flags any item placed after `#[cfg(test)] mod tests`. `arbitrary_source_item_ordering` only sorts `impl` methods alphabetically and has no constructor, colocation, or abstraction-tier support. | `items_after_test_module` covers Rust test footers and inspires Python's `statement-after-main-guard`. Clippy leaves `impl` and module colocation/abstraction ordering uncovered. |
| **Ruff & Mypy / Pyright** | Ruff `F821` (`undefined-name`), `E402` (`module-import-not-at-top-of-file`); Mypy overload/property checks ✅ | `F821` catches import-time `NameError`s (except inside functions called from an early `if __name__ == "__main__":`). Mypy/Pyright reject non-contiguous `@overload` and split `@property` setters. | Explains why `@overload` and `@property` contiguity are already covered by type checkers, while `field-after-method`, `constructor-after-method`, `uncolocated-helper`, `private-before-public-function`, `callee-before-caller`, and `statement-after-main-guard` are uncovered by Ruff. |

---

## 3. Comparison & Disposition Table (`R1`–`R9`)

| ID | Reference / Concept | Disposition | Rationale |
| :--- | :--- | :--- | :--- |
| **R1** | **Constructor-First Ordering (`constructor-after-method`)** — Python (`__prepare__`, `__init_subclass__`, `__new__`, `__init__`, `__post_init__`) & Rust inherent `impl` (`pub`/`pub(...)` `new`, `try_new`, `new_*`, `try_new_*`) ✅ | **Adopt** | `97.5%–100%` compliance across all 5 corpora; uncovered by Ruff and Clippy. |
| **R2** | **Two-Place Helper Placement (`uncolocated-helper`)** — Once below its public callers, a private helper `h` sits either immediately after its single consumer (`Roots(h) = {p}`) or in the trailing helper section at the end of the scope (`pos(h) > last_pub`) ✅ | **Adopt** | Supports both vertical-slice modules (`[pub_a, _a1, pub_b, _b1]`) and public-first scopes (`[pub_a, pub_b, _a1, _b1, _shared]`) while catching helpers stranded between unrelated public functions (`read_config_file` in `rule_selection.rs`). |
| **R3** | **Colocation-Aware Visibility Ordering (`private-before-public-function`)** — Single-use helpers below their owner `p`, shared helpers (`\|Roots(h)\| >= 2`) below all of their public callers (`pos(h) > max(Roots(h))`), unrooted private functions below `last_pub` ✅ | **Adopt** | Eliminates `45%` of naive visibility-bucketing false positives on colocated clusters (`[pub_a, _a1, pub_b, _b1]`) while enforcing the public-before-private abstraction gradient across modules and classes/`impl`s. |
| **R4** | **Private-Helper Top-Down Ordering (`callee-before-caller`)** — Higher-abstraction private callers before lower-abstraction private callees (`Private -> Private`) ✅ | **Adopt** | Replaces bottom-up `call-before-definition`; aligns reading order with the top-down abstraction gradient (`_exclusive -> _shared`) while leaving `Public -> Public` peer entrypoints unconstrained. |
| **R5** | **Annotated Class Attributes Before Methods (`field-after-method`)** — Python (`Stmt::AnnAssign` `x: T` after `def` in a class body) ✅ | **Adopt** | `100.0%` compliance across all 1,591 classes in CPython 3.14.8 (`0` false positives); complements `inline-public-attribute-annotation` and protects `@dataclass` / Pydantic field order. |
| **R6** | **Associated `type` / `const` Before `fn` in Rust `impl` / `trait` (`associated-item-after-method`)** ✅ | **Adopt** | `100%` compliance in Omni `src/`, `98.5%` in ripgrep, `95.4%` in cargo. |
| **R7** | **No Statements After `if __name__ == "__main__":` (`statement-after-main-guard`)** — Python counterpart to Clippy `items_after_test_module` ✅ | **Adopt** | `99.1%` compliance across 219 main-guarded files in CPython; closes a real Ruff `F821` blind spot where `def`/`class` declared below `if __name__ == "__main__": main()` causes a runtime `NameError`. |
| **R8** | **Bottom-Up `call-before-definition`** ✅ | **Retire / Replace with `R4`** | Bottom-up (`callee` before `caller`) inverts the abstraction gradient and contradicts `private-before-public-function` whenever a public function calls a private helper. |
| **R9** | **Split `@overload` & Split `@property`** ✅ | **Reject / Defer** | Already enforced by Mypy and Pyright (`Overloaded function signatures must be next to each other`, `Callable has no attribute "setter"`). |

---

## 4. Internal Codebase Building Blocks

| Need | Existing Building Block | Location |
| :--- | :--- | :--- |
| Architectural component-isolation and layering model | `Architecture` (`Component`, `Layer`, dependency rules) | [decisions/006_architectural_linting.md](../../../decisions/006_architectural_linting.md), [src/architecture.rs](../../../src/architecture.rs) |
| Colocated Component-Unit & Abstraction-Layer Ordering ADR | `ADR 011` (`Roots(h)`, `Cluster(p)`, `P1 -> P2 -> P3` precedence) | [decisions/011_colocated_abstraction_ordering.md](../../../decisions/011_colocated_abstraction_ordering.md) |
| Walking Python modules, classes, direct methods, and `Stmt::AnnAssign` fields | `StmtClassDef`, `Stmt::FunctionDef`, `Stmt::AnnAssign`, `extract_decorators_from_slice` | [src/code_lint/ast/python.rs](../../../src/code_lint/ast/python.rs), [src/code_lint/ast/python/classes.rs](../../../src/code_lint/ast/python/classes.rs) |
| Grouping Python `@overload` stubs & `@property` getter/setter/deleter pairs | `collect_type_method_scopes` (`TypeMethod`, `MethodVisibility`) | [src/code_lint/ast/python/classes.rs](../../../src/code_lint/ast/python/classes.rs) |
| Detecting Python `if __name__ == "__main__":` guards | `Stmt::If` with `Expr::Compare` matching `__name__` and `"__main__"` | [src/code_lint/ast/python.rs](../../../src/code_lint/ast/python.rs) |
| Walking Rust modules, inherent `impl` blocks, and `ast::Fn` visibility/receiver/calls | `ra_ap_syntax::ast::{SourceFile, Module, Impl, AssocItem, Fn, HasVisibility, HasName}` | [src/code_lint/ast/rust.rs](../../../src/code_lint/ast/rust.rs) |
| Taxonomy topic `Topic::DECLARATION_ORDER` | Declared in `src/rule_declaration/taxonomy.rs` and `docs/dev/tag_guide.md` | [src/rule_declaration/taxonomy.rs](../../../src/rule_declaration/taxonomy.rs) |
| Placeholders `{function}`, `{caller}`, `{class}`, `{name}` & verb `Move` | Declared in `PLACEHOLDERS` and `SUGGESTION_VERBS` | [tests/registry.rs](../../../tests/registry.rs), [docs/dev/naming_and_message_style_guide.md](../naming_and_message_style_guide.md) |
