# Phase 2: References & Prior Art — `BannedTypeAnnotationsRule` & Adjacent Checks

> **Status:** VALIDATED — DROPPED (user decision: no rule; see 03_design.md §0).

## 1. External Linters and Type Checkers

| Tool / Rule | What It Checks | Relevance & Takeaway |
|---|---|---|
| **Ruff `TID251`** (`banned-api`, `flake8-tidy-imports`) | Bans importing or referencing user-configured fully-qualified symbol paths (`[tool.ruff.lint.flake8-tidy-imports.banned-api]`). | Empty by default. Operates on module/symbol paths rather than composite subscript shapes (`tuple[A, B]` or `Coroutine[object, object, T]`). Projects that want to ban a specific symbol (`decimal.Decimal`, `typing.Any`, etc.) already have `TID251`. |
| **Ruff `ANN401`** (`any-type`, `flake8-annotations`) | Bans `typing.Any` in function parameter and return annotations. | Direct catalyst for the legacy linter's `Coroutine[object, object, T]` pattern: because `mypy` and `pyright` infer `Coroutine[Any, Any, T]` for calls to `async def` functions, developers under `ANN401` replace `Any` with `object`. |
| **Ruff `PYI058`** (`generator-return-from-iter-method`, `flake8-pyi`) | Flags `__iter__` / `__aiter__` methods returning concrete `Generator[...]` / `AsyncGenerator[...]` instead of `Iterator[...]` / `AsyncIterator[...]`. | Closest Ruff analogue to preferring an abstract protocol over a generator/coroutine implementation type. Notably, Ruff scopes `PYI058` strictly to `__iter__`/`__aiter__` and does **not** ban `Coroutine[_, _, T]` returns globally because `asyncio.create_task` and `asyncio.run` require `Coroutine`. |
| **Ruff `UP006` / `UP035`** (`pyupgrade`) | Rewrites `typing.Tuple` → `tuple` and `typing.Coroutine` / `typing.Awaitable` → `collections.abc.Coroutine` / `collections.abc.Awaitable`. | Handles import modernization only; does not alter `Coroutine` vs `Awaitable` or tuple arity. |
| **Mypy / Pyright & `typeshed` (`asyncio`)** | Standard library stubs for `asyncio.create_task`, `TaskGroup.create_task`, `AbstractEventLoop.create_task`, and `asyncio.run`. | All four functions are typed in `typeshed` as accepting `Coroutine[Any, Any, _T]`, **not** `Awaitable[_T]` (matching CPython's runtime `coroutines.iscoroutine` check). Replacing `Coroutine[..., ..., T]` with `Awaitable[T]` on any value passed to `create_task` or `asyncio.run` causes a hard `mypy`/`pyright` type error. |
| **Clippy `clippy::disallowed_types`** | User-configured list of banned resolved type paths with custom replacement messages in `clippy.toml`. | Empty by default; requires a configuration DSL mapping type paths to reason strings. Incompatible with Omni's zero-config `every_tag_has_a_rule` design and `RuleOptions` model. |
| **Pylint / Flake8 / Refurb** | General Python static analysis suites. | None include a rule banning `Coroutine[object, object, T]`, banning `float` in favor of `Decimal`, or rewriting `Decimal("0")` / `Decimal("0.00")` to `Decimal(0)`. |

---

## 2. Existing Rules and Helpers in `omnicoach`

### 2.1 Sibling Rules in `src/code_lint/rules/`

| Omni Rule | File | Overlap with Legacy Candidates |
|---|---|---|
| `positional-tuple-return` | [positional_tuple_return.rs](../../../src/code_lint/rules/positional_tuple_return.rs) | Bans fixed-length `tuple[T1, T2, ...]` (`arity >= 2`) in return and generator yield/return annotations (unwrapping `\| None`, `Optional`, `Union`, `Awaitable`, `Coroutine`, `Annotated`, and `Iterator`/`Generator` wrappers). **Completely supersedes** the 5 hardcoded domain tuple shapes in `BannedTypeAnnotationsRule` on return types. |
| `concrete-collection-parameter`, `concrete-collection-return`, `concrete-collection-attribute` | [concrete_collection_parameter.rs](../../../src/code_lint/rules/concrete_collection_parameter.rs), [concrete_collection_return.rs](../../../src/code_lint/rules/concrete_collection_return.rs), [concrete_collection_attribute.rs](../../../src/code_lint/rules/concrete_collection_attribute.rs) | Enforce abstract protocols (`Sequence`, `Mapping`, `AbstractSet`) over concrete containers (`list`, `dict`, `set`) across parameters, returns, and public class attributes, with full contract exemptions (`@override`, `@overload`, `@abstractmethod`, `Protocol`/`ABC`, dunders, fixtures, click/typer, Pydantic/ORM, `TypedDict`). |
| `primitive-duration` | [primitive_duration.rs](../../../src/code_lint/rules/primitive_duration.rs) | Bans primitive `int`/`float` annotations on time/duration-named parameters, returns, and attributes in favor of `datetime.timedelta`. **Supersedes** the duration-type subset of `PrimitiveTypeSuggestionsRule`. |
| `identical-positional-types` | [identical_positional_types.rs](../../../src/code_lint/rules/identical_positional_types.rs) | Bans functions with `>= 2` positional parameters sharing the same type (`def f(a: str, b: str)`). **Supersedes** the swappable-primitive-parameter hazard targeted by `PrimitiveTypeSuggestionsRule`. |

### 2.2 Shared AST Infrastructure in `src/code_lint/ast/`

If any annotation rule were added or extended, all necessary AST helpers already exist:

- [annotations.rs](../../../src/code_lint/ast/python/annotations.rs): `unwrap_annotated`, `contains_type_subscript`, `is_subscript_named`, `has_annotated_metadata`, `is_pydantic_model_class`, `is_orm_model_class`, `is_typed_dict_class`, `has_pytest_fixture_decorator`, `has_framework_Preserve_decorator`, `is_dunder_name`.
- [returns.rs](../../../src/code_lint/ast/python/returns.rs): `for_each_return_type_target` (unwraps `Union`, `Optional`, `Awaitable`, `Coroutine`, and `Annotated` around return annotations), `should_skip_return_contract_check`.
- [functions.rs](../../../src/code_lint/ast/python/functions.rs) and [classes.rs](../../../src/code_lint/ast/python/classes.rs): `has_override_decorator`, `has_overload_decorator`, `has_abstractmethod_decorator`, `is_contract_base_class`.

---

## 3. Verdict per Candidate Check

| Legacy Rule / Component | Verdict | Rationale |
|---|---|---|
| **`BannedTypeAnnotationsRule` (5 domain tuple shapes)** | **Drop (superseded by `positional-tuple-return`)** | The 5 shapes (`Dimensions`, `ValueRange`, `MergeInputs`, `ClientSetup`, `PendingRequest`) are hardcoded to the legacy linter's project domain. Their generalizable core—banning unnamed multi-element positional tuples in return types—is already implemented in `positional-tuple-return`. |
| **`BannedTypeAnnotationsRule` (`Coroutine[object, object, ...]`)** | **Drop** *(fallback narrowed design documented in `03_design.md` if requested)* | `Coroutine[Any, Any, T]` (or `Coroutine[object, object, T]` under `ANN401`) is **required** by `typeshed`, `mypy`, and `pyright` whenever a coroutine object is passed to `asyncio.create_task()`, `TaskGroup.create_task()`, `loop.create_task()`, or `asyncio.run()`, all of which reject `Awaitable[T]`. Suggesting `Coroutine[None, None, T]` instead is arbitrary churn on phantom generator type parameters. |
| **`PrimitiveTypeSuggestionsRule`** | **Drop (superseded)** | Already disabled in the legacy linter due to extreme false positives; high-signal cases are covered by `primitive-duration`, `identical-positional-types`, and `positional-tuple-return`. |
| **`FloatToDecimalSuggestionsRule`** | **Drop** | Domain-specific to monetary calculations; `float` is the standard Python type for non-monetary continuous quantities. |
| **`DecimalZeroFormattingRule`** | **Drop** | Factually unsound for `Decimal("0.00")`, where the string literal sets IEEE 754 decimal exponent `-2` (required by `.quantize(Decimal("0.00"))` and fixed-point formatting), whereas `Decimal(0)` has exponent `0`. |
