# Phase 2: References & Prior Art — `BannedTypeAnnotationsRule` & Positional-Tuple Symmetry

> **Status:** VALIDATED — Building `positional-tuple-parameter`, `positional-tuple-attribute`, and `adhoc-type-annotation` across Python and Rust (see [03_design.md](03_design.md)).

## 1. External Linters and Type Checkers

| Tool / Rule | What It Checks | Relevance & Takeaway |
|---|---|---|
| **Ruff `TID251`** (`banned-api`, `flake8-tidy-imports`) | Bans importing or referencing user-configured fully-qualified symbol paths (`[tool.ruff.lint.flake8-tidy-imports.banned-api]`). | Empty by default. Operates on module/symbol paths rather than composite subscript/generic type shapes (`tuple[A, B]`, `Generator[_, None, None]`, or `(Width, Height)`). Projects that want to ban a specific symbol (`decimal.Decimal`, `typing.Any`, etc.) have `TID251`, whereas composite type-annotation shapes require structural AST matching (`adhoc-type-annotation`). |
| **Ruff `ANN401`** (`any-type`, `flake8-annotations`) | Bans `typing.Any` in function parameter and return annotations. | Direct catalyst for the legacy linter's `Coroutine[object, object, T]` pattern: because `mypy` and `pyright` infer `Coroutine[Any, Any, T]` for calls to `async def` functions, developers under `ANN401` replace `Any` with `object`. |
| **Ruff `PYI058`** (`generator-return-from-iter-method`, `flake8-pyi`) | Flags `__iter__` / `__aiter__` methods returning concrete `Generator[...]` / `AsyncGenerator[...]` instead of `Iterator[...]` / `AsyncIterator[...]`. | Closest Ruff analogue to preferring an abstract iterator protocol over a concrete `Generator[T, None, None]` annotation. Notably, Ruff does **not** ban `Coroutine[_, _, T]` in favor of `Awaitable[T]` because `asyncio.create_task` and `asyncio.run` require `Coroutine`. Making `adhoc-type-annotation` opt-in (`base: &[]`) avoids false positives while letting projects configure sound generic patterns such as `"Generator[_, None, None] => collections.abc.Iterator"`. |
| **Ruff `UP006` / `UP035`** (`pyupgrade`) | Rewrites `typing.Tuple` → `tuple` and `typing.Coroutine` / `typing.Awaitable` → `collections.abc.Coroutine` / `collections.abc.Awaitable`. | Handles import modernization only; does not check tuple arity or composite type shapes. |
| **Clippy `clippy::type_complexity` & `clippy::disallowed_types`** | `type_complexity` flags deeply nested types by an AST score; `disallowed_types` bans user-configured resolved type paths with custom replacement messages in `clippy.toml`. | `type_complexity` does not flag simple 2-tuples `(String, u16)` in parameters or struct fields, and `disallowed_types` matches resolved type paths rather than structural tuple/generic shapes like `(Width, Height)`. |
| **Effective Python (Item 19 / Item 37) & Rust API Guidelines (`C-NEWTYPE`, Structs)** | Recommend `NamedTuple` / `@dataclass(frozen=True)` in Python and named-field `struct`s in Rust whenever a composite value carries multiple positional elements across function or data-structure boundaries. | Directly motivates completing the `positional-tuple-*` family across parameters (`positional-tuple-parameter`) and attributes/fields (`positional-tuple-attribute`) alongside `positional-tuple-return`. |

---

## 2. Existing Rules and Helpers in `omnicoach`

### 2.1 Sibling Rules in `src/code_lint/rules/`

| Omni Rule | File | Relationship to New Rules |
|---|---|---|
| `positional-tuple-return` | [positional_tuple_return.rs](../../../src/code_lint/rules/positional_tuple_return.rs) | Bans fixed-length `tuple[T1, T2, ...]` / `(T1, T2, ...)` (`arity >= min-elements`, default `2`) in return and generator yield/return annotations across Python and Rust. `positional-tuple-parameter` and `positional-tuple-attribute` complete this family for parameters and attributes/fields with identical `min-elements = 2` and classification (`Topic::POSITIONAL_MEANING`, `Precision::Exact`, `Consensus::Opinionated`, `ImpactedQuality::Maintainability`). |
| `concrete-collection-parameter`, `concrete-collection-return`, `concrete-collection-attribute` | [concrete_collection_parameter.rs](../../../src/code_lint/rules/concrete_collection_parameter.rs), [concrete_collection_return.rs](../../../src/code_lint/rules/concrete_collection_return.rs), [concrete_collection_attribute.rs](../../../src/code_lint/rules/concrete_collection_attribute.rs) | Establish the parameter / return / attribute rule triad and contract exemptions (`is_exempt_from_signature_rules` on functions, `!param.is_variadic() && param.kind != PythonParameterKind::Receiver` on parameters, `!class.is_interface()` on classes), and anchor parameter/attribute findings on `type_node`. |
| `repeated-index-access` | [repeated_index_access.rs](../../../src/code_lint/rules/repeated_index_access.rs) | Flags reading multiple positional indices (`span[0]`, `span[1]`; `span.0`, `span.1`) on the consumer side; its suggestion explicitly points to `NamedTuple` / dataclass / struct when a tuple crosses a boundary. |
| `primitive-duration` & `identical-positional-types` | [primitive_duration.rs](../../../src/code_lint/rules/primitive_duration.rs), [identical_positional_types.rs](../../../src/code_lint/rules/identical_positional_types.rs) | Cover the actionable subsets of the dropped `PrimitiveTypeSuggestionsRule`. |

### 2.2 Shared AST & Semantic Infrastructure in `src/code_lint/`

- **Python AST**:
  - [annotations.rs](../../../src/code_lint/ast/python/annotations.rs): `return_type_union`, `PythonReturnTypeBranch::positional_tuple_arity`, `collect_collection_types`, `TRANSPARENT_UNION_WRAPPERS` (`Optional`, `Union`), `TRANSPARENT_FIRST_ARG_WRAPPERS` (`Annotated`, `ClassVar`, `Final`, `Required`, `NotRequired`, `ReadOnly`), `extract_generic_base_and_args`.
  - [functions.rs](../../../src/code_lint/ast/python/functions.rs): `extract_function_signatures`, `PythonFunctionSignature`, `PythonParameterInfo`, `PythonParameterKind`, `has_imposed_signature`.
  - [classes.rs](../../../src/code_lint/ast/python/classes.rs): `extract_classes`, `PythonClassInfo::annotated_attributes`, `PythonClassInfo::instance_attribute_annotations`, `PythonClassInfo::is_interface`, `is_interface_method`.
- **Rust AST**:
  - [returns.rs](../../../src/code_lint/ast/rust/returns.rs): `collect_function_returns`, `is_trait_or_trait_impl_function`, `option_payload`, `unwrap_rust_pointer_wrappers`, `extract_path_type_generics`, `normalize_type_path_text`.
- **Import Resolution**:
  - [imports.rs](../../../src/code_lint/ast/imports.rs): `resolve_name`, `ResolvedName`, `QualifiedName`.
- **Test Harness**:
  - [test_utils.rs](../../../src/test_utils.rs): `run_code_rule`, `assert_rule_pass`, `assert_rule_fail`, `assert_documented_examples`, `rule_test!`.

---

## 3. Verdict per Candidate Check

| Legacy Rule / Component | Verdict | Rationale |
|---|---|---|
| **Positional tuples in parameters & attributes** (`BannedTypeAnnotationsRule` patterns 1–5 structural core) | **Implement `positional-tuple-parameter` & `positional-tuple-attribute`** (`Python`, `Rust`) | Completes the structural `positional-tuple-*` family alongside `positional-tuple-return` so any fixed-length multi-element tuple (`arity >= min-elements`, default `2`) in parameters or class/struct attributes/fields is flagged in Python and Rust without project-specific configuration. |
| **Project-configured banned type shapes & replacements** (`BannedTypeAnnotationsRule` patterns 1–6) | **Implement `adhoc-type-annotation`** (`Python`, `Rust`) | Provides an opt-in (`base: &[]`) structural type-annotation pattern matcher supporting `"<pattern> => <replacement>"` entries, `_` / `...` wildcards, order-independent unions (`A \| B`, `Optional[A]`, `Union[A, B]`), import-resolved type constructors, and nested type-argument matching across parameter, return, and attribute annotations. |
| **`PrimitiveTypeSuggestionsRule`** | **Drop (superseded)** | Disabled in the legacy linter due to extreme false positives; high-signal cases are covered by `primitive-duration`, `identical-positional-types`, and `positional-tuple-*`. |
| **`FloatToDecimalSuggestionsRule`** | **Drop** | Domain-specific to monetary calculations; `float` is the standard type for non-monetary continuous quantities. |
| **`DecimalZeroFormattingRule`** | **Drop** | Unsound for `Decimal("0.00")`, where the string literal sets IEEE 754 decimal exponent `-2` (required by `.quantize(Decimal("0.00"))` and fixed-point formatting), whereas `Decimal(0)` has exponent `0`. |
