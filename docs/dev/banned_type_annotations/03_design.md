# Phase 3: Design — `BannedTypeAnnotationsRule` & Adjacent Checks

> **Status:** VALIDATED — DROPPED (user decision: no rule; see 03_design.md §0).

## 0. User Decision

All four checks are dropped and recorded under *Not pursued* in [ROADMAP.md](../../../ROADMAP.md), including the `concrete-coroutine-return` fallback (§4). A user-configurable banned-type list is not unsound in itself, but it is not pursued either: Ruff `banned-api` (`TID251`, `[tool.ruff.lint.flake8-tidy-imports.banned-api]`) already bans any qualified name with a custom message, a user-supplied list carries no rationale, examples or `--explain` documentation, and shape-based bans (such as positional tuples) are better served by dedicated rules like `positional-tuple-return`.

## 1. Primary Recommendation: Drop All Four Candidate Checks

Based on Phase 1 and Phase 2 analysis, **all four legacy checks in this cluster should be dropped** (0 new rules added to `omnicoach`):

1. **`BannedTypeAnnotationsRule` (`banned-type-annotations`)**:
   - **5 domain tuple shapes** (`Dimensions`, `ValueRange`, `MergeInputs`, `ClientSetup`, `PendingRequest`): 100% proprietary to the legacy linter's project domain. Their general structural principle (banning unnamed multi-element `tuple[T1, T2, ...]` in return annotations) is already enforced by [positional_tuple_return.rs](../../../src/code_lint/rules/positional_tuple_return.rs).
   - **User-configurable banned type list**: Violates `Simplicity First` (no speculative configurability with an empty default list), `every_tag_has_a_rule`, and `RuleOptions` (which supports only `()`, `CountOption`, or `ListOption` of strings, not `(ast_pattern -> replacement_message)` dictionaries).
   - **`Coroutine[object, object, T]` check**: Unsound as a `ban`-level rule because `asyncio.create_task()`, `TaskGroup.create_task()`, `loop.create_task()`, and `asyncio.run()` are typed in `typeshed` (and enforced at CPython runtime) as requiring `Coroutine[Any, Any, _T]` and **rejecting `Awaitable[_T]`**. Replacing `Coroutine[..., ..., T]` with `Awaitable[T]` on any helper that feeds `create_task()` breaks `mypy` and `pyright`, while replacing `Coroutine[object, object, T]` with `Coroutine[None, None, T]` is arbitrary churn on unused generator type parameters.
2. **`PrimitiveTypeSuggestionsRule` (`primitive-type-suggestions`)**:
   - Already disabled in the legacy linter due to near-100% false-positive rate on ordinary `str`/`int`/`float` annotations.
   - Its high-signal structural subsets are already implemented in Omni by [primitive_duration.rs](../../../src/code_lint/rules/primitive_duration.rs), [identical_positional_types.rs](../../../src/code_lint/rules/identical_positional_types.rs), and [positional_tuple_return.rs](../../../src/code_lint/rules/positional_tuple_return.rs).
3. **`FloatToDecimalSuggestionsRule` (`float-to-decimal-suggestions`)**:
   - Domain-specific to monetary calculations; `float` is the standard Python type for non-monetary continuous values.
4. **`DecimalZeroFormattingRule` (`decimal-zero-formatting`)**:
   - Semantically buggy for `Decimal("0.00")`, where the string literal encodes IEEE 754 decimal exponent `-2` (quantum `0.01`) required by `.quantize(Decimal("0.00"))` and fixed-scale string formatting (`str(Decimal("0.00")) == "0.00"` vs `str(Decimal(0)) == "0"`).

---

## 2. Numbered Decisions (`D1..D6`)

- **D1 — Drop the 5 project-domain tuple matchers in `BannedTypeAnnotationsRule`**: The tuple patterns (`Dimensions`, `ValueRange`, `MergeInputs`, `ClientSetup`, `PendingRequest`) reference legacy-only domain types and are already covered structurally on return types by `positional-tuple-return`.
- **D2 — Do not add an empty-by-default configurable `banned-type-annotation` rule**: Adding a speculative user-configured AST type-pattern matcher violates `Simplicity First`, `every_tag_has_a_rule`, and the `RuleOptions` architecture.
- **D3 — Recommend dropping the `Coroutine[object, object, T]` sub-check (with a narrowed `concrete-coroutine-return` fallback in Section 4)**: Because `asyncio.create_task()`, `TaskGroup.create_task()`, `loop.create_task()`, and `asyncio.run()` reject `Awaitable[T]` in `typeshed` and at runtime, banning `Coroutine[_, _, T]` in parameters or task-factory returns forces developers into `mypy`/`pyright` errors or `# noqa` suppressions.
- **D4 — Drop `PrimitiveTypeSuggestionsRule`**: Already disabled in the legacy linter; its actionable subsets are covered by `primitive-duration`, `identical-positional-types`, and `positional-tuple-return`.
- **D5 — Drop `FloatToDecimalSuggestionsRule`**: Domain-specific to monetary arithmetic; not a general Python lint rule.
- **D6 — Drop `DecimalZeroFormattingRule`**: Factually incorrect for `Decimal("0.00")` (destroys decimal scale/exponent `-2` used by `.quantize()` and `str()`) and micro-style for `Decimal("0")`.

---

## 3. Open Questions (`Q1..Q2`)

- **Q1 — Confirm full drop vs. fallback `concrete-coroutine-return` rule**: Do you agree with dropping `BannedTypeAnnotationsRule` completely (given `positional-tuple-return` and the `asyncio.create_task` `Coroutine` requirement in `typeshed`), or would you like to implement the narrowed `concrete-coroutine-return` fallback specified in Section 4?
- **Q2 — Confirm no `positional-tuple-parameter` / `positional-tuple-attribute` expansion**: `positional-tuple-return` intentionally scopes fixed-length tuple bans to return/yield annotations (since 2-tuples and 3-tuples like `bounds: tuple[float, float]` or `point: tuple[int, int]` are common in parameters and attributes). Confirm that we should keep positional tuple checks scoped to returns only.

---

## 4. Fallback Design (Only If User Elects to Keep the `Coroutine` Check in `Q1`)

If you prefer to keep a narrowed version of Pattern 6 (`Coroutine[Yield, Send, Return]` with placeholder `Yield`/`Send` types) rather than dropping `BannedTypeAnnotationsRule` entirely, the cleanest high-signal design scopes the check to **return annotations** (mirroring `concrete-collection-return` and `nullable-collection-return`), where synchronous wrapper functions returning an unawaited coroutine should return `Awaitable[T]` (or be `async def -> T`).

### 4.1 Rule Metadata (`RuleDeclaration`)

- **Name**: `concrete-coroutine-return`
- **Category**: `code_lint` (`Check::ConcreteCoroutineReturn`)
- **Tags**:
  - `Language::Python`
  - `Scope::Production`
  - `Stage::Precommit`
  - `Gate::Ban`
  - `Concern::ApiContract`
  - `Construct::Return`
  - `construct::TypeAnnotation`
  - `Dependency::AstOnly`
  - `FixSafety::NeedsReview`
- **Summary**: `"Ban concrete Coroutine[Yield, Send, Return] with placeholder yield/send types in return annotations"`
- **Why**: `"Coroutine[YieldType, SendType, ReturnType] exposes legacy generator-coroutine send and yield channels that native async def coroutines never use, coupling callers to a 3-argument concrete type instead of the 1-argument Awaitable[ReturnType] protocol."`
- **Fix**: `"Replace Coroutine[..., ..., T] in the return annotation with collections.abc.Awaitable[T], or convert the function to async def returning T."`
- **ViolationTemplate**:
  - `problem`: `"Function '{name}' annotates its return type with concrete '{found}' instead of 'Awaitable'"`
  - `fix`: `"Replace '{found}' with 'Awaitable[{return_type}]', or make '{name}' an 'async def' returning '{return_type}'"`

### 4.2 AST Trigger Contract

1. Inspect `StmtFunctionDef` and `StmtAsyncFunctionDef` nodes that have a return annotation (`func.returns`).
2. Check function-level and enclosing-class exemptions (`E3`–`E8` below) via `returns::should_skip_return_contract_check`.
3. Walk return-type targets (`Union` arms, `Optional[T]`, `Annotated[T, ...]`; note: do **not** unwrap `Coroutine` inside `for_each_return_type_target` before checking the `Coroutine` subscript itself).
4. Match `ast::Expr::Subscript` where:
   - `value` is `Coroutine` (`Name("Coroutine")` or `Attribute(..., attr="Coroutine")`),
   - `slice` is a 3-element `Expr::Tuple` (`[yield_ty, send_ty, return_ty]`), and
   - both `yield_ty` and `send_ty` (after `unwrap_annotated`) are placeholder types (`None`, `object`, or `Any`, including `typing.Any`).
5. Report one violation per matching `Coroutine[...]` subscript in the return annotation.

### 4.3 Exemptions (`E1..E9`)

| ID | Exemption | Rationale |
|---|---|---|
| **E1** | `Awaitable[T]` return annotation | Already uses the abstract `Awaitable` protocol. |
| **E2** | `Coroutine[YieldT, SendT, ReturnT]` where `YieldT` or `SendT` is a concrete domain type (neither `Any`, `object`, nor `None`) | Legitimate generator-based coroutine protocol that genuinely yields or receives typed values via `.send()`. |
| **E3** | `@override`-decorated method (`functions::has_override_decorator`) | Signature is locked by a base class or external interface (`should_skip_return_contract_check`). |
| **E4** | `@overload`-decorated signature (`functions::has_overload_decorator`) | Typing stub signature paired with an implementation (`should_skip_return_contract_check`). |
| **E5** | `@abstractmethod`-decorated method (`functions::has_abstractmethod_decorator`) | Abstract contract definition (`should_skip_return_contract_check`). |
| **E6** | Method directly inside a `Protocol` or `ABC`/`ABCMeta` class (`classes::is_contract_base_class`) | Contract base class definition (`should_skip_return_contract_check`). |
| **E7** | Dunder method (`annotations::is_dunder_name`) | Python data-model protocol methods have fixed signatures (`should_skip_return_contract_check`). |
| **E8** | `@pytest.fixture`-decorated function (`annotations::has_pytest_fixture_decorator`) | Pytest fixture injection contract (`should_skip_return_contract_check`). |
| **E9** | Parameter and attribute annotations (`def f(coro: Coroutine[Any, Any, int]):`) | Out of scope for return-only rule; parameters often pass `coro` to `asyncio.create_task(coro)`, which rejects `Awaitable[T]`. |

### 4.4 Planned Test Grid (Fallback Only)

#### `fail` Cases

- `coroutine_object_object_return`: `def fetch() -> Coroutine[object, object, int]: ...`
- `coroutine_any_any_return`: `def fetch() -> Coroutine[Any, Any, str]: ...`
- `coroutine_none_none_return`: `def fetch() -> Coroutine[None, None, None]: ...`
- `qualified_collections_abc_coroutine_return`: `def fetch() -> collections.abc.Coroutine[Any, Any, int]: ...`
- `union_coroutine_or_none_return`: `def maybe_fetch() -> Coroutine[object, object, int] | None: ...`
- `annotated_coroutine_return`: `def fetch() -> Annotated[Coroutine[Any, Any, int], "meta"]: ...`

#### `pass` Cases (1-to-1 with `E1..E9`)

- `awaitable_return_exempt` (**E1**): `def fetch() -> Awaitable[int]: ...`
- `typed_yield_send_coroutine_exempt` (**E2**): `def gen_coro() -> Coroutine[int, str, bool]: ...`
- `override_method_exempt` (**E3**): `@override\ndef fetch(self) -> Coroutine[Any, Any, int]: ...`
- `overload_signature_exempt` (**E4**): `@overload\ndef fetch(x: int) -> Coroutine[Any, Any, int]: ...`
- `abstractmethod_exempt` (**E5**): `@abstractmethod\ndef fetch(self) -> Coroutine[Any, Any, int]: ...`
- `protocol_method_exempt` (**E6**): `class Fetcher(Protocol):\n    def fetch(self) -> Coroutine[Any, Any, int]: ...`
- `dunder_method_exempt` (**E7**): `def __call__(self) -> Coroutine[Any, Any, int]: ...`
- `pytest_fixture_exempt` (**E8**): `@pytest.fixture\ndef coro_factory() -> Coroutine[Any, Any, int]: ...`
- `parameter_coroutine_exempt` (**E9**): `def spawn(coro: Coroutine[Any, Any, int]) -> Task[int]: ...`
