# Phase 2: Gather Resources and References — Declaration Ordering

This document records **Phase 2 (Gather Resources and References)** for Declaration Ordering across Python and Rust, building on [01_understand.md](01_understand.md).

> Status: **COMPLETE** (2026-10-08).

Confidence markers: ✅ verified against official docs/source and empirical corpus runs this session.

---

## 1. Program Comprehension & Cognitive Science Literature

1. **Soloway, E. & Ehrlich, K. (1984), *"Empirical Studies of Programming Knowledge"*, IEEE Transactions on Software Engineering, SE-10(5), 595–609** ✅
   - Establishes that experienced programmers rely on **programming plans** and **rules of programming discourse** (structural conventions for where initialization, public contracts, and helpers appear).
   - When code violates discourse rules (e.g., burying initialization or placing incidental helpers at the top of a scope), expert recall and comprehension drop to the level of novices because top-down hypothesis verification fails.

2. **Pennington, N. (1987) & Von Mayrhauser, A. & Vans, A. M. (1995), *"Program Comprehension During Software Maintenance and Evolution"*, IEEE Computer, 28(8), 44–55** ✅
   - Synthesizes **Top-Down (Domain/Goal-Driven)** and **Bottom-Up (Control-Flow/Chunking)** comprehension into an integrated model: developers always orient first via the top-level structural beacons of a class or module (constructors, public signatures, field schema) before tracing into private helper implementations on demand.

3. **Busjahn, T. et al. (2015), *"Eye Movements in Code Reading: Relaxing the Linear Order"*, IEEE ICPC 2015** ✅
   - Eye-tracking experiments prove that programmers do **not** read source files strictly top-to-bottom from line 1 to line $N$ (**story order**); they scan structural headers first and jump along call edges (**execution order**).
   - Consequently, bottom-up (`callee`-before-`caller`) ordering optimizes for linear line-1-to-$N$ reading that developers do not perform, while destroying top-of-scope contract visibility.

4. **Martin, R. C. (2008), *Clean Code: A Handbook of Agile Software Craftsmanship*, Ch. 5 & Ch. 10 ("The Newspaper Metaphor", "The Stepdown Rule", "Class Organization")** ✅
   - Defines progressive disclosure for types: class variables/fields first, constructors second, public methods third, and private utility methods after the public methods.

---

## 2. External Linter State of the Art

### 2.1 SOTA Rules Catalog Across Ecosystems

| Ecosystem / Tool | Rule Name | What It Enforces | Relevance to Omni |
| :--- | :--- | :--- | :--- |
| **`wemake-python-styleguide`** | `WPS338` (`WrongMethodOrderViolation`) ✅ | Orders Python class methods: `__init_subclass__` $\to$ `__new__` $\to$ `__init__` $\to$ `__call__`/`__await__` $\to$ public & magic $\to$ `_protected` $\to$ `__private`. | Direct precedent for `constructor-after-method` (**C1**) and `private-before-public-method` (**C2**). Unimplemented in Ruff. |
| **`flake8-class-attributes-order`** | `CCE001` (`WrongClassAttributesOrder`) ✅ | Orders Python class body: fields/attributes $\to$ `__new__`/`__init__`/`__post_init__` $\to$ public/magic/property/classmethod/staticmethod $\to$ `_protected` $\to$ `__private`. | Direct precedent for `field-after-method` (**C3**), `constructor-after-method` (**C1**), and `private-before-public-method` (**C2**). |
| **Java Checkstyle** | `DeclarationOrder`, `OverloadMethodsDeclarationOrder`, `InnerTypeLast` ✅ | Enforces static/instance fields $\to$ constructors $\to$ methods (`DeclarationOrder`), contiguous overloads, and inner types last. | Confirms the universal OOP layout: **Fields $\to$ Constructors $\to$ Methods**. |
| **Kotlin Detekt** | `style:ClassOrdering` ✅ | Enforces properties & `init` blocks $\to$ secondary constructors $\to$ methods $\to$ companion object. | Same structural invariant (`Fields -> Constructors -> Methods`). |
| **Ruby RuboCop** | `Layout/ClassStructure` ✅ | Enforces constants/attributes $\to$ `initialize` $\to$ public methods $\to$ protected/private methods. | Same structural invariant (`Attributes -> Initializer -> Public -> Private`). |
| **SwiftLint** | `type_contents_order` ✅ | Enforces properties $\to$ `init`/`deinit` $\to$ public/instance methods $\to$ helpers. | Same structural invariant. |
| **TypeScript-ESLint / ESLint** | `@typescript-eslint/member-ordering`, `grouped-accessor-pairs`, `no-use-before-define` ✅ | `member-ordering` enforces fields $\to$ constructors $\to$ public methods $\to$ private methods. `no-use-before-define` disables `functions: false` in Airbnb, StandardJS, and Google configs. | Confirms why member role/visibility ordering succeeds while call-graph function ordering (`no-use-before-define` `functions: true`) is disabled in practice. |
| **Rust Clippy** | `clippy::items_after_test_module` (warn), `clippy::items_after_statements` (pedantic), `clippy::arbitrary_source_item_ordering` (restriction) ✅ | `items_after_test_module` flags any item placed after `#[cfg(test)] mod tests`. `arbitrary_source_item_ordering` only sorts `impl` methods alphabetically and has no constructor or visibility tier support. | `items_after_test_module` covers Rust test footers and inspires Python's `statement-after-main-guard` (**C7**). Clippy leaves `impl` constructor/visibility ordering (**C1**, **C2**) and `impl` associated-item ordering (**C6**) uncovered. |
| **Ruff & Mypy / Pyright** | Ruff `F821` (`undefined-name`), `E402` (`module-import-not-at-top-of-file`); Mypy overload/property checks ✅ | `F821` catches import-time `NameError`s (except inside functions called from an early `if __name__ == "__main__":`!). `E402` checks top-of-file imports. Mypy/Pyright reject non-contiguous `@overload` and split `@property` setters. | Explains why **C4** (`@overload`) and **C5** (`@property`) are already covered by type checkers, while **C1**, **C2**, **C3**, and **C7** (including the `F821` blind spot after `__main__`) are completely uncovered by Ruff! |

---

## 3. Comparison & Disposition Table (`R1`–`R8`)

| ID | Reference / Concept | Disposition | Rationale |
| :--- | :--- | :--- | :--- |
| **R1** | **Constructor-First Ordering (`C1`: `constructor-after-method`)** — Python (`__prepare__`, `__init_subclass__`, `__new__`, `__init__`, `__post_init__`) & Rust inherent `impl` (`pub`/`pub(...)` `new`, `try_new`, `new_*`, `try_new_*`) ✅ | **Adopt** | `97.5%–100%` compliance across all 5 corpora; uncovered by Ruff and Clippy. |
| **R2** | **2-Tier Visibility Ordering (`C2`: `private-before-public-method`)** — Python (`public`/dunder before `_private`/`__mangled`) & Rust inherent `impl` (`pub`/`pub(...)` before bare `fn`) ✅ | **Adopt** | `77.7%–91.0%` compliance; 2-tier visibility preserves Rust's restricted-constructor / public-accessor pattern (`AstNode::from_span`, `ripgrep::Error::new`, `cargo::Artifact::parse`) and catches 3 real defects in Omni `src/`. |
| **R3** | **Annotated Class Attributes Before Methods (`C3`: `field-after-method` / `attribute-after-method`)** — Python (`Stmt::AnnAssign` `x: T` after `def` in a class body) ✅ | **Recommend Adopting** | `100.0%` compliance across all 1,591 classes in CPython 3.14.8 (`0` false positives); avoids flagging `alias = method` assignments; directly complements `inline-public-attribute-annotation` and protects `@dataclass` / Pydantic field order. |
| **R4** | **No Statements After `if __name__ == "__main__":` (`C7`: `statement-after-main-guard`)** — Python counterpart to Clippy `items_after_test_module` ✅ | **Recommend Adopting** | `99.1%` compliance across 219 main-guarded files in CPython; closes a real **Ruff `F821` blind spot** where `def`/`class` declared below `if __name__ == "__main__": main()` causes a runtime `NameError` when the script is executed. |
| **R5** | **Associated `type` / `const` Before `fn` in Rust `impl` / `trait` (`C6`: `associated-item-after-method`)** ✅ | **Optional** | `100%` compliance in Omni `src/`, `98.5%` in ripgrep, `95.4%` in cargo. Clean and exact, though violations are relatively uncommon. |
| **R6** | **Intra-Scope Call-Graph Ordering (`C9`: `call-before-definition` / `stepdown`)** ✅ | **Retire** | Mathematically contradicts visibility tiers on cross-tier calls, inverts the abstraction gradient on same-tier calls, and conflicts with multi-entrypoint call DAGs (`36%–48%` mixed in CPython, Omni, and cargo). |
| **R7** | **Global Module Item-Kind / Visibility Buckets** (Clippy `module-item-order-groupings`) ✅ | **Reject** | Breaks cohesive feature clustering in both Python (`1,326` classes after `def` in CPython) and Rust (`56/84` files in Omni `src/`). |
| **R8** | **Split `@overload` (`C4`) & Split `@property` (`C5`)** ✅ | **Reject / Defer** | Already enforced by Mypy and Pyright (`Overloaded function signatures must be next to each other`, `Callable has no attribute "setter"`). |

---

## 4. Internal Codebase Building Blocks

| Need | Existing Building Block | Location |
| :--- | :--- | :--- |
| Walking Python classes, direct methods, and `Stmt::AnnAssign` fields | `StmtClassDef`, `Stmt::FunctionDef`, `Stmt::AnnAssign`, `extract_decorators_from_slice` | [src/code_lint/ast/python.rs](../../../src/code_lint/ast/python.rs), [src/code_lint/ast/python/classes.rs](../../../src/code_lint/ast/python/classes.rs) |
| Grouping Python `@overload` stubs & `@property` getter/setter/deleter pairs | `collect_class_methods` (`TypeMethod`, `MethodVisibility`) | [src/code_lint/ast/python/classes.rs:L531-L583](../../../src/code_lint/ast/python/classes.rs#L531-L583) |
| Detecting Python `if __name__ == "__main__":` guards | `Stmt::If` with `Expr::Compare` matching `__name__` and `"__main__"` | [src/code_lint/ast/python.rs](../../../src/code_lint/ast/python.rs) |
| Walking Rust inherent `impl` blocks & `ast::Fn` visibility/receiver | `ra_ap_syntax::ast::{Impl, AssocItem, Fn, HasVisibility, HasName}` | [src/code_lint/ast/rust.rs](../../../src/code_lint/ast/rust.rs) |
| Taxonomy topic `Topic::DECLARATION_ORDER` | Already declared in `src/rule_declaration/taxonomy.rs` and `docs/dev/tag_guide.md` | [src/rule_declaration/taxonomy.rs:L247-L254](../../../src/rule_declaration/taxonomy.rs#L247-L254) |
| Placeholders `{function}`, `{name}`, `{class}` & verb `Move` | Already in `PLACEHOLDERS` and `SUGGESTION_VERBS` | [tests/registry.rs:L367-L417](../../../tests/registry.rs#L367-L417) |
