# Phase 1: Understand — Evaluating the legacy linter's `AvoidOptionalOrNoneRule`

This document records **Phase 1 (Understand)** of the exploration cycle for evaluating the legacy linter's `AvoidOptionalOrNoneRule` ([scratch/legacy_reference/legacy_lints.py](../../../scratch/legacy_reference/legacy_lints.py#L1982-L2019)) for Omni.

> **Status**: **VALIDATED** (2026-10-03). Decisions `D1`–`D5` and `Q1`–`Q4` accepted (proceed with Option B `nullable-collection-return`; `Precision::Heuristic`, `Consensus::Opinionated`, `ImpactedQuality::Maintainability`, `Topic::STATIC_TYPING`).

---

## 1. What the legacy linter's `AvoidOptionalOrNoneRule` Actually Does

In the legacy linter ([legacy_lints.py:1982-2019](../../../scratch/legacy_reference/legacy_lints.py#L1982-L2019)), `AvoidOptionalOrNoneRule` subclasses `AnnotationInspectorRule` ([legacy_lints.py:1189-1272](../../../scratch/legacy_reference/legacy_lints.py#L1189-L1272)):

1. **Inspected AST Sites (`AnnotationInspectorRule`)**:
   - Every function and async function definition (`ast.FunctionDef`, `ast.AsyncFunctionDef`) in non-test files, unless decorated with `@override`:
     - All positional-only, positional-or-keyword, keyword-only, `*args`, and `**kwargs` parameter annotations (`_inspect_function`, L1206–1222).
     - The return type annotation `node.returns` (`_inspect_function`, L1223–1224).
   - Every annotated assignment (`ast.AnnAssign`) where `is_attribute(node)` is true (`_inspect_ann_assign`, L1226–1245):
     - Any attribute target `obj.attr: <type>` anywhere in the file (not restricted to `self` or `__init__`).
     - Any class-body attribute `attr: <type>` (including private `_attr`, `ClassVar`, `Final`, `@dataclass` fields, `Protocol` attributes, `ABC` attributes, and `TypedDict` keys).

2. **Detection Logic (`AvoidOptionalOrNoneRule._inspect`, L1986–2018)**:
   - Walks **every** descendant AST node inside the annotation (`for sub in ast.walk(annotation):`) and sets `has_none = True` if any subtree matches:
     - `ast.BinOp` with `ast.BitOr` (`|`) where `ast.unparse(sub.left).strip() == "None"` or `ast.unparse(sub.right).strip() == "None"`.
     - `ast.Subscript` whose base unparses to `"Optional"` or `"typing.Optional"`.
     - `ast.Subscript` whose base unparses to `"Union"` or `"typing.Union"` and whose comma-split slice text contains `"None"`.
   - Emits:
     > `"{target_name} annotation includes nullable `None`. Consider avoiding `| None` / `Optional` when possible to keep interfaces strict (e.g., by using sentinel values, exceptions, or split methods)."`

3. **Execution Semantics in the legacy linter vs. Omni**:
   - In the legacy linter, `AvoidOptionalOrNoneRule` was registered in `_WARNING_RULES` ([legacy_lints.py:3678](../../../scratch/legacy_reference/legacy_lints.py#L3678)) and called `self.visitor.report_warning(...)` ([legacy_lints.py:571-574](../../../scratch/legacy_reference/legacy_lints.py#L571-L574)):
     - `report_warning` **did not increment `self.violations`**—it never failed CI or exited non-zero.
     - It **only printed on lines touched in the current `git diff`** (`lineno in self.changed_lines`).
     - It was completely disabled when `--no-warnings` or `legacy linter_LINT_NO_WARNINGS=1` was set.
   - **Empirical self-check on `legacy_lints.py`**: the legacy linter's own linter script ([legacy_lints.py](../../../scratch/legacy_reference/legacy_lints.py)) contains **18 function parameters and return annotations** using `| None` (e.g., `def _get_type_name(node: ast.AST) -> str | None` at L150, `changed_lines: AbstractSet[int] | None = None` at L530, `def get_adjacent_comment_text(self, lineno: int) -> str | None` at L576, `paths: Sequence[Path] | None = None` at L1064, `visited: set[str] | None = None` at L1125). The legacy linter only passed its own linter because `_WARNING_RULES` did not count as violations!
   - **In Omni**, there are no non-blocking diff-only warnings: every registered rule produces blocking diagnostics (either `EnforcementMode::Ban` or `EnforcementMode::RequireExplanation`) and must pass repo-wide linting.

---

## 2. Critical Signal-to-Noise Evaluation

### 2.1 Why Implementing `AvoidOptionalOrNoneRule` As-Is Fails Rule Design Principles

Evaluating the blanket rule against [Rule Design Guide](../rule_design_guide.md):

1. **Violates Rule Granularity ([Rule Design Guide §1](../rule_design_guide.md): *1 Rule = 1 Antipattern; The Split Test*)**:
   - An optional parameter (`timeout: float | None = None`), a nullable return type (`def find_user(id: UserId) -> User | None`), and a nullable attribute (`completed_at: datetime | None = None`) have completely different semantics, trade-offs, and remedies.
   - Moreover, because the legacy linter uses `ast.walk(annotation)`, it flags `None` inside nested type arguments that the function signature does not even make nullable at the top level—such as `Callable[[int | None], None]`, `Mapping[str, str | None]`, or `Sequence[Bound | None]`.

2. **Catastrophic False-Positive Rate on Idiomatic Python**:
   - **Parameters**:
     - **Python's mutable-default idiom (Ruff `B006` `mutable-argument-default` & `B008` `function-call-in-default-argument`)**: Because Python evaluates default argument expressions once at `def` time, any optional parameter whose fallback is a fresh mutable collection (`visited: set[str] | None = None`), a `Mapping` (`headers: Mapping[str, str] | None = None`, since `{}` is a mutable `dict` banned by `B006`), or a dynamically computed value (`now: datetime | None = None`, `paths: Sequence[Path] | None = None`) **must** be typed `T | None = None`.
     - **Optional configuration & bounds**: `timeout: timedelta | None = None` ("no timeout"), `max_items: int | None = None` ("unbounded"), `cursor: str | None = None` ("start from beginning").
   - **Return values**:
     - **Total lookup, search, and parsing functions**: `dict.get(key) -> V | None`, `re.search(...) -> Match[str] | None`, `next(it, None) -> T | None`, `find_order(id) -> Order | None`. Raising an exception on every expected cache miss or regex non-match violates standard Python design and forces `try / except LookupError:` control flow around ordinary queries.
   - **Class & instance attributes**:
     - **Optional domain state & lifecycle fields**: `closed_at: datetime | None = None`, `error_code: str | None = None`, `parent_id: NodeId | None = None`, and lazy caches (`_client: Client | None = None`).

3. **Creates Severe Perverse Incentives ([Rule Design Guide §3](../rule_design_guide.md))**:
   - The legacy linter's message actively suggests *"using sentinel values"* instead of `| None`.
   - In a statically typed language (Python with Mypy/Pyright/ty, or Rust with `Option<T>`), `T | None` is the **type-safe solution** to Tony Hoare's "billion-dollar mistake" (which was *implicit* nullability where `null` inhabited every reference type `T` unchecked).
   - When a linter penalizes `T | None`, developers replace `count: int | None = None` with `count: int = -1`, or `user_id: str | None = None` with `user_id: str = ""`. Unlike `T | None`—where the type checker forces callers to narrow via `if user_id is not None:`—sentinel values like `-1` and `""` have type `int` and `str`, **silently bypassing static type checking** and causing downstream arithmetic and logic bugs.

---

### 2.2 Evaluating the Narrower Rescope: Nullable Collections (`Collection[T] | None`)

A much narrower question is whether `AvoidOptionalOrNoneRule` should be rescoped to **nullable collection annotations** (`list[T] | None`, `Sequence[T] | None`, `Mapping[K, V] | None`, `AbstractSet[T] | None`, `Optional[Sequence[T]]`), where an empty collection (`()`, `frozenset()`, `{}`) already represents "zero elements" without needing a second empty state `None`.

Applying the **Split Test** ([Rule Design Guide §1](../rule_design_guide.md)) across the three annotation positions reveals a sharp asymmetry between **return types** on one hand and **parameters / attributes** on the other:

#### A. Nullable Collection Parameters (`items: Sequence[T] | None = None`) — **Low Signal / High Friction ❌**
Even when restricted to collections, flagging `param: CollectionType | None = None` on function parameters produces widespread false positives in Python for four structural reasons:
1. **Ruff `B006` (`mutable-argument-default`) on `Mapping` parameters**:
   - Python has no builtin immutable dictionary literal (`{}` constructs a mutable `dict`). Writing `def fetch(headers: Mapping[str, str] = {})` triggers Ruff `B006`. The standard, universal Python idiom for an optional read-only mapping parameter is `headers: Mapping[str, str] | None = None`.
2. **Ruff `B006` on mutable collection parameters (`MutableSequence`, `MutableMapping`, `MutableSet`, `list`, `dict`, `set`)**:
   - Any function that mutates an optional accumulator or visited set in place (such as `visited: MutableSet[str] | None = None` in [legacy_lints.py:1125](../../../scratch/legacy_reference/legacy_lints.py#L1125)) cannot default to `= set()` or `= []` without sharing mutated state across calls.
3. **Filter / Allowlist Semantics (`None` = "no filter / match all" vs. `()` = "match nothing")**:
   - In `def select_rules(include_tags: AbstractSet[str] | None = None)` (or Omni's own `select: Option<Vec<String>>` in [src/rule_selection.rs:122](../../../src/rule_selection.rs#L122)), `None` means *"no filter applied (include everything)"*, whereas an empty collection `frozenset()` / `()` means *"filter matches zero items"*. Replacing `| None = None` with `= ()` inverts the default behavior of the function.
4. **Computed Fallback & Partial-Update (PATCH) Semantics**:
   - In `def discover_protocol_signatures(paths: Sequence[Path] | None = None)` ([legacy_lints.py:1064](../../../scratch/legacy_reference/legacy_lints.py#L1064)), `paths=None` means *"scan default `src/` and `tests/` directories"*, whereas `paths=()` means *"scan zero files"*. Similarly, in update/PATCH methods (`tags: Sequence[str] | None = None`), `None` means *"leave existing tags unchanged"* while `()` means *"clear all tags"*.

#### B. Nullable Collection Attributes (`items: Sequence[T] | None = None`) — **Low Signal / High Friction ❌**
Class and instance attributes share the same legitimate tri-state patterns as parameters:
- Partial-update / PATCH DTOs and optional configuration filters (`select: Option<Vec<String>>`, `allowed_origins: Sequence[str] | None = None` where `None` = unconfigured/allow-all vs. `()` = allow-none).
- Lazy-loaded or cached collection attributes (`_cached_entries: Sequence[Entry] | None = None` where `None` = not yet fetched vs. `()` = fetched and empty).

#### C. Nullable Collection Return Types (`-> Sequence[T] | None`) — **High Signal & SOTA-Backed ✅**
In contrast to parameters and attributes, **return types** are where nullable collections are a well-known, high-signal antipattern:
1. **Direct SOTA Backing**:
   - **Effective Java (3rd ed.) Item 54**: *"Return empty collections or arrays, not nulls."*
   - **SonarQube `S1168`** & **PMD `ReturnEmptyCollectionRatherThanNull`**: *"Empty arrays and collections should be returned instead of null."*
2. **Why Return Types Differ from Parameters**:
   - Return annotations have **no default-argument evaluation**, so Ruff `B006` never forces `Mapping[K, V] | None` or `list[T] | None` on a return type—a function can always return `{}` or `[]` or `()` or `frozenset()` directly.
   - When a function returns `Sequence[T] | None` (or `list[T] | None`, `Mapping[K, V] | None`, `AbstractSet[T] | None`, `Iterable[T] | None`), every caller is forced to guard against `None` (`for x in get_items() or ():` or `if (items := get_items()) is not None:`) before iterating, indexing, or checking `len()`. Returning an empty collection (`()`, `[]`, `{}`, `frozenset()`) lets callers iterate and query length unconditionally.
3. **Handling Legitimate Tri-State Return Types**:
   - Occasionally, a function genuinely needs a tri-state return: e.g., `cache.get_items(key) -> Sequence[Item] | None` where `None` means *"cache miss"* and `()` means *"cache hit with zero items"*.
   - Just like `concrete-collection-return` and `mutable-collection-return`, setting the default enforcement mode of `nullable-collection-return` to **`EnforcementMode::RequireExplanation`** (configurable to `ban`) flags accidental nullable collection returns while allowing genuine tri-state returns when documented with an explanation comment above the function header.

---

## 3. Summary Comparison of Options & Recommendation

| Option | Description | Signal-to-Noise | Verdict |
| :--- | :--- | :--- | :--- |
| **Option 1: Implement As-Is** (`avoid-optional-or-none`) | Flag every `T \| None`, `Optional[T]`, `Union[..., None]` across all parameters, returns, and attributes. | **Extremely low (unusable as a blocking rule)**: flags almost every idiomatic Python file (18× in `legacy_lints.py` alone), collides with Ruff `B006`, and incentivizes untyped magic sentinels (`-1`, `""`). | **Reject ❌** |
| **Option 2a/2b: Nullable Collection Parameters / Attributes** | Flag `Collection[T] \| None` on parameters or class/instance attributes. | **Low**: collides with Ruff `B006` on `Mapping[K, V] \| None = None` and `MutableSequence[T] \| None = None`, and flags tri-state filters (`None` = match all vs. `()` = match none) and PATCH models. | **Reject ❌** |
| **Option 2c: Rescope to `nullable-collection-return`** | Flag Python return annotations whose top-level type is a nullable collection (`Sequence[T] \| None`, `list[T] \| None`, `Optional[Mapping[K, V]]`, etc.), with default `EnforcementMode::RequireExplanation`. | **High**: grounded in Effective Java Item 54 & SonarQube `S1168`, unaffected by `B006` default-arg constraints, and accommodates genuine tri-state returns via `require-explanation`. | **Recommended if a rule is desired ✅** |
| **Option 3: Drop Entirely** (or defer Option 2c to `ROADMAP.md`) | Do not port `AvoidOptionalOrNoneRule` at all; optionally record `nullable-collection-return` as a candidate rule in `ROADMAP.md`. | **Cleanest if we only want strict port parity for high-confidence error rules**: `AvoidOptionalOrNoneRule` was a non-blocking git-diff warning in the legacy linter, not an error rule. | **Viable alternative ✅** |

### Recommendation
1. **Drop the legacy linter's blanket `AvoidOptionalOrNoneRule`**—it must not be ported as a general `T | None` rule across parameters, returns, or attributes.
2. **Choose between Option 2c (implement `nullable-collection-return` now) and Option 3 (drop completely / record `nullable-collection-return` in `ROADMAP.md`)**:
   - If we want to salvage the high-signal core of `AvoidOptionalOrNoneRule`, rescope it to **`nullable-collection-return`** (Python, `RuleTarget::SourceOnly`, default `EnforcementMode::RequireExplanation`, `Precision::Exact`, `Consensus::Opinionated`, `ImpactedQuality::Maintainability`, `Topic::STATIC_TYPING`), flagging return annotations that wrap a collection type (`Sequence`, `Mapping`, `Set`/`AbstractSet`, `Iterable`, `Collection`, `Reversible`, `MutableSequence`, `MutableMapping`, `MutableSet`, `list`, `dict`, `set`, `tuple`, `frozenset`, `deque`, `defaultdict`, `Counter`, `OrderedDict`) in `| None`, `Optional[...]`, or `Union[..., None]` at the top level (or inside `Annotated[...]`, `Awaitable[...]`, `Coroutine[..., ..., Ret]`).
   - If we prefer not to add a new rule for a legacy the legacy linter warning rule, **drop `AvoidOptionalOrNoneRule` completely** (and optionally note `nullable-collection-return` under Candidate Rules in `ROADMAP.md`).

---

## 4. Goals & Explicit Non-Goals (If Rescoped to `nullable-collection-return`)

### Goals
- **G1 — Eliminate Two-State Empty Collection Returns**: Flag Python return annotations where a collection or iterable type is unioned with `None` (`Sequence[T] | None`, `Optional[list[T]]`, etc.), guiding authors to return an empty collection (`()`, `[]`, `{}`, `frozenset()`) instead.
- **G2 — Support Legitimate Tri-State Return Contracts**: Default to `EnforcementMode::RequireExplanation` (matching `concrete-collection-return` and `mutable-collection-return`) so functions where `None` means "not found / cache miss / uninitialized" (distinct from "0 items") can document that contract in a header comment.
- **G3 — Reuse Existing `ast::python` Signature & Annotation Infrastructure**: Reuse `extract_function_signatures`, `PythonFunctionSignature::is_exempt_from_signature_rules`, and the type constructor helpers in [src/code_lint/ast/python.rs](../../../src/code_lint/ast/python.rs).
- **G4 — Zero False Positives on Non-Collection Optionals and Inner Element Nullability**: Never flag scalar/object optionals (`User | None`, `str | None`, `int | None`, `tuple[int, str] | None` fixed-shape heterogeneous records) or collections of nullable elements (`Sequence[int | None]`, `Mapping[str, int | None]`).

### Explicit Non-Goals
- **NG1 — Flagging General `T | None` on Scalars, Domain Objects, or Fixed-Shape Tuples**: `str | None`, `User | None`, and fixed-length record tuples `tuple[str, int] | None` (as opposed to variadic homogeneous `tuple[T, ...] | None`) have no "empty collection" identity element.
- **NG2 — Flagging Nullable Collection Parameters or Attributes**: Excluded due to Ruff `B006` mutable-default constraints (`Mapping[K, V] | None = None`, `MutableSet[T] | None = None`) and ubiquitous filter/PATCH tri-state semantics (`select: Sequence[str] | None = None`).
- **NG3 — Flagging `Iterator[T] | None` / `Generator[...] | None`**: Unlike `Sequence` or `Iterable`, a function that uses `yield` is always a generator function (returning a generator object even if it returns early); an explicit `Iterator[T] | None` is rare and distinct from a collection container. (See `Q3`.)

---

## 5. Numbered Decisions (`D1`–`D5`)

- **D1 — Reject Blanket `AvoidOptionalOrNoneRule`**: Do not port the legacy linter's blanket `T | None` check across parameters, returns, or attributes.
- **D2 — Reject Nullable-Collection Checks on Parameters and Attributes**: Do not flag `Collection[T] | None` on function parameters or class/instance attributes because of `B006` default-argument rules and filter/PATCH semantics.
- **D3 — Target Scope (if Option 2c is selected)**: `nullable-collection-return` targets Python (`SupportLang::Python`, `RuleTarget::SourceOnly`), with default `EnforcementMode::RequireExplanation`.
- **D4 — Top-Level Return Nullability Only (if Option 2c is selected)**: Only inspect whether the return type itself (or the awaited result inside `Awaitable[...]` / `Coroutine[Any, Any, ...]` or `Annotated[..., ...]`) is a union containing both `None` and at least one collection type. Never flag non-nullable collections whose inner element types are nullable (`Sequence[int | None]`, `Mapping[str, Value | None]`).
- **D5 — Fixed-Length Tuple Exemption (if Option 2c is selected)**: Distinguish homogeneous variadic tuples (`tuple[T, ...] | None`, which act as immutable sequences and have `()` as their empty value) from fixed-length heterogeneous record tuples (`tuple[int, str] | None`, such as `legacy_lints.py:2742` `-> tuple[str, str] | None`, which represent a 2-field record that is either present or `None` and cannot be replaced by `()` without breaking unpacking `a, b = fn()`).

---

## 6. Open Questions for User Validation (`Q1`–`Q4`)

- **Q1 (Primary Decision — Drop vs. Rescope to `nullable-collection-return`)**:
  - Should we **(A) Drop `AvoidOptionalOrNoneRule` completely** (and optionally list `nullable-collection-return` in `ROADMAP.md`), or **(B) Implement the rescoped `nullable-collection-return` rule** (Option 2c)?
- **Q2 (Covered Collection Types in `nullable-collection-return`, if Option B)**:
  - Should `nullable-collection-return` match:
    1. Read-only & mutable ABCs: `Sequence`, `MutableSequence`, `Mapping`, `MutableMapping`, `Set`, `AbstractSet`, `MutableSet`, `Collection`, `Iterable`, `Reversible`
    2. Concrete collections: `list`, `List`, `dict`, `Dict`, `set`, `frozenset`, `FrozenSet`, `deque`, `Deque`, `defaultdict`, `DefaultDict`, `Counter`, `OrderedDict`
    3. Variadic tuples `tuple[T, ...]` / `Tuple[T, ...]` (and bare `tuple` / `Tuple`), while **exempting** fixed-length record tuples `tuple[T1, T2]` (where `()` would not type-check against `a, b = fn()`)?
- **Q3 (Async Return Wrapper Unwrapping, if Option B)**:
  - For `async def` or functions returning `Awaitable[Sequence[T] | None]` / `Coroutine[Any, Any, Sequence[T] | None]`, should the rule unwrap `Annotated`, `Awaitable`, and `Coroutine` return positions (matching `concrete-collection-return` and `mutable-collection-return`)?
- **Q4 (Multi-Type Unions, if Option B)**:
  - Should `Sequence[str] | int | None` (a union of a collection, a non-collection, and `None`) be flagged or exempted? (Exempting multi-branch unions that contain a non-collection type `T_other` avoids flagging discriminated unions like ` str | Sequence[str] | None`, flagging only when the non-`None` union branches are all collection types.)
