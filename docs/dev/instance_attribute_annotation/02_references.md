# Phase 2: Gather Resources and References — `InstanceAttributeAnnotationRule`

This document records **Phase 2 (Gather Resources and References)** for evaluating the legacy linter's `InstanceAttributeAnnotationRule` ([legacy_lints.py:L1674-L1820](../../../scratch/legacy_reference/legacy_lints.py#L1674-L1820)). It provides the external SOTA evidence and internal codebase analysis backing the recommendations in [01_understand.md](01_understand.md).

Confidence markers: ✅ verified against official specifications, documentation, and codebase source this session.

---

## 1. External State of the Art (PEPs, Type Checkers, Style Guides & Linters)

### 1.1 Python Typing Specifications (PEP 526, PEP 591 & `typing.python.org`)

1. **PEP 526 — Syntax for Variable Annotations** ✅ ([peps.python.org/pep-0526](https://peps.python.org/pep-0526/))
   - **Class and Instance Variable Annotations**:
     - PEP 526 introduced value-less annotations in class bodies (`damage: int`) specifically so instance variables initialized in `__init__` or `__new__` can be declared at the class level without creating a default class attribute in `cls.__dict__`:
       ```python
       class BasicStarship:
           captain: str = 'Picard'               # instance variable with default
           damage: int                           # instance variable without default
           stats: ClassVar[Dict[str, int]] = {}  # class variable
       ```
     - PEP 526 also permits inline annotations on `self` inside methods as a convenience:
       > *"As a matter of convenience (and convention), instance variables can be annotated in `__init__` or other methods, rather than in the class."*
     - Crucially, PEP 526 makes **no distinction** between public (`damage: int`) and private (`_damage: int`) instance variables in the class body.
   - **Runtime Effects on `__annotations__`**:
     - Under [PEP 526 §Runtime Effects of Type Annotations](https://peps.python.org/pep-0526/#runtime-effects-of-type-annotations):
       - Annotating a variable in a **class body** (`foo: int` or `_foo: int`) populates `cls.__annotations__["foo"] = int` at class creation time (accessible via `typing.get_type_hints(cls)` and `inspect.get_annotations(cls)`), while leaving `cls.__dict__` untouched when no `= value` is present.
       - Annotating a local variable or attribute **inside a function or method** (`self.foo: int = 1`) causes the Python compiler to **completely discard the annotation at compile time**—neither the function's `__annotations__` nor the enclosing class's `__annotations__` records `"foo"`.
   - **Duplicate Type Annotations**:
     - PEP 526 §Global and local variable annotations states:
       > *"Duplicate type annotations will be ignored [at runtime]. However, static type checkers may issue a warning for annotations of the same variable by a different type."*

2. **PEP 591 & Python Typing Spec — `Final` Qualifier on Instance Attributes** ✅ ([typing.python.org/en/latest/spec/qualifiers.html#final](https://typing.python.org/en/latest/spec/qualifiers.html#final))
   - The official Python Typing Specification defines two ways to declare a final instance attribute:
     1. **In the class body without an initializer**: `x: Final[int]` (followed by `self.x = 1` in `__init__`). The spec explicitly mandates:
        > *"In class bodies and stub files you can omit the right hand side and just write `ID: Final[float]`. If the right hand side is omitted, there must be an explicit type argument to `Final`."*
     2. **In `__init__` on `self`**: `self.id: Final = 1` or `self.id: Final[int] = 1` (allowed *only* in `__init__`).
   - **Direct Implication for the legacy linter's Checks**:
     - The legacy linter's **Check 2** flags `_x: Final[int]` in a class body (because `is_class_var` only checks `"ClassVar" in anno_str` and `value is None`!), directly contradicting the PEP 591 specification example `class ImmutablePoint: x: Final[int]`.
     - For **Check 1**, if a developer writes bare `self.x: Final = 1` (without a type argument `[int]`), moving `x: Final` directly to the class body without adding `[int]` is a PEP 591 type error.

3. **Python Typing Spec — `Protocol` (PEP 544), `TypedDict` (PEP 589), `NamedTuple`, and Dataclasses (PEP 557)** ✅
   - Structural `Protocol` classes, `TypedDict` definitions, `NamedTuple` classes, `@dataclass`, `@attrs.define`, `pydantic.BaseModel`, and `msgspec.Struct` all use class-body `AnnAssign` syntax as their declarative schema mechanism.
   - In **`pydantic.BaseModel`**, private instance attributes (`_cache: dict[str, str]` or `_cache: dict[str, str] = PrivateAttr(default_factory=dict)`) **must** be declared in the class body so Pydantic's metaclass registers them in `__private_attributes__`. If a developer follows the legacy linter Check 2's suggestion and removes `_cache: ...` from the `BaseModel` class body in favor of `self._cache: dict[str, str] = {}` inside a method, Pydantic raises `ValueError: "MyModel" object has no field "_cache"` at runtime!

---

### 1.2 Static Type Checkers (Mypy & Pyright)

1. **Mypy (`[no-redef]`, `[assignment]`, `[attr-defined]`)** ✅
   - **Duplicate Inline Annotations (the legacy linter Check 3)**:
     - Mypy already tracks first-definition types for `self.<attr>` across methods and flags re-annotations with `error: Attribute "<attr>" already defined on line N [no-redef]` (or `error: Incompatible types in assignment [assignment]` if the types differ).
     - Unlike the legacy linter's naive `len(ann_list) > 1` AST count, Mypy's binder understands control flow (`if`/`else` branches, `@overload` signatures) and class inheritance.
   - **Class-Body Private Attributes (the legacy linter Check 2)**:
     - Mypy treats `_foo: T` in a class body identically to `foo: T` as an instance attribute declaration. When `__slots__ = ("_foo",)` is used, Mypy relies on either `_foo: T` in the class body or `self._foo: T` in `__init__` to type the slot.

2. **Pyright / Pylance (`reportUninitializedInstanceVariable`, `reportGeneralTypeIssues`, `reportIncompatibleVariableOverride`)** ✅
   - **Duplicate / Redeclared Attributes (the legacy linter Check 3)**:
     - Pyright flags redeclaring an attribute's type (`Declaration "<attr>" is obscured by a declaration of the same name`) when an attribute is annotated multiple times or with conflicting types.
   - **Class-Body Declarations for Multi-Method Initialization (Contra the legacy linter Check 2)**:
     - When `reportUninitializedInstanceVariable` is enabled (or when attributes are initialized in helper methods like `reset()` / `setUp()` rather than directly in `__init__`), Pyright specifically recommends declaring the attribute's type (both public `foo: T` and private `_foo: T`) in the class body so the attribute is known across all methods regardless of method ordering.

---

### 1.3 Style Guides & Linters (Google Python Style Guide, Ruff, Pylint, `wemake-python-styleguide`)

1. **Google Python Style Guide (§3.8.3 Classes & §3.19 Type Annotations)** ✅ ([google.github.io/styleguide/pyguide.html](https://google.github.io/styleguide/pyguide.html))
   - Demonstrates annotating instance and class attributes directly in the class body (`attr1: str`, `attr2: int = 0`) followed by `self.attr1 = attr1` in `__init__`, avoiding redundant type declarations in both the `Attributes:` docstring section and `__init__` assignments.
   - Never prohibits annotating private attributes (`_attr: T`) in the class body.

2. **Ruff (`flake8-annotations` `ANN`, `flake8-bugbear` `B`, `RUF012`, `flake8-pyi` `PYI`)** ✅
   - `ANN` enforces parameter and return type annotations on functions/methods; it does **not** enforce or ban class-body vs. inline `self.<attr>` annotations.
   - `RUF012` (`mutable-class-default`) flags mutable default values in class attributes (`items: list[int] = []`) without `ClassVar` or `Final`. Notably, value-less class-body annotations (`items: list[int]` or `_items: list[int]`) have **no runtime default value** and are **never** flagged by `RUF012`.

3. **Pylint (`W0201` `attribute-defined-outside-init`)** ✅
   - `W0201` warns when `self.attr = ...` is assigned in a method other than `__init__` unless `attr` is already declared in `__init__` or the class body. Declaring `_attr: T` or `attr: T` in the class body is a standard way to declare attributes initialized by lifecycle hooks (`setUp`, `reset`, `__enter__`).

4. **`wemake-python-styleguide` (`WPS601` `ShadowedClassAttributeViolation`)** ✅
   - `WPS601` flags `self.x = 2` only when `x = 1` (with a runtime value) is defined on the class, shadowing a class attribute with an instance attribute. Value-less `AnnAssign` (`x: int` or `_x: int`) in the class body is explicitly exempt because it declares an instance attribute's type, not a runtime class attribute.

---

### 1.4 Summary Comparison Table (`R1`–`R7`)

| ID | Reference | Relevance to the legacy linter's 3 Checks | Verdict for Omni |
| :--- | :--- | :--- | :--- |
| **R1** | **PEP 526** (`Syntax for Variable Annotations`) ✅ | Designed value-less `attr: T` in class bodies for instance variables (both public and private). Notes that function/method-local annotations (`self.attr: T`) are discarded from runtime `__annotations__`. | **Reject Check 2** (class-body `_foo: T` is standard PEP 526).<br>**Supports Check 1 rationale** (`cls.__annotations__` only records class-body annotations). |
| **R2** | **PEP 591 & Python Typing Spec (`Final`)** ✅ | Explicitly specifies `x: Final[int]` in class body + `self.x = 1` in `__init__`. Forbids bare `x: Final` (without `[T]`) in class body when `= val` is omitted. | **Reject Check 2** (which falsely flags `_x: Final[int]` in class bodies).<br>**Adapt for Check 1 (D5)**: exempt or special-case bare `self.x: Final = 1`. |
| **R3** | **Mypy (`[no-redef]`) & Pyright (`reportGeneralTypeIssues`)** ✅ | Already flag duplicate/conflicting `self._foo: T` annotations with full control-flow and inheritance awareness. | **Reject Check 3** as 100% redundant with type checkers and more error-prone on `if`/`else` branches. |
| **R4** | **Pydantic `BaseModel`, `attrs`, & `__slots__`** ✅ | Require or strongly favor declaring private attributes (`_foo: T`) in the class body. | **Reject Check 2** (following Check 2 breaks Pydantic `BaseModel` private attributes at runtime). |
| **R5** | **Google Python Style Guide (§3.8.3 / §3.19)** ✅ | Places public attribute type declarations (`attr1: str`) in the class header and assigns `self.attr1 = attr1` without inline re-annotation in `__init__`. | **Supports Check 1** (`inline-public-attribute-annotation`) as an `Opinionated` readability/contract rule. |
| **R6** | **Ruff (`RUF012`, `ANN`) & `wemake` (`WPS601`)** ✅ | Confirm that value-less `attr: T` in a class body creates no runtime class attribute and does not shadow instance state. | Confirms moving `self.foo: T = val` to `foo: T` (class body) + `self.foo = val` (method) has zero runtime memory or shadowing cost. |
| **R7** | **Omni Rule Design & Naming Guides ([ADR 010](../../../decisions/010_naming_and_message_conventions.md))** ✅ | 1 Rule = 1 Antipattern = 1 `ViolationTemplate`. | The legacy linter's 3 checks cannot coexist in one rule; dropping Checks 2 & 3 leaves Check 1 as a clean single-template rule (or 0 rules if dropped). |

---

## 2. Internal Codebase Analysis (`src/code_lint/ast/python.rs`)

### 2.1 Existing Class & Attribute Extraction Infrastructure
We inspected [python.rs](../../../src/code_lint/ast/python.rs) to see how class and instance attributes are currently parsed and represented:

1. **`collect_public_class_attributes` ([python.rs:L2050-L2116](../../../src/code_lint/ast/python.rs#L2050-L2116)) and `collect_init_annotated_attrs_rec` ([python.rs:L2008-L2043](../../../src/code_lint/ast/python.rs#L2008-L2043))**:
   - Currently used by [concrete_collection_attribute.rs](../../../src/code_lint/rules/concrete_collection_attribute.rs) and [mutable_collection_attribute.rs](../../../src/code_lint/rules/mutable_collection_attribute.rs).
   - Look at how `collect_init_annotated_attrs_rec` already matches inline public attribute annotations in `tree-sitter-python`:
     ```rust
     if node.kind() == "assignment"
         && let (Some(left), Some(type_node)) = (node.field("left"), node.field("type"))
         && left.kind() == "attribute"
         && left
             .field("object")
             .is_some_and(|object_node| object_node.text() == "self")
         && let Some(attribute_node) = left.field("attribute")
     ```
   - Notice too how `collect_init_annotated_attrs_rec` properly stops at scope boundaries (`"function_definition" | "class_definition" | "lambda"` at [python.rs:L2014-L2019](../../../src/code_lint/ast/python.rs#L2014-L2019)), avoiding the legacy linter's `SelfAttributeVisitor` nested-scope bug!

2. **How `tree-sitter-python` Represents `self.foo: T = val` and `self.foo: T`**:
   - In `tree-sitter-python`, both `self.foo: int = 1` and value-less `self.foo: int` inside a method body are `expression_statement` $\to$ `assignment` nodes with:
     - `left`: `attribute` (`object`: `identifier` `"self"`, `attribute`: `identifier` `"foo"`)
     - `type`: `type` node (`int`)
     - `right`: optional value node (`1`, or `None` when value-less).

3. **Interaction Between `inline-public-attribute-annotation` (Check 1) and `concrete-collection-attribute` / `mutable-collection-attribute`**:
   - If a class has `def __init__(self) -> None: self.items: list[str] = []`:
     - `concrete-collection-attribute` flags `list[str]` (the type is a concrete mutable collection).
     - `inline-public-attribute-annotation` (if implemented) flags `self.items: list[str] = []` (the public attribute annotation is inline in `__init__` instead of in the class body).
   - These two rules are orthogonal in **why** they fire and **how** to fix them (`items: Sequence[str]` in the class body satisfies both).

4. **Interaction with `tests/registry.rs` Enforcements**:
   - We verified all constraints in [registry.rs](../../../tests/registry.rs#L351-L402):
     - Rule name `inline-public-attribute-annotation` is 4 words (`<= 4`), kebab-case, no polarity prefix/suffix.
     - Placeholders `{name}`, `{class}`, `{function}`, and `{expression}` are all in `PLACEHOLDERS`.
     - Suggestion verb `Move` is in `SUGGESTION_VERBS`.
     - Summary, rationale, and suggestion contain no `FIX_VERBS` in summary/rationale, no `JUDGEMENT_WORDS` (`banned`, `forbidden`, `discouraged`, `illegal`, `must`, `should`), and no `ABBREVIATIONS` (`e.g.`, `i.e.`).

---

## 3. Candid Conclusion & Recommendation

1. **Do NOT port the legacy linter's `InstanceAttributeAnnotationRule` as written.** Two of its three checks (Check 2: banning `_foo: T` in class bodies; Check 3: counting duplicate inline `self._foo: T` annotations) are anti-patterns or redundant with Mypy/Pyright.
2. **Selected Path — Option A: Narrow to Check 1 (`inline-public-attribute-annotation`)**:
   - Implement a small, exact, single-template rule in Omni that flags `self.<public_attr>: <T>` inside methods and guides the author to declare `<public_attr>: <T>` in the class body (`D1–D6`).
   - Drop Check 2 (conflicts with PEP 526, PEP 591, Pydantic `BaseModel`, `attrs`, and `__slots__`).
   - Drop Check 3 (duplicate or uninitialized attribute annotations), leaving it to Mypy, Pyright, Ruff `RUF012`, and Pylint `W0201`:
     ```toml
     [tool.mypy]
     enable_error_code = ["no-redef"] # Flags duplicate `self.attr: T` annotations across methods

     [tool.pyright]
     reportUninitializedInstanceVariable = "warning" # Flags attributes not initialized in `__init__` or declared on class
     reportGeneralTypeIssues = "error"               # Flags conflicting attribute redeclarations

     [tool.ruff.lint]
     extend-select = ["RUF012"] # mutable-class-default: flags mutable class defaults without ClassVar/Final

     [tool.pylint."messages control"]
     enable = ["attribute-defined-outside-init"] # W0201
     ```

---

## 4. Sources
- PEP 526 — Syntax for Variable Annotations: https://peps.python.org/pep-0526/
- PEP 591 — Adding a final qualifier to typing: https://peps.python.org/pep-0591/
- Python Typing Specification — Class type assignability (`ClassVar`): https://typing.python.org/en/latest/spec/class-compat.html#classvar
- Python Typing Specification — Type qualifiers (`Final`): https://typing.python.org/en/latest/spec/qualifiers.html#final
- Google Python Style Guide (§3.8.3 Classes, §3.19 Type Annotations): https://google.github.io/styleguide/pyguide.html
- Mypy Error Codes (`[no-redef]`): https://mypy.readthedocs.io/en/stable/error_code_list.html#check-that-each-name-is-defined-once-no-redef
- Ruff `RUF012` (`mutable-class-default`): https://docs.astral.sh/ruff/rules/mutable-class-default/
- Pylint `W0201` (`attribute-defined-outside-init`): https://pylint.readthedocs.io/en/stable/user_guide/messages/warning/attribute-defined-outside-init.html
- `wemake-python-styleguide` `WPS601` (`ShadowedClassAttributeViolation`): https://wemake-python-styleguide.readthedocs.io/en/latest/pages/usage/violations/oop.html
