# Phase 1: Understand — `InstanceAttributeAnnotationRule` → `inline-public-attribute-annotation`

This document records **Phase 1 (Understand)** of the exploration cycle for evaluating the legacy linter's `InstanceAttributeAnnotationRule` ([legacy_lints.py:L1674-L1820](../../../scratch/legacy_reference/legacy_lints.py#L1674-L1820)) for Omni.

> **Status**: **VALIDATED** (2026-10-03). Goals G1–G4, Non-Goals NG1–NG4, Decisions D1–D6, and Open Questions Q1–Q3 accepted:
> - **Q1**: Drop Check 2 and Check 3 unconditionally; implement the narrowed Check 1 as `inline-public-attribute-annotation`.
> - **Q2**: Highlight the `assignment` AST node (`self.foo: int = 1` / `self.foo: int`).
> - **Q3**: Exempt bare `self.foo: Final = ...` (unparameterized `Final`, including `typing.Final`, `typing_extensions.Final`, and `Annotated[Final, ...]`), while flagging parameterized `self.foo: Final[int] = ...`.

---

## 1. Dissecting the legacy linter's `InstanceAttributeAnnotationRule`

In the legacy linter ([legacy_lints.py:L1674-L1820](../../../scratch/legacy_reference/legacy_lints.py#L1674-L1820)), `InstanceAttributeAnnotationRule` inspects `ast.ClassDef` nodes (skipping `@dataclass` classes) and packs **three distinct checks** with three separate diagnostic messages into a single rule:

| Check | Target Construct | The legacy linter Condition (`legacy_lints.py`) | The legacy linter Diagnostic Message |
| :--- | :--- | :--- | :--- |
| **Check 1**<br>(`_verify_public_attribute`, L1740–1751) | Public instance attribute annotated inline in a method (`self.foo: T = ...` or `self.foo: T`, where `not foo.startswith("_")`) | `ann_list = self_ann_assigns.get(attr, [])` is non-empty for a non-`_`-prefixed attribute. Reports every `AnnAssign` node in `ann_list`. | ``Public attribute `{attr}` must be annotated at the class level, not inline.`` |
| **Check 2**<br>(`_verify_private_attribute`, L1721–1728) | Private instance attribute annotated at the class body level (`_foo: T`, where `_foo.startswith("_")`) | `attr in all_attrs` (assigned via `self._foo` in a method) **and** `attr in class_annotations` **and** `"ClassVar" not in ast.unparse(annotation)` **and** `class_annotations[attr].value is None`. | ``Private instance attribute `{attr}` must not be annotated at the class level. Annotate it inline inside methods (e.g. `self.{attr}: Type = ...`) instead.`` |
| **Check 3**<br>(`_verify_private_attribute`, L1730–1738) | Private instance attribute annotated inline more than once across methods (`self._foo: T = ...` repeated) | `len(self_ann_assigns.get(attr, [])) > 1` for a `_`-prefixed attribute. Reports `ann_list[1:]`. | ``Private instance attribute `{attr}` is annotated inline multiple times. Specify the annotation only on the first assignment.`` |

### 1.1 Architectural Violation in the legacy linter: Bundling Three Rules into One

Under [Rule Design Guide §1](../rule_design_guide.md) (*1 Rule = 1 Antipattern; The Split Test*) and [Naming and Message Style Guide §2](../naming_and_message_style_guide.md) ([ADR 010](../../../decisions/010_naming_and_message_conventions.md)):
- Every `CodeRule` in Omni has **exactly one `ViolationTemplate`** (`summary`, `rationale`, `suggestion`).
- Checks 1, 2, and 3 target **opposite syntactic constructs** (`self.foo: T` in a method vs. `_foo: T` in a class body vs. duplicate `self._foo: T` across methods), have **different rationales**, and prescribe **opposite fixes** (move annotation *to* class body vs. move annotation *into* method vs. delete duplicate `: T`).
- Therefore, the legacy linter's 3-in-1 bundle **cannot** be ported as a single rule. Each check must be evaluated on its own merits and either isolated into a single-purpose rule or dropped.

### 1.2 Implementation Bugs & Quirks in the legacy linter's AST Logic

Even on its own terms, the legacy linter's `SelfAttributeVisitor` and `InstanceAttributeAnnotationRule` contain several structural defects:

1. **Accidental Conditionality of Check 2 on `all_attrs` (L1797–1800)**:
   - The legacy linter computes `all_attrs = set(self_ann_assigns.keys()) | set(self_assigns.keys())` (only attributes assigned via `self.<attr>` in a method) and iterates `for attr in all_attrs:`.
   - Consequently, a class-body `_foo: int` is **only** flagged by Check 2 if some method in the same class also assigns `self._foo = ...`! If `_foo: int` is never assigned on `self` in that class body (or is assigned via `super().__init__()` or `setattr`), Check 2 silently ignores it.
2. **Only `@dataclass` Is Skipped (`_should_skip_class`, L1754–1761)**:
   - Notice *why* the legacy linter had to skip `@dataclass`: because `@dataclass` requires all fields—including private fields like `_cache: dict[str, int]`—to be annotated in the class body, which would otherwise trigger Check 2 whenever a method assigns `self._cache = ...`!
   - However, the legacy linter fails to skip `@attrs.define` / `@attr.s`, `pydantic.BaseModel`, `msgspec.Struct`, `TypedDict`, `NamedTuple`, or `Protocol` / `ABC`. In a Pydantic `BaseModel` or `attrs` class that declares `_cache: dict[str, int]` in the class body and resets `self._cache = {}` in a method, Check 2 falsely flags the required class-body declaration.
3. **Unbounded `generic_visit` in `SelfAttributeVisitor` (L1682–1701)**:
   - `SelfAttributeVisitor` runs `self.generic_visit(node)` inside every method without stopping at nested `class_definition` or nested `function_definition` boundaries. If a method defines a local helper class with its own `def __init__(self): self.x: int = 1`, the legacy linter attributes `self.x` to the *outer* class.
4. **Control-Flow Blindness in Check 3 (L1730–1738)**:
   - Check 3 counts raw `AnnAssign` nodes (`len(ann_list) > 1`). If `__init__` or `@overload` methods annotate `self._foo` in mutually exclusive branches or overload signatures, Check 3 flags the second branch.
5. **Check 1 Only Inspects `AnnAssign` (`self.foo: T = ...`), Not Unannotated `Assign` (`self.foo = ...`)**:
   - Despite collecting `self.assigns` (`self.foo = 1`), `_verify_public_attribute` (L1740–1751) never uses `self_assigns`! It only checks whether `self_ann_assigns.get(attr, [])` is non-empty. That is, the legacy linter does **not** require unannotated `self.foo = 1` to have a class-level annotation; it only bans writing `: T` inline on `self.foo: T = 1`.

---

## 2. Critical Evaluation of the 3 Checks Against SOTA

Detailed evidence and external citations are documented in [02_references.md](02_references.md). Below is the synthesis for each check:

### 2.1 Check 3 (Duplicate Inline `self._foo: T` Annotations) → **DROP**

- **Redundant with Type Checkers**:
  - **Mypy** (`[no-redef]`: *"Name `...` already defined on line N"*) and **Pyright** (`reportAttributeAccessIssue` / `reportGeneralTypeIssues`: *"Declaration `...` is obscured by a declaration of the same name"*) **already** flag duplicate or conflicting attribute type annotations on `self.<attr>` (both public and private).
- **Inferior to Type Checkers**:
  - Mypy and Pyright are flow-sensitive (understanding `if`/`else` branches, `try`/`except`, and `@overload` stubs) and check whether a class-body annotation and an inline annotation conflict. The legacy linter's naive AST node count (`len(ann_list) > 1`) is flow-insensitive and ignores public attributes (`self.foo: T`) only because Check 1 already banned them.
- **Verdict**: **Drop Check 3.** It duplicates standard type checkers with lower precision.

### 2.2 Check 2 (Forbidding Private Instance Attributes `_foo: T` in the Class Body) → **DROP**

- **Contradicts PEP 526 and the Python Typing Specification**:
  - [PEP 526](https://peps.python.org/pep-0526/#class-and-instance-variable-annotations) and the [Python Typing Specification (ClassVar)](https://typing.python.org/en/latest/spec/class-compat.html#classvar) explicitly designed value-less class-body annotations (`_damage: int`) to declare instance variables without creating a runtime attribute in `cls.__dict__`. Neither PEP 526 nor the [Google Python Style Guide](https://google.github.io/styleguide/pyguide.html#319-type-annotations) differentiates between public (`damage: int`) and private (`_damage: int`) instance variable annotations in the class body.
- **Harms Idiomatic Python Patterns**:
  Forbidding `_foo: T` in the class body is an **idiosyncratic anti-pattern** that actively breaks or degrades valid Python code in six common scenarios:
  1. **`__slots__` Classes**: In standard classes using `__slots__ = ("_foo", "_bar")`, declaring `_foo: int` (without a default value) in the class body is the canonical way to type-annotate slotted attributes for Mypy, Pyright, and `typeshed` without colliding with the slot descriptor in `cls.__dict__`.
  2. **`pydantic.BaseModel`, `attrs`, and `msgspec.Struct`**:
     - In Pydantic v2 (`BaseModel`), private instance attributes (`_foo: int` or `_foo: int = PrivateAttr()`) **must** be declared in the class body so Pydantic's metaclass registers them in `__private_attributes__`; omitting the class-body declaration and assigning `self._foo: int = 1` in a method raises `ValueError: "..." object has no field "_foo"` at runtime!
     - In `@attrs.define` / `@attr.s`, private fields (`_foo: int`) are declared in the class body.
  3. **`Protocol`, `ABC`, `TypedDict`, and `NamedTuple`**:
     - Abstract bases and structural protocols often declare protected attributes (`_state: int`) in the class body for subclasses or default mixin methods to assign (`self._state = ...`), and classes without `__init__` rely on class-body annotations.
  4. **Multi-Method or Helper Initialization (`__new__`, `reset()`, `setUp()`)**:
     - When a class initializes or resets private state in a helper method (`def _reset(self) -> None: self._buf = []` called from `__init__` and `clear()`), Pyright (`reportUninitializedInstanceVariable`) and Mypy recommend declaring `_buf: list[str]` in the class body so method order does not affect type inference.
  5. **Conditional Initialization in `__init__`**:
     - When `self._client` is assigned in `if`/`else` branches of `__init__`, declaring `_client: Client | None` once in the class body (or before the `if`) is cleaner than attaching `: Client | None` to the first branch's `self._client` assignment.
  6. **PEP 591 `Final` Private Attributes**:
     - The [Python Typing Specification for Final](https://typing.python.org/en/latest/spec/qualifiers.html#final) explicitly specifies class-body `_x: Final[int]` paired with `self._x = 1` in `__init__`.
- **Perverse Incentive ([Rule Design Guide §3](../rule_design_guide.md))**:
  - Forcing public attributes into the class header while banning private attributes from the class header prevents developers from keeping a class's complete state layout together at the top of the class when they want to.
- **Verdict**: **Drop Check 2.** It is dogmatic, contradicts PEP 526 and Pyright/Mypy best practices, and breaks framework classes (`pydantic.BaseModel`, `attrs`, `__slots__`).

### 2.3 Check 1 (Forbidding Inline Method Annotations on Public Attributes `self.foo: T`) → **NARROW to `inline-public-attribute-annotation`**

Unlike Checks 2 and 3, Check 1 (`self.foo: T = ...` or `self.foo: T` for a **public** attribute inside an instance method) has a coherent architectural rationale:

1. **Public Attributes Are Part of the Class Header Contract**:
   - Unlike `_`-prefixed private state, a public attribute (`foo`) is part of the class's external interface. Declaring `foo: T` in the class body places the public data contract at the top of the class alongside class docstrings and fields (matching the [Google Python Style Guide §3.8.3 / §3.19](https://google.github.io/styleguide/pyguide.html) layout), instead of burying public attribute types inside `__init__` or other methods (`setup()`, `run()`).
2. **Runtime `cls.__annotations__` & `typing.get_type_hints(cls)` Visibility**:
   - Per [PEP 526 (Runtime Effects of Type Annotations)](https://peps.python.org/pep-0526/#runtime-effects-of-type-annotations), Python's compiler **discards** local and `self.<attr>: T` annotations inside functions/methods at compile time—they are **not** stored in `cls.__annotations__`.
   - Consequently, `typing.get_type_hints(MyClass)` and runtime introspection/documentation tools (`inspect.get_annotations`, Sphinx `autodoc`, `mkdocstrings`/`griffe`) only see public attributes when they are annotated in the class body (`class MyClass: foo: T`).
3. **Synergy with Omni's Existing Attribute Rules (`collect_public_class_attributes`)**:
   - Today, [collect_public_class_attributes](../../../src/code_lint/ast/python.rs#L2050-L2116) (used by `concrete-collection-attribute` and `mutable-collection-attribute`) has to walk *both* the class body and `__init__` (`collect_init_annotated_attrs_rec`), and still misses public attributes annotated inline in non-`__init__` methods (`def reset(self): self.items: list[str] = []`). Encouraging class-body annotations for public attributes unifies where public attribute types live.
4. **Dropping Check 2 Removes the `@dataclass` Conflict**:
   - Once Check 2 is dropped, Check 1 applies cleanly to *all* classes (including `@dataclass`, where annotating `self.extra: int = 0` inside `__post_init__` instead of declaring `extra: int = field(init=False)` in the class body is a genuine bug).

---

## 3. Goals & Explicit Non-Goals

### 3.1 Goals

| ID | Goal | Reason |
| :--- | :--- | :--- |
| **G1** | Enforce a single, high-signal antipattern (`1 Rule = 1 Antipattern`): flagging inline type annotations on public instance attributes (`self.foo: T`) inside instance methods. | Satisfies [Rule Design Guide §1](../rule_design_guide.md) and [ADR 010](../../../decisions/010_naming_and_message_conventions.md) with one orthogonal `ViolationTemplate`. |
| **G2** | Drop the legacy linter's Check 2 (banning `_foo: T` in class bodies) and Check 3 (duplicate inline `self._foo: T` annotations). | Eliminates false positives on `__slots__`, `pydantic.BaseModel`, `attrs`, `Protocol`, multi-method init, and avoids duplicating Mypy `[no-redef]` / Pyright. |
| **G3** | Fix the legacy linter's AST scoping and qualifier bugs (nested classes/functions, `@staticmethod`/`@classmethod` receivers, bare `Final` qualifier). | Prevents false positives on inner classes/functions defined inside methods, static/class methods, and avoids suggesting invalid class-body syntax for bare `Final`. |
| **G4** | Maintain strict architectural separation between `code_lint::ast::python` and `code_lint::rules`. | All Tree-sitter CST traversal stays in [python.rs](../../../src/code_lint/ast/python.rs). |

### 3.2 Explicit Non-Goals

| ID | Non-Goal | Reason |
| :--- | :--- | :--- |
| **NG1** | Banning private instance attribute annotations (`_foo: T`) in the class body (the legacy linter Check 2). | Contradicts PEP 526, `__slots__` typing, `pydantic.BaseModel`, `attrs`, and Pyright `reportUninitializedInstanceVariable`. |
| **NG2** | Checking for duplicate or conflicting attribute annotations (the legacy linter Check 3). | Already enforced with control-flow and type awareness by Mypy (`[no-redef]`, `[assignment]`) and Pyright. |
| **NG3** | Requiring every unannotated `self.foo = val` assignment to have a class-body `foo: T` declaration. | When `def __init__(self, foo: int): self.foo = foo` assigns a typed parameter or literal, Mypy and Pyright infer `self.foo` without redundant annotations; Pyright's `reportUninitializedInstanceVariable` handles uninitialized attributes for teams that want strict slot/field declarations. |
| **NG4** | Rust language support. | Rust structs require all fields (public and private) to be declared with types in the `struct` item definition at compile time; inline field creation in methods does not exist in Rust. |

---

## 4. Validated Decisions (D1–D6)

| ID | Decision | Rationale |
| :--- | :--- | :--- |
| **D1** | **Scope Reduction**: Drop Check 2 (class-level `_foo: T` ban) and Check 3 (duplicate inline `self._foo: T`); implement the narrowed Check 1 as **`inline-public-attribute-annotation`**. | Keeps only the check with a genuine runtime (`__annotations__`) and interface-visibility rationale while eliminating the legacy linter's anti-patterns. |
| **D2** | **Rule Name & File**: `inline-public-attribute-annotation` (`src/code_lint/rules/inline_public_attribute_annotation.rs`, `pub const RULE: CodeRule`). | 4 words, kebab-case, names the flagged construct (an inline public attribute annotation) with no polarity prefix ([Naming Guide §1](../naming_and_message_style_guide.md)). |
| **D3** | **Target & Mode**: `SupportLang::Python`, `RuleTarget::SourceOnly`, `RuleOptions::code_rule(())` (default `EnforcementMode::Ban`, configurable to `require-explanation`). | Matches `concrete-collection-attribute` and `mutable-collection-attribute`: test helper classes in `tests/` routinely set up ad-hoc attributes in `setUp()` / `__init__` without formal class-header contracts. |
| **D4** | **Classification**:<br>- `topics: &[Topic::STATIC_TYPING]`<br>- `precision: Precision::Exact`<br>- `consensus: Consensus::Opinionated`<br>- `impacted_quality: ImpactedQuality::Maintainability` | Inspects type annotations (`STATIC_TYPING`); syntactically exact (`Precision::Exact`); PEP 526 permits `self.foo: T` in `__init__` so reasonable developers disagree (`Consensus::Opinionated`); affects class contract readability and `__annotations__` discoverability (`Maintainability`). |
| **D5** | **AST Scope, Receiver & Bare `Final` Rules**:<br>- Only inspect direct methods (`function_definition`) of a `class_definition` body that are not decorated with `@staticmethod` or `@classmethod` and whose first parameter is a `Receiver` named `"self"`.<br>- Do **not** recurse into nested `class_definition`, `function_definition`, or `lambda` inside a method.<br>- Match `assignment` nodes with both a `left` (`attribute` where `object` is identifier `"self"` and `!attribute.starts_with('_')`) and a `type` node (including when the attribute is already annotated in the class body).<br>- Skip unparameterized `Final` (`self.foo: Final = 1`, `typing.Final`, `typing_extensions.Final`, or `Annotated[Final, ...]`), while flagging parameterized `self.foo: Final[int] = 1`.<br>- Highlight the `assignment` node (`self.foo: int = 1` / `self.foo: int`). | Fixes the legacy linter's `SelfAttributeVisitor` nested-scope and staticmethod bugs, highlights the full annotated assignment clearly, and respects PEP 591's grammar constraint on bare `Final`. |
| **D6** | **Violation Template**:<br>- **`summary`**: ``Public attribute `self.{name}` of `{class}` is annotated inline in `{function}` with `{expression}`.``<br>- **`rationale`**: ``Inline attribute annotations inside methods are omitted from `{class}.__annotations__` at runtime and hide the class's public data contract inside method bodies.``<br>- **`suggestion`**: ``Move `{name}: {expression}` to the body of `{class}` and assign `self.{name}` without a type annotation in `{function}`.`` | Strictly orthogonal What / Why / How; uses existing `PLACEHOLDERS` (`name`, `class`, `function`, `expression`) and `SUGGESTION_VERBS` (`Move`) from [registry.rs](../../../tests/registry.rs) without needing vocabulary additions. |

---

## 5. Resolved Questions (Q1–Q3)

- **Q1 (Narrow to Check 1 vs. Drop Entirely)**: **Validated** — drop Check 2 and Check 3 unconditionally, and implement the narrowed Check 1 as `inline-public-attribute-annotation`.
- **Q2 (Diagnostic Span)**: **Validated** — highlight the `assignment` AST node (`self.foo: int = 1` or `self.foo: int`).
- **Q3 (Bare `Final` Exemption — D5)**: **Validated** — exempt unparameterized `Final` (`self.foo: Final = ...`, `typing.Final`, `typing_extensions.Final`, `Annotated[Final, ...]`), while flagging parameterized `self.foo: Final[int] = ...`.
