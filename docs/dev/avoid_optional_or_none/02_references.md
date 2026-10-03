# Phase 2: Gather Resources and References — `AvoidOptionalOrNoneRule`

This document records **Phase 2 (Gather Resources and References)** for evaluating the legacy linter's `AvoidOptionalOrNoneRule` ([scratch/legacy_reference/legacy_lints.py](../../../scratch/legacy_reference/legacy_lints.py#L1982-L2019)). It builds on [01_understand.md](01_understand.md).

> **Status**: **VALIDATED** (2026-10-03). SOTA analysis and `nullable-collection-return` rescope accepted; `Precision::Heuristic` set in [03_design_plan.md](03_design_plan.md).

Confidence markers: ✅ verified against official docs/source this session · ⚠️ synthesized from tool behavior/literature · ❌ searched, not found.

---

## 1. External State of the Art (`R1`–`R9`)

### 1.1 Deep-Dive on Type Theory, Style Guides, and Linters

1. **Tony Hoare's "Billion-Dollar Mistake" (1965 / 2009) vs. Explicit Union-with-`None` Types (PEP 484 / PEP 604 / Rust `Option<T>`)** ✅
   - **What the "Billion-Dollar Mistake" actually is**: In ALGOL W (1965), C, C++, Java, C# (pre-C# 8), and untyped Python, `null` / `None` is a silent subtype of **every** reference type `T`. Any variable declared as `User` or `str` may secretly hold `null` at runtime without the compiler or type checker warning the caller.
   - **How modern type systems solved it**: ML, Haskell (`Maybe a`), Rust (`Option<T>`), Swift (`T?`), Kotlin (`T?`), and Python's PEP 484 (`strict_optional`, enabled by default in Mypy since 0.600) removed `None` from ordinary types `T` and required **explicit sum/union types** (`T | None` in PEP 604, `Option<T>` in Rust) whenever a value may be absent.
   - **Why banning `T | None` inverts the lesson**:
     - When `T | None` is used, static type checkers (Mypy, Pyright, `ty`) enforce **type narrowing** (`if x is not None:`) before any attribute access or operation on `T`.
     - When developers avoid `T | None` by substituting in-band sentinel values (`-1`, `""`, `0`, `NullUser`), the static type becomes `int`, `str`, or `User`, **disabling static narrowing checks** and reintroducing the exact runtime hazard Tony Hoare warned against.
     - Similarly, the **Null Object Pattern** (Fowler, *Refactoring*; Woolf, 1996) is only valid when the no-op behavior is polymorphic across all operations (e.g., a `NullLogger` that discards log messages, or an **empty collection `()` / `[]` whose `for x in items:` loop executes 0 times**). For scalar values or domain entities with invariants (`Bound`, `Order`, `UserId`), a "Null Object" is a silent runtime footgun.

2. **Ruff (`UP007`, `UP045`, `RUF013`, `RUF036`, `B006`, `B008`)** ✅
   - **Syntax modernization (`pyupgrade` & `ruff`)**:
     - **`UP007` (`non-pep604-annotation-union`)**: Replaces `typing.Union[A, B]` with `A | B`.
     - **`UP045` (`non-pep604-annotation-optional`)**: Replaces `typing.Optional[T]` with `T | None`.
     - **`RUF013` (`implicit-optional`)**: Bans PEP 484 legacy implicit optionals (`def f(x: int = None)`), requiring explicit `x: int | None = None`.
     - **`RUF036` (`none-not-at-end-of-union`)**: Enforces `T | None` ordering rather than `None | T`.
   - **Interaction with `B006` (`mutable-argument-default`) and `B008` (`function-call-in-default-argument`)**:
     - Ruff `B006` flags mutable literals/comprehensions in parameter defaults (`def f(x: list[int] = [])`, `def f(m: Mapping[str, int] = {})`, `def f(s: set[int] = set())`) and explicitly prescribes `None` as the default (`def f(x: list[int] | None = None): if x is None: x = []`).
     - Because Python has **no immutable dict literal** (`{}` creates a `dict` at runtime), `B006` makes `param: Mapping[K, V] | None = None` mandatory for optional mapping parameters (unless a module-level `EMPTY_MAP: Mapping[str, Any] = MappingProxyType({})` constant is defined).
   - **Absence of any `T | None` ban**: Ruff has over 800 rules across 60+ plugins and has **zero rules** discouraging `T | None` in parameters, returns, or attributes ❌.

3. **Pylint & `wemake-python-styleguide` (WPS)** ✅
   - Neither Pylint nor `wemake-python-styleguide` (the strictest opinionated Python linter in the ecosystem) bans `T | None` or `Optional[T]` in annotations ❌.
   - Pylint's `W0102` (`dangerous-default-value`) likewise mandates `param: list[T] | None = None` instead of `param: list[T] = []`.

4. **Rust Clippy (`clippy::ref_option`, `clippy::option_option`, `clippy::unnecessary_wraps`)** ✅
   - **`clippy::ref_option`** (pedantic): Flags `&Option<T>` in parameters/returns and suggests `Option<&T>` (or `Option<&[T]>` / `Option<&str>`) to avoid requiring the caller to own an `Option<T>`. Note that it replaces `&Option<T>` with **`Option<&T>`**—it preserves `Option`!
   - **`clippy::option_option`** (pedantic): Flags nested `Option<Option<T>>`.
   - **`clippy::unnecessary_wraps`** (pedantic): Flags **private** functions whose body returns `Some(...)` or `Ok(...)` on every code path and never returns `None` or `Err(...)`. It inspects the **function body return statements**, not the type signature in isolation, and only runs on private functions.

5. **Effective Java Item 54 & Item 55, SonarQube `S1168` & `S3553`, and PMD `ReturnEmptyCollectionRatherThanNull`** ✅
   - **Effective Java (3rd ed.) Item 54 (*"Return empty collections or arrays, not nulls"*)**:
     - Explains that returning `null` instead of an empty collection forces callers to write error-prone `if (items != null)` guards before iterating, whereas returning an empty collection (`Collections.emptyList()`, or in Python `()`, `[]`, `{}`, `frozenset()`) allows callers to handle empty and non-empty results uniformly with zero extra cost.
   - **Effective Java (3rd ed.) Item 55 (*"Return optionals judiciously"*)**:
     - Explicitly states: *"Container types, including collections, maps, streams, arrays, and optionals should not be wrapped in optionals. Rather than returning an empty `Optional<List<T>>`, you should simply return an empty `List<T>` (Item 54)."*
   - **SonarQube `S1168` ("Empty arrays and collections should be returned instead of null") & PMD `ReturnEmptyCollectionRatherThanNull`**:
     - Enforces Item 54 on return types across Java, C#, Kotlin, and C++.
   - **SonarQube `S3553` ("`Optional` should not be used for parameters")**:
     - Note: Java's `S3553` exists only because Java has **method overloading** (`foo(String x)` + `foo()`) and Java's `Optional<T>` is a heap-allocated wrapper box around an already-nullable reference (`Optional<T>` can itself be `null` in Java!). In Python, there is no runtime method overloading, and `T | None` has zero runtime boxing overhead (`None` is a singleton identity reference).

---

### 1.2 SOTA Summary Table (`R1`–`R8`)

| ID | Reference | What It Covers | Adopt / Adapt / Reject | Why |
| :--- | :--- | :--- | :--- | :--- |
| **R1** | **The legacy linter `AvoidOptionalOrNoneRule`** ([legacy_lints.py:1982-2019](../../../scratch/legacy_reference/legacy_lints.py#L1982-L2019)) ✅ | Non-blocking git-diff warning on every `\| None`, `Optional[...]`, or `Union[..., None]` in parameters, returns, and attributes. | **Reject as-is** (**D1**). | Fires on almost every idiomatic Python file (18× in `legacy_lints.py` itself), violates 1-rule-1-antipattern, collides with Ruff `B006`, and incentivizes untyped magic sentinels (`-1`, `""`). |
| **R2** | **PEP 484 (`strict_optional`) & PEP 604 (`T \| None`)** ✅ | Explicit union with `None` as the static typing representation of optionality. | **Adopt** as foundational principle (**D1**). | Explicit `T \| None` forces static type narrowing (`if x is not None:`), solving Tony Hoare's implicit-nullability mistake. |
| **R3** | **Ruff `UP007`, `UP045`, `RUF013`, `RUF036`** ✅ | Syntax modernization (`Optional[T]` $\to$ `T \| None`), banning implicit optional (`x: int = None`), canonical union order. | **Leave to Ruff** (no overlap). | Pure formatting/syntax modernization already handled by Ruff. |
| **R4** | **Ruff `B006` (`mutable-argument-default`) & Pylint `W0102`** ✅ | Bans mutable collection defaults (`= []`, `= {}`, `= set()`), requiring `param: Collection \| None = None`. | **Respect** (**D2**). | Explains why flagging `Mapping[K, V] \| None = None` or `MutableSequence[T] \| None = None` on parameters would directly contradict standard Python linters. |
| **R5** | **Effective Java Item 54 & Item 55** ✅ | *"Return empty collections or arrays, not nulls"* and *"Container types, including collections, maps, streams, arrays, and optionals should not be wrapped in optionals."* | **Adopt if rescoped to `nullable-collection-return`** (**Option 2c**). | Collection types already have a natural empty value (`()`, `[]`, `{}`, `frozenset()`), so returning `Collection[T] \| None` creates a redundant two-state empty representation. |
| **R6** | **SonarQube `S1168` & PMD `ReturnEmptyCollectionRatherThanNull`** ✅ | Flags methods returning nullable collections instead of empty collections. | **Adopt if rescoped to `nullable-collection-return`** (**Option 2c**). | Confirms that **return types** (not parameters or fields) are the high-signal target for nullable-collection linting. |
| **R7** | **Rust Clippy `clippy::ref_option` & `clippy::unnecessary_wraps`** ✅ | Flags `&Option<T>` (suggesting `Option<&T>`) and private functions whose bodies always return `Some(...)`. | **Reject** for `AvoidOptionalOrNoneRule`. | `ref_option` is about Rust reference indirection (`&Option<T>` vs `Option<&T>`), and `unnecessary_wraps` is a body control-flow check on private functions. |
| **R8** | **Omni's Own Codebase (`src/rule_selection.rs:122`, `scratch/legacy_reference/legacy_lints.py:1064`)** ✅ | Uses `select: Option<Vec<String>>` (`None` = all rules, `Some([])` = no rules) and `paths: Sequence[Path] \| None = None` (`None` = default dirs, `()` = 0 files). | **Adopt** as proof of why parameters/fields must not be flagged (**D2**) and why `RequireExplanation` is needed on returns (**G2**). | Demonstrates concrete, real-world tri-state collection semantics in both Rust and Python within this repository. |

---

## 2. Internal Codebase Architecture & AST Analysis

### 2.1 Existing Building Blocks in [src/code_lint/ast/python.rs](../../../src/code_lint/ast/python.rs)

If **Option 2c (`nullable-collection-return`)** is chosen, almost all required AST infrastructure already exists in [src/code_lint/ast/python.rs](../../../src/code_lint/ast/python.rs):

| Component | Existing Helper in `src/code_lint/ast/python.rs` | Notes for `nullable-collection-return` |
| :--- | :--- | :--- |
| Function signature & return node extraction | `extract_function_signatures(file)` ([L855–879](../../../src/code_lint/ast/python.rs#L855-L879)) | Provides `signature.name`, `signature.return_type_node`, and `signature.is_exempt_from_signature_rules()`. |
| Signature exemptions (`@override`, `@overload`, `@abstractmethod`, `Protocol`, `ABC`, dunders) | `PythonFunctionSignature::is_exempt_from_signature_rules()` ([L668–670](../../../src/code_lint/ast/python.rs#L668-L670)) | Identical exemption contract to `concrete-collection-return` and `mutable-collection-return`. |
| Unwrapping generic subscripts (`generic_type` & `subscript`) | `extract_generic_base_and_args(node)` ([L949–990](../../../src/code_lint/ast/python.rs#L949-L990)) | Extracts `(base_node, type_args)` cleanly across PEP 585 and `typing` subscripts. |
| Standard typing namespace check | `is_std_type_constructor_prefix(path, terminal)` ([L896–904](../../../src/code_lint/ast/python.rs#L896-L904)) | Matches unqualified or `typing.`, `typing_extensions.`, `collections.abc.`, `builtins.` prefixes. |
| Concrete & abstract collection recognition | `is_concrete_collection_constructor` ([L919–930](../../../src/code_lint/ast/python.rs#L919-L930)), `SINGLE_ARG_COVARIANT_CONTAINERS` ([L1006–1020](../../../src/code_lint/ast/python.rs#L1006-L1020)), `MUTABLE_COLLECTION_ABCS` ([L1156](../../../src/code_lint/ast/python.rs#L1156)) | Covers `list`, `dict`, `set`, `frozenset`, `deque`, `defaultdict`, `Counter`, `OrderedDict`, `Sequence`, `MutableSequence`, `Mapping`, `MutableMapping`, `Set`, `AbstractSet`, `MutableSet`, `Collection`, `Iterable`, `Reversible`. |

### 2.2 How Top-Level Nullable Collection Detection Works in Tree-Sitter Python (If Option 2c Is Chosen)

Unlike `concrete-collection-return` (which walks through `Optional[T]` and `T | None` as transparent wrappers to inspect `T`), `nullable-collection-return` checks the **interaction** between a union/optional wrapper and a collection branch at the return value's top level:

1. **Unwrap Return Envelope Qualifiers**:
   - Unwrap outer `type` and `parenthesized_expression` nodes.
   - Unwrap `Annotated[T, ...]` (taking arg 0).
   - Unwrap async return envelopes `Awaitable[T]` (taking arg 0) and `Coroutine[YieldT, SendT, ReturnT]` (taking arg 2), so functions returning `Awaitable[Sequence[str] | None]` are checked consistently with `async def fn() -> Sequence[str] | None`.
2. **Check for a Nullable Union at the Unwrapped Return Root**:
   - **Case A — `Optional[T]` (`typing.Optional[T]`, `typing_extensions.Optional[T]`)**:
     - The return root is explicitly nullable with inner type `T`. Collect union branches from `T` (in case of `Optional[list[int] | set[int]]`).
   - **Case B — PEP 604 `A | B` (`binary_operator` with `|` or `union_type`) or `Union[A, B, ...]`**:
     - Flatten top-level `|` / `Union[...]` / `Optional[...]` branches.
     - Check whether at least one branch is `None` (in `tree-sitter-python`, `None` inside a `type` annotation parses as node kind `"none"`).
3. **Verify Collection Branches**:
   - If the union contains `None` and every non-`None` branch is a collection type constructor:
     - Standard collection ABCs: `Sequence`, `MutableSequence`, `Mapping`, `MutableMapping`, `Set`, `AbstractSet`, `MutableSet`, `Collection`, `Iterable`, `Reversible`
     - Concrete collections: `list`, `List`, `dict`, `Dict`, `set`, `frozenset`, `FrozenSet`, `deque`, `Deque`, `defaultdict`, `DefaultDict`, `Counter`, `OrderedDict`
     - Homogeneous/variadic tuples: bare `tuple` / `Tuple`, or `tuple[T, ...]` / `Tuple[T, ...]` (where the second type argument is `ellipsis` `...`), while **excluding** fixed-length record tuples `tuple[int, str]` (whose length is fixed by the type system so `()` is not a valid inhabitant of `tuple[int, str]`).
   - Do **not** recurse into collection element types: `Sequence[int | None]` has no top-level `None` branch and is **not** flagged.

### 2.3 Registry, Naming, and Template Fit ([naming_and_message_style_guide.md](../naming_and_message_style_guide.md))

If **Option 2c (`nullable-collection-return`)** is adopted:
- **Rule name**: `nullable-collection-return` (3 words, kebab-case, names the flagged construct, parallels `concrete-collection-return` and `mutable-collection-return`).
- **File**: `src/code_lint/rules/nullable_collection_return.rs` (`pub const RULE: CodeRule`).
- **Classification**:
  - `topics: &[Topic::STATIC_TYPING]`
  - `precision: Precision::Exact` (syntactically exact check on the return annotation)
  - `consensus: Consensus::Opinionated` (tri-state returns such as cache misses are valid when explained)
  - `impacted_quality: ImpactedQuality::Maintainability`
- **Options**: `EnforcementMode::RequireExplanation` by default (matching `concrete-collection-return` and `mutable-collection-return`).
- **Candidate `ViolationTemplate`** (using only existing placeholders `{expression}`, `{function}`, `{token}` and existing verb `Remove` or `Replace`):
  - `summary`: `"Return annotation `{expression}` of `{function}` makes collection type `{token}` nullable."`
  - `rationale`: `"Wrapping a collection return type in `| None` or `Optional` creates two representations for an empty result and forces callers to check for `None` before iterating or querying length."`
  - `suggestion`: `"Remove `None` from the return annotation of `{function}` and return an empty collection such as `()`, `[]`, `{}`, or `frozenset()` when no elements are present."`

---

## 3. Configuration for Dropped Check 1 (`Optional[T]` Syntax Modernization) (`pyproject.toml`)

The legacy linter's Check 1 (banning `typing.Optional[T]` in favor of PEP 604 `T | None`) is dropped in Omni because Ruff's `pyupgrade` and `ruff` rules already enforce and auto-fix PEP 604 union syntax:

```toml
[tool.ruff]
target-version = "py310" # Required (>= py310) for UP007 and UP045 outside `from __future__ import annotations`

[tool.ruff.lint]
extend-select = [
    "UP007",  # non-pep604-annotation-union: rewrites `Union[A, B]` -> `A | B`
    "UP045",  # non-pep604-annotation-optional: rewrites `Optional[T]` -> `T | None`
    "RUF013", # implicit-optional: flags `x: int = None` and requires `x: int | None = None`
    "RUF036", # none-not-at-end-of-union: enforces `T | None` ordering rather than `None | T`
]
```

---

## 4. Sources

- Tony Hoare — *Null References: The Billion Dollar Mistake* (QCon London, 2009)
- PEP 484 — *Type Hints (`strict_optional` and `Optional[T]`)*: https://peps.python.org/pep-0484/
- PEP 604 — *Allow writing union types as `X | Y`*: https://peps.python.org/pep-0604/
- Joshua Bloch — *Effective Java, 3rd Edition*, Item 54 (*"Return empty collections or arrays, not nulls"*) & Item 55 (*"Return optionals judiciously"*)
- SonarSource `RSPEC-1168` (*"Empty arrays and collections should be returned instead of null"*): https://rules.sonarsource.com/java/RSPEC-1168/
- PMD Java Design Rules — `ReturnEmptyCollectionRatherThanNull`: https://pmd.github.io/pmd/pmd_rules_java_design.html#returnemptycollectionratherthannull
- Ruff Rules — `UP007`, `UP045`, `RUF013`, `RUF036`, `B006` (`mutable-argument-default`): https://docs.astral.sh/ruff/rules/
- Rust Clippy — `clippy::ref_option` & `clippy::unnecessary_wraps`: https://rust-lang.github.io/rust-clippy/master/index.html
