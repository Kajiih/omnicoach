# Phase 3: Design — `positional-tuple-parameter`, `positional-tuple-attribute` & `adhoc-type-annotation`

> **Status:** VALIDATED (user, 2026-10-26).

## 1. Overview & Rule Summary

Following the validated Phase 1 decisions ([01_understand.md](01_understand.md) §4), this track delivers three rules across Python and Rust while keeping `PrimitiveTypeSuggestionsRule`, `FloatToDecimalSuggestionsRule`, and `DecimalZeroFormattingRule` dropped:

| Rule | File | Languages | Options | Target | Topics | Precision / Consensus / Quality |
|---|---|---|---|---|---|---|
| `positional-tuple-parameter` | `src/code_lint/rules/positional_tuple_parameter.rs` | `Python`, `Rust` | `CountOption` `min-elements = 2`, `enforcement-mode = "ban"` | `RuleTarget::SourceOnly` | `Topic::POSITIONAL_MEANING` | `Precision::Exact`, `Consensus::Opinionated`, `ImpactedQuality::Maintainability` |
| `positional-tuple-attribute` | `src/code_lint/rules/positional_tuple_attribute.rs` | `Python`, `Rust` | `CountOption` `min-elements = 2`, `enforcement-mode = "ban"` | `RuleTarget::SourceOnly` | `Topic::POSITIONAL_MEANING` | `Precision::Exact`, `Consensus::Opinionated`, `ImpactedQuality::Maintainability` |
| `adhoc-type-annotation` | `src/code_lint/rules/adhoc_type_annotation.rs` | `Python`, `Rust` | `ListOption` `banned` (`base: &[]`), `enforcement-mode = "ban"` | `RuleTarget::SourceOnly` | `Topic::STATIC_TYPING` | `Precision::Exact`, `Consensus::Opinionated`, `ImpactedQuality::Maintainability` |

---

## 2. Numbered Decisions (`D1..D10`)

- **D1 — Shared AST abstraction for parameter and attribute type contracts ([ADR 012](../../decisions/012_module_placement_by_abstraction.md))**:
  - Just as [src/code_lint/ast/returns.rs](../../../src/code_lint/ast/returns.rs) defines `FunctionReturn` and `collect_function_returns(file)` for return contracts across Python and Rust, we add language-dispatched contract extractors for parameter and attribute/field type annotations in `code_lint::ast`:
    - **`ParameterAnnotation<'a>`** (`collect_parameter_annotations(file) -> Vec<ParameterAnnotation<'a>>`):
      - `name: String` (parameter identifier, e.g. `"bounds"`),
      - `function: String` (enclosing function/method name, e.g. `"clip"`),
      - `type_node: AstNode<'a>` (the parameter's type annotation node),
      - `positional_tuple_arity: Option<usize>` (largest fixed-length tuple arity $\ge 2$ after unwrapping transparent wrappers).
    - **`AttributeAnnotation<'a>`** (`collect_attribute_annotations(file) -> Vec<AttributeAnnotation<'a>>`):
      - `name: String` (attribute or field identifier, e.g. `"bounds"`),
      - `class: String` (enclosing class, `struct`, or `enum` name, e.g. `"Viewport"`),
      - `type_node: AstNode<'a>` (the attribute/field's type annotation node),
      - `positional_tuple_arity: Option<usize>` (largest fixed-length tuple arity $\ge 2$ after unwrapping transparent wrappers).
  - Both `positional-tuple-parameter` and `positional-tuple-attribute` become thin, language-independent filters over `collect_parameter_annotations` and `collect_attribute_annotations`, mirroring `positional-tuple-return`.

- **D2 — Transparent wrapper unwrapping for positional tuples (Python & Rust)**:
  - A parameter or attribute type is a `PositionalTuple { arity }` ($arity \ge 2$) when its top-level type—after unwrapping transparent qualifiers, optionals/unions, and borrow/pointer wrappers—is a fixed-length tuple of at least 2 elements:
    - **Python**:
      - Fixed-length `tuple[T1, T2, ...]` or `Tuple[T1, T2, ...]` (resolved through imports via `ast::resolve_name`; excludes variadic `tuple[T, ...]` where any argument is `...`, empty `tuple[()]`, and 1-tuple `tuple[T]`).
      - Unwraps union constructs (`A | B`, `Optional[T]`, `Union[T1, T2]`) and transparent qualifiers (`Annotated[T, ...]`, `ClassVar[T]`, `Final[T]`, `Required[T]`, `NotRequired[T]`, `ReadOnly[T]`, `dataclasses.InitVar[T]`). When a union has multiple fixed-length tuple branches, `positional_tuple_arity` returns the maximum arity (matching `PythonReturnTypeUnion::positional_tuple_arity`).
      - Does **not** recurse into collection elements (`list[tuple[int, str]]`, `Sequence[tuple[int, str]]`, `dict[str, tuple[int, int]]`, `Callable[[tuple[int, int]], None]`), matching `positional-tuple-return` E6 (`enumerate()` / `.items()` pairs inside collections).
    - **Rust**:
      - Fixed-length tuple `(T1, T2, ...)` (`ast::Type::TupleType` with $\ge 2$ fields; excludes unit `()` and 1-tuple `(T,)`).
      - Unwraps `Option<T>` (`Option`, `option::Option`, `std::option::Option`, `core::option::Option`) and transparent reference/smart-pointer wrappers (`&T`, `&mut T`, `Box<T>`, `Rc<T>`, `Arc<T>`, `Cow<'_, T>`), in any nesting order (e.g. `Option<&(u32, u32)>` and `&Option<(u32, u32)>`).
      - Does **not** recurse into collection elements (`Vec<(u32, u32)>`, `&[(u32, u32)]`, `HashMap<K, (V1, V2)>`).

- **D3 — Inspected sites and contract exemptions for `positional-tuple-parameter` and `positional-tuple-attribute`**:
  - **Parameters (`collect_parameter_annotations`)**:
    - **Python**: Functions from `extract_function_signatures(file)` where `!is_exempt_from_signature_rules(&signature)` (skips `@override`, `@overload`, `@abstractmethod`, `@pytest.fixture`, `@<fn>.register`, `@<prop>.setter` / `@<prop>.deleter`, data-model dunders other than `__init__` / `__new__` / `__call__`, and methods of `Protocol` or `ABC` classes). Skips receiver parameters (`self`, `cls`: `param.kind == PythonParameterKind::Receiver`) and variadic `*args` / `**kwargs` (`param.is_variadic()`, where a tuple annotation in PEP 646 `Unpack` or legacy conventions has special typing semantics). Note: closures (`lambda`) have no parameter type annotations in Python syntax.
    - **Rust**: `ast::Fn` items outside `trait` declarations, `impl Trait for Type` blocks (`!is_trait_or_trait_impl_function(&function)`), and inline test modules/functions (`!node.is_in_rust_inline_test()`). Inspects each `ast::Param` with a `ty()`, where the parameter pattern is a single identifier (`ast::Pat::IdentPat`) or wildcard (`ast::Pat::WildcardPat`, named `"_"`). Destructured tuple parameters (`fn f((x, y): (u32, u32))`) are also inspected using the pattern text (`(x, y)`) as `name` because the caller still passes an unnamed positional `(u32, u32)` at every call site!
  - **Attributes / Fields (`collect_attribute_annotations`)**:
    - **Python**: Classes from `extract_classes(file)` where `!class.is_interface()` (skips `Protocol` and `ABC` / `ABCMeta` classes). Collects class-body annotated attributes (`x: T` in `class.definition.body`) and instance attribute annotations (`self.x: T` in direct instance methods via `class.instance_attribute_annotations()`, deduplicated by attribute `(name, span)` so `__init__` annotations are visited once in source order). Both public and `_`-prefixed attributes are checked (matching legacy `AnnotationInspectorRule`, because unlike `concrete-collection-attribute` where private mutable state is normal, a private positional tuple `self._bounds: tuple[int, int]` still forces positional indexing `self._bounds[0]` across the class).
    - **Rust**: Named fields (`ast::RecordField`) on `ast::Struct` and `ast::Enum` (`ast::Variant`) items outside inline tests (`!node.is_in_rust_inline_test()`). Tuple structs (`struct Pair(u32, u32);`) have no `RecordField`s and are already covered at `.0` / `.1` read sites by `repeated-index-access`.

- **D4 — Flagged spans and templates for `positional-tuple-parameter` and `positional-tuple-attribute`**:
  - Consistent with `concrete-collection-parameter` and `concrete-collection-attribute`, both rules anchor their diagnostic on `type_node` (the parameter or attribute's type annotation node) and pass `("name", ...)`, `("function" | "class", ...)`, and `("count", &arity.to_string())`.
  - **`positional-tuple-parameter` `TEMPLATE`**:
    - `summary`: `"Parameter `{name}` of `{function}` is typed as a positional tuple of {count} elements."`
    - `rationale`: `"A tuple names none of its elements, so callers pass values by position and swapping two elements of the same type raises no error."`
    - `suggestion`:
      - `Python`: `"Replace the tuple annotation on `{name}` in `{function}` with a `NamedTuple` or a frozen dataclass whose fields name each element."`
      - `Rust`: `"Replace the tuple type on `{name}` in `{function}` with a struct whose fields name each element."`
  - **`positional-tuple-attribute` `TEMPLATE`**:
    - `summary`:
      - `Python`: `"Attribute `{name}` of `{class}` is typed as a positional tuple of {count} elements."`
      - `Rust`: `"Field `{name}` of `{class}` is typed as a positional tuple of {count} elements."`
    - `rationale`: `"A tuple names none of its elements, so every read and write depends on element order and swapping two elements of the same type raises no error."`
    - `suggestion`:
      - `Python`: `"Replace the tuple annotation on `{name}` in `{class}` with a `NamedTuple` or a frozen dataclass whose fields name each element."`
      - `Rust`: `"Replace the tuple type on `{name}` in `{class}` with a struct whose fields name each element."`
  - Every field satisfies `tests/registry.rs`: ends with `.`, uses backticks, has 1-sentence `summary` with no fix verbs, `rationale` with no fix verb at start and no `"should"`, `suggestion` starting with `"Replace"` ($\in \text{SUGGESTION\_VERBS}$), no judgement words, and placeholders strictly from `PLACEHOLDERS` (`name`, `function`, `class`, `count`).

- **D5 — `adhoc-type-annotation` entry syntax (`"<pattern>"` and `"<pattern> => <replacement>"`)**:
  - `adhoc-type-annotation` declares `BANNED: ListOption` (`ListKind::Deny`, `FilterListDefaults { base: &[], extend: &[], remove: &[] }`).
  - Each string in `banned` / `extend-banned` is parsed as:
    - `"<pattern> => <replacement>"` when the entry contains `"=>"` (trimmed on both sides; e.g. `"tuple[Width, Height] => Dimensions"` or `"Generator[_, None, None] => collections.abc.Iterator"`), setting `{replacement}` to `` `{replacement_text}` `` (wrapped in backticks if not already backtick-quoted).
    - `"<pattern>"` when `" => "` / `"=>"` is absent, setting `{replacement}` to `"a dedicated domain type"`.
  - **`adhoc-type-annotation` `TEMPLATE`**:
    - `summary`: `"Type annotation `{expression}` uses `{token}`."`
    - `rationale`: `"An ad-hoc composite type annotation couples signatures and fields to a raw representation instead of the domain type or protocol meant for that contract."`
    - `suggestion`: `"Replace `{token}` in `{expression}` with {replacement}."`
    - Note how `{replacement}` renders cleanly in both cases:
      - With `=> Dimensions`: ``Replace `tuple[Width, Height]` in `tuple[Width, Height] | None` with `Dimensions`.``
      - Without `=>`: ``Replace `tuple[Width, Height]` in `tuple[Width, Height] | None` with a dedicated domain type.``
    - Every `tests/registry.rs` rule is satisfied (`summary`, `rationale`, `suggestion`, `PLACEHOLDERS`: `expression`, `token`, `replacement`).

- **D6 — Structural AST pattern grammar & matching for `adhoc-type-annotation`**:
  - Both pattern strings and target source annotations are normalized into a language-neutral structural **`TypePattern`** / **`TypeShape`** tree in `code_lint::ast::type_annotations` (and matched in `code_lint::semantic::type_annotations` or `code_lint::ast::type_annotations`):
    1. **Wildcards**: `_` or `...` in a pattern matches any single type subtree at that position (and a trailing `...` in `tuple[Iterator[Dimensions], ...]` matches one or more remaining arguments at the end of a subscript/tuple argument list!).
       - Note on Python variadic `tuple[T, ...]`: in a target Python annotation `tuple[int, ...]`, `...` is the literal `Ellipsis` variadic marker (`TypeShape::Ellipsis`). A pattern `tuple[int, ...]` matches `tuple[int, ...]`, whereas a pattern with 3+ elements ending in `...` (like `tuple[Iterator[Dimensions], ...]`) or `_` (`Generator[_, None, None]`) matches fixed-length tuples/generics! To make this completely unambiguous:
         - `_` in a pattern is always a single-argument wildcard (`TypePattern::Wildcard`).
         - `...` in a pattern matches either a literal `Ellipsis` in the target annotation (`tuple[int, ...]`) OR a trailing prefix wildcard when the target has $\ge \text{pattern.args.len()}$ arguments.
    2. **Unions (`A | B`, `Optional[A]`, `Union[A, B]`)**:
       - Flattened into a set/multiset of branch shapes (with `Optional[A]` normalized to `A | None`, and `Annotated[T, ...]` unwrapped to `T`).
       - Matching two unions is **order-independent** and deduplicated: pattern `Bound | None` matches target `Bound | None`, `None | Bound`, `Optional[Bound]`, and `typing.Union[Bound, None]`.
    3. **Tuples (`tuple[A, B]`, `Tuple[A, B]`, `(A, B)`)**:
       - In Python, `tuple[A, B]` and `typing.Tuple[A, B]` normalize to `Tuple([A, B])` (when `tuple`/`Tuple` resolves to the stdlib tuple constructor).
       - In Rust, `(A, B)` normalizes to `Tuple([A, B])`.
       - Thus `"tuple[Width, Height]"` (or `"(Width, Height)"`) matches fixed-length 2-element tuples of `Width` and `Height`!
    4. **Generic / Subscripted Types (`Base[Arg1, Arg2]` in Python, `Base<Arg1, Arg2>` in Rust) and Bare Named Types (`Base`)**:
       - The constructor name is resolved in the target file via `ast::resolve_name`:
         - If the target constructor's first segment is `ResolvedName::Local` (a class/type defined in the same file):
           - Wait: what about domain types like `Width`, `Height`, `Bound`, `Dimensions`, `HttpClient`? In a file that defines `class Width:` or imports `from .models import Width`, how should constructor names in patterns match?
           - Let's check this carefully!
           - In `BannedTypeAnnotationsRule`, `Width`, `Height`, `Bound`, `HttpClient`, `FakeClient`, `Credentials`, `Response` are **domain classes** (either imported from another module in the project, e.g. `from myproj.types import Width`, or defined in the current file, or unqualified in snippets), while `anyio.Event`, `Generator`, `Iterator`, `list` are stdlib/third-party types!
           - If a user writes `"tuple[Width, Height] => Dimensions"` in `.omnicoach.toml`:
             - In a real codebase, `Width` might be imported via `from app.geometry import Width` (so `resolve_name(file, "Width")` is `ResolvedName::Imported("app.geometry.Width")` with terminal `"Width"`), OR defined locally in `geometry.py` (`ResolvedName::Local` with written text `"Width"`), OR unbound in a test snippet (`ResolvedName::Unbound` with written text `"Width"`).
             - Meanwhile, for stdlib/qualified patterns like `"anyio.Event"` or `"collections.abc.Generator[_, None, None]"`: if `from anyio import Event` is in the file, `Event` resolves to `ResolvedName::Imported("anyio.Event")` (path `"anyio.Event"`, terminal `"Event"`).
             - What if a local class shadows a stdlib type, e.g., `class Generator:` defined locally when the pattern is `"collections.abc.Generator[_, None, None]"`? A qualified pattern `"collections.abc.Generator"` has a module prefix `"collections.abc"`, so a local `class Generator` does NOT match `"collections.abc.Generator"`. And what if the pattern is unqualified `"Generator[_, None, None]"`? In Python stdlib, `Generator` comes from `typing` or `collections.abc`.
             - Therefore, how should a pattern constructor `P` match a target type constructor `T`?
               - Let `T` have:
                 - `written`: the compact source text as written at the annotation site (e.g. `"Width"`, `"anyio.Event"`, `"t.Generator"`),
                 - `resolved`: `resolve_name(file, written)`:
                   - `Imported(canonical_path)` (with `terminal` = last segment of `canonical_path`),
                   - `Unbound` (with `canonical_path = written`, `terminal` = last segment of `written`),
                   - `Local` (defined in `file`).
               - Case A: Pattern constructor `P` is **qualified** (contains `.` in Python or `::` in Rust, e.g. `"anyio.Event"`, `"typing.Generator"`, `"crate::net::HostPort"`):
                 - Matches when `T` is not `Local` and `T`'s `canonical_path == P` (or, for stdlib typing aliases in Python where `P` is in `typing` / `typing_extensions` / `collections.abc`, matches any of those standard typing namespaces with the same terminal!).
               - Case B: Pattern constructor `P` is **unqualified** (a bare identifier like `"Width"`, `"Generator"`, `"Iterator"`, `"list"`):
                 - If `P` is a Python stdlib typing/collection constructor (`Generator`, `Iterator`, `Iterable`, `Awaitable`, `Sequence`, `Mapping`, `list`, `dict`, `set`, `tuple`, `Tuple`, `List`, `Dict`, `Set`, `Callable`, `Any`, `object`, `BaseException`, etc.), it matches when `T` is not `Local` and `T` resolves to that stdlib constructor (unqualified or imported from `typing`, `typing_extensions`, `collections.abc`, or `builtins`).
                 - Otherwise (`P` is a domain type name like `"Width"`, `"Height"`, `"Bound"`, `"SortOrder"`, `"HttpClient"`), it matches when `T`'s written name is `P` OR `T` is `Imported(path)` whose terminal segment is `P`!
               - Let's verify this!
                 - If pattern is `"tuple[Width, Height]"`:
                   - `tuple` is a stdlib constructor → matches `tuple[...]` or `typing.Tuple[...]` (unless `class Tuple:` is defined locally!).
                   - `Width` is a domain name → matches `Width` whether `Width` is unbound, imported (`from pkg.types import Width`), or defined in the file!
                 - If pattern is `"tuple[anyio.Event, list[Response | BaseException]]"`:
                   - `anyio.Event` is qualified → matches `anyio.Event` or `Event` after `from anyio import Event` (and does NOT match `threading.Event` or a local `class Event:`!).
                   - `list` and `BaseException` are stdlib names → match builtin `list` / `typing.List` and builtin `BaseException`.
                   - `Response` is a domain name → matches `Response` (unbound, imported, or local).
                 - If pattern is `"Generator[_, None, None]"`:
                   - `Generator` is a stdlib name → matches `Generator`, `typing.Generator`, `collections.abc.Generator`, `t.Generator` (and does NOT match a locally defined `class Generator:`!).
                 - In Rust:
                   - Same rule! Standard library prelude/wrapper names (`Option`, `Result`, `Vec`, `Box`, `Arc`, `Rc`, `Cow`, `String`, primitives `u8`..`usize`, `bool`, `str`) match when `T` is not `Local` and resolves to the stdlib name; qualified paths (`tokio::sync::Notify`, `std::collections::HashMap`) match when `T` is not `Local` and `T`'s resolved path matches; unqualified domain names (`Width`, `Height`, `Bound`) match when `T`'s written name or imported terminal is that name.
    5. **Reference & Pointer Transparency in Rust**:
       - In Rust, if a pattern is `(Width, Height)` (without `&`), and a parameter is `size: &(Width, Height)`, since `adhoc-type-annotation` walks all nested type nodes (see D7), `(Width, Height)` inside `&(Width, Height)` is visited and flagged at `(Width, Height)`—and if a pattern explicitly starts with `&` (e.g. `&Vec<u8>`), it matches the `RefType` `&Vec<u8>` directly!

- **D7 — Subtree traversal & non-overlapping source-order reporting in `adhoc-type-annotation`**:
  - For every inspected type annotation (parameters, returns, and attributes/fields, using the same contract exemptions as D3):
    - Walk the type annotation AST in pre-order (source order), skipping `Annotated[T, metadata]` value metadata expressions (`args[1..]`).
    - At each type node, test configured patterns. If a pattern matches:
      - Emit one diagnostic anchored at that matched type node (`node`), with:
        - `expression`: the full enclosing parameter, return, or attribute type annotation text (e.g. `list[tuple[Width, Height]]`),
        - `token`: the matched subnode's source text (e.g. `tuple[Width, Height]`),
        - `replacement`: the pattern's replacement string (`Dimensions` formatted as `` `Dimensions` ``, or `"a dedicated domain type"` when `=>` is omitted).
      - Do not report a second pattern on the same node; continue walking child type arguments only if they do not overlap the same pattern match (or if a child matches a *different* banned pattern, e.g. an outer banned container and an inner banned tuple). When multiple configured patterns match the *same* AST node, the most specific pattern (or first in deterministic order) wins once.

- **D8 — Test harness support for opt-in `ListOption` rules (`src/test_utils.rs`)**:
  - `adhoc-type-annotation` has `BANNED.default.base = &[]` so it produces zero findings unless configured.
  - We extend `src/test_utils.rs` cleanly:
    - Add `run_code_rule_with_overrides(rule, source, filename, overrides: Option<&RuleOverrides>)`.
    - Add an optional `options: &[(&str, &[&str])]` (or `banned: &[&str]`) parameter to `rule_test!`:
      - E.g. `rule_test!(RULE, banned: &["tuple[Width, Height] => Dimensions", ...], { ... })`.
      - When `banned: &[...]` is provided to `rule_test!`, `test_utils` constructs a `RuleOverrides` with `banned = [...]` and passes it to `assert_documented_examples`, `assert_rule_pass`, and `assert_rule_fail`.
      - All existing `rule_test!(RULE, { ... })` and `rule_test!(RULE, repeat: DistinctLiterals, { ... })` invocations remain 100% unchanged!

- **D9 — Placement of shared helpers ([ADR 006](../../decisions/006_architectural_dag_and_conformance.md), [ADR 011](../../decisions/011_colocated_abstraction_ordering.md), [ADR 012](../../decisions/012_module_placement_by_abstraction.md))**:
  - `src/code_lint/ast/type_annotations.rs` (language-dispatched) + `src/code_lint/ast/python/annotations.rs` + `src/code_lint/ast/rust/annotations.rs`:
    - `ParameterAnnotation`, `AttributeAnnotation`, `collect_parameter_annotations`, `collect_attribute_annotations`.
    - `TypeAnnotationMatch`, `find_matching_type_annotations(file, banned_entries)`: parses pattern strings into structural `TypePattern`s and matches them against parameter, return, and attribute/field type annotations in `file`.
  - Why in `code_lint::ast` (or `code_lint::semantic` on top of `code_lint::ast`)?
    - Per ADR 012 P1/P2/P7: extracting parameter, return, and attribute annotations and structurally comparing a type annotation against a type pattern (`A | B`, `Optional[A]`, `tuple[A, B]`, `(A, B)`, import-resolved constructors) are parser-backed syntax/import facts; `code_lint::ast` owns `ruff_python_ast` and `ra_ap_syntax`, while `code_lint::semantic::type_annotations` (or `code_lint::ast::type_annotations`) owns matching a `HashSet<String>` of `"pattern => replacement"` entries against a `ParsedFile`.
    - Let's check: `calls::find_banned_calls(file, banned)` lives in `src/code_lint/semantic/calls.rs` and calls `ast::collect_call_candidates(file)`; `bindings::find_suffixed_bindings(file, banned)` lives in `src/code_lint/semantic/bindings.rs` and calls `ast::collect_bindings(file)`.
    - Putting `TypeShape` / `collect_type_annotations(file)` in `code_lint::ast` and `find_banned_type_annotations(file, banned)` in `code_lint::semantic::type_annotations` (or `ast` + `semantic` following the exact `calls` / `bindings` pattern) keeps `code_lint::ast` 100% free of rule-config entry syntax (`"=>"` splitting and `{replacement}` formatting) and puts the config-entry matcher in `code_lint::semantic`! That is completely faithful to ADR 006 and ADR 012 P7.

- **D10 — Roadmap & `not_pursued_rules.md` updates**:
  - In [docs/dev/not_pursued_rules.md](../../not_pursued_rules.md), update the `BannedTypeAnnotationsRule` bullet so it lists only the three dropped rules (`PrimitiveTypeSuggestionsRule`, `FloatToDecimalSuggestionsRule`, `DecimalZeroFormattingRule`) and notes that `BannedTypeAnnotationsRule` is covered by `positional-tuple-return`, `positional-tuple-parameter`, `positional-tuple-attribute`, and `adhoc-type-annotation`.
  - In [ROADMAP.md](../../../ROADMAP.md), update the rule count and any relevant entries.

---

## 3. Detailed Rule Specifications & Exemptions

### 3.1 `positional-tuple-parameter`

#### Rule Declaration

- **Name**: `positional-tuple-parameter`
- **Languages**: `&[Language::Python, Language::Rust]`
- **Options**: `RuleOptions::code_rule(MIN_ELEMENTS)` where `MIN_ELEMENTS` has `key: "min-elements"`, `default: LanguageDefaults::new(2, &[])`
- **Target**: `RuleTarget::SourceOnly`
- **Classification**: `Topic::POSITIONAL_MEANING`, `Precision::Exact`, `Consensus::Opinionated`, `ImpactedQuality::Maintainability`
- **Template**:

  ```rust
  const TEMPLATE: ViolationTemplate = violation_template! {
      summary: "Parameter `{name}` of `{function}` is typed as a positional tuple of {count} elements.",
      rationale: "A tuple names none of its elements, so callers pass values by position and swapping two elements of the same type raises no error.",
      suggestion: {
          Python => "Replace the tuple annotation on `{name}` in `{function}` with a `NamedTuple` or a frozen dataclass whose fields name each element.",
          Rust => "Replace the tuple type on `{name}` in `{function}` with a struct whose fields name each element.",
      },
  };
  ```

- **Documented Examples**:
  - **Python**:
    - Flagged (`flagged_span: "tuple[float, float]"`):

      ```python
      def inside_box(point: tuple[float, float], limit: float) -> bool:
          return point[0] <= limit and point[1] <= limit
      ```

    - Fixed:

      ```python
      from typing import NamedTuple

      class Point(NamedTuple):
          x: float
          y: float

      def inside_box(point: Point, limit: float) -> bool:
          return point.x <= limit and point.y <= limit
      ```

  - **Rust**:
    - Flagged (`flagged_span: "(f64, f64)"`):

      ```rust
      fn inside_box(point: (f64, f64), limit: f64) -> bool {
          point.0 <= limit && point.1 <= limit
      }
      ```

    - Fixed:

      ```rust
      struct Point {
          x: f64,
          y: f64,
      }

      fn inside_box(point: Point, limit: f64) -> bool {
          point.x <= limit && point.y <= limit
      }
      ```

---

### 3.2 `positional-tuple-attribute`

#### Rule Declaration

- **Name**: `positional-tuple-attribute`
- **Languages**: `&[Language::Python, Language::Rust]`
- **Options**: `RuleOptions::code_rule(MIN_ELEMENTS)` where `MIN_ELEMENTS` has `key: "min-elements"`, `default: LanguageDefaults::new(2, &[])`
- **Target**: `RuleTarget::SourceOnly`
- **Classification**: `Topic::POSITIONAL_MEANING`, `Precision::Exact`, `Consensus::Opinionated`, `ImpactedQuality::Maintainability`
- **Template**:

  ```rust
  const TEMPLATE: ViolationTemplate = violation_template! {
      summary: {
          Python => "Attribute `{name}` of `{class}` is typed as a positional tuple of {count} elements.",
          Rust => "Field `{name}` of `{class}` is typed as a positional tuple of {count} elements.",
      },
      rationale: "A tuple names none of its elements, so every read and write depends on element order and swapping two elements of the same type raises no error.",
      suggestion: {
          Python => "Replace the tuple annotation on `{name}` in `{class}` with a `NamedTuple` or a frozen dataclass whose fields name each element.",
          Rust => "Replace the tuple type on `{name}` in `{class}` with a struct whose fields name each element.",
      },
  };
  ```

- **Documented Examples**:
  - **Python**:
    - Flagged (`flagged_span: "tuple[int, int]"`):

      ```python
      from dataclasses import dataclass

      @dataclass(frozen=True)
      class Window:
          size: tuple[int, int]
      ```

    - Fixed:

      ```python
      from dataclasses import dataclass

      @dataclass(frozen=True)
      class Dimensions:
          width: int
          height: int

      @dataclass(frozen=True)
      class Window:
          size: Dimensions
      ```

  - **Rust**:
    - Flagged (`flagged_span: "(u32, u32)"`):

      ```rust
      struct Window {
          size: (u32, u32),
      }
      ```

    - Fixed:

      ```rust
      struct Dimensions {
          width: u32,
          height: u32,
      }

      struct Window {
          size: Dimensions,
      }
      ```

---

### 3.3 `adhoc-type-annotation`

#### Rule Declaration

- **Name**: `adhoc-type-annotation`
- **Languages**: `&[Language::Python, Language::Rust]`
- **Options**: `RuleOptions::code_rule(BANNED)` where `BANNED` has `kind: ListKind::Deny`, `default: FilterListDefaults { base: &[], extend: &[], remove: &[] }`
- **Target**: `RuleTarget::SourceOnly`
- **Classification**: `Topic::STATIC_TYPING`, `Precision::Exact`, `Consensus::Opinionated`, `ImpactedQuality::Maintainability`
- **Template**:

  ```rust
  const TEMPLATE: ViolationTemplate = violation_template! {
      summary: "Type annotation `{expression}` uses `{token}`.",
      rationale: "An ad-hoc composite type annotation couples signatures and fields to a raw representation instead of the domain type or protocol meant for that contract.",
      suggestion: "Replace `{token}` in `{expression}` with {replacement}.",
  };
  ```

- **Documented Examples** (tested with `banned: &["tuple[Width, Height] => Dimensions", "Generator[_, None, None] => collections.abc.Iterator", "(Width, Height) => Dimensions"]`):
  - **Python**:
    - Flagged (`flagged_span: "tuple[Width, Height]"`):

      ```python
      def resize(canvas: Canvas, target: tuple[Width, Height]) -> None:
          canvas.apply(target)
      ```

    - Fixed:

      ```python
      def resize(canvas: Canvas, target: Dimensions) -> None:
          canvas.apply(target)
      ```

  - **Rust**:
    - Flagged (`flagged_span: "(Width, Height)"`):

      ```rust
      fn resize(canvas: &mut Canvas, target: (Width, Height)) {
          canvas.apply(target);
      }
      ```

    - Fixed:

      ```rust
      fn resize(canvas: &mut Canvas, target: Dimensions) {
          canvas.apply(target);
      }
      ```

---

## 4. Exemptions (`E1..E11`) & Planned Test Grids

| ID | Applies To | Exemption | Rationale |
|---|---|---|---|
| **E1** | `positional-tuple-*` | Variadic `tuple[T, ...]`, empty `tuple[()]` / `()`, and 1-element `tuple[T]` / `(T,)` | Homogeneous variable-length sequences (`tuple[T, ...]`), unit types, and 1-tuples are not multi-element positional records. |
| **E2** | `positional-tuple-*` | Tuple nested inside a collection or callable (`list[tuple[int, str]]`, `Sequence[tuple[int, str]]`, `Vec<(u32, u32)>`, `&[(u32, u32)]`, `Callable[[tuple[int, int]], None]`) | Collection elements (such as `enumerate()` or `.items()` pairs) do not define a top-level parameter or attribute tuple contract; project-specific nested tuple shapes can be configured in `adhoc-type-annotation`. |
| **E3** | All 3 rules (parameters & returns) | Python functions with imposed signatures (`@override`, `@overload`, `@abstractmethod`, `@pytest.fixture`, `@<fn>.register`, property setters/deleters, data-model dunders other than `__init__`/`__new__`/`__call__`, and methods of `Protocol` or `ABC` classes) | Signature is locked by a supertype, typing overload, or framework protocol (`policy::is_exempt_from_signature_rules`). |
| **E4** | All 3 rules (parameters & returns) | Rust trait method declarations (`trait T { fn f(...); }`) and trait implementations (`impl T for S { fn f(...) {} }`) | Signature is locked by the trait definition (`is_trait_or_trait_impl_function`). |
| **E5** | All 3 rules (attributes) | Python `Protocol` and `ABC` / `ABCMeta` class attributes (`class.is_interface()`) | Interface contract definitions are skipped, matching `concrete-collection-attribute` and `mutable-collection-attribute`. |
| **E6** | All 3 rules (parameters) | Python receiver parameters (`self`, `cls`) and variadic parameters (`*args`, `**kwargs`) | Receiver types and `*args` / `**kwargs` (including `Unpack[...]` variadic typing) are not ordinary value parameters. |
| **E7** | `positional-tuple-attribute`, `adhoc-type-annotation` | Module-level and function-local variable annotations (`x: tuple[int, int] = (1, 2)`, `let pair: (u32, u32) = (1, 2);`) | Only function signatures (parameters/returns) and class/struct/enum attributes/fields are contract boundaries; local tuple bindings are checked at read sites by `repeated-index-access`. |
| **E8** | All 3 rules | Rust inline test modules and `#[test]` functions (`node.is_in_rust_inline_test()`) and test files (`RuleTarget::SourceOnly`) | Rules target production source contracts. |
| **E9** | All 3 rules | `Annotated[T, metadata]` value metadata arguments (`args[1..]`) | Runtime metadata expressions inside `Annotated[T, ...]` are not type annotations. |
| **E10** | All 3 rules | Local class shadowing `tuple` / `Tuple` or a banned stdlib type (`class Tuple: ...`, `struct Option<T>(T);`) | Resolved via `ast::resolve_name` (`ResolvedName::Local`). |
| **E11** | `adhoc-type-annotation` | Unconfigured default (`banned = []`) | Emits zero diagnostics when `banned` / `extend-banned` is empty. |

### 4.1 Planned Test Grid for `positional-tuple-parameter`

- **Python `pass`**: `named_tuple_or_dataclass_parameter`, `variadic_tuple_exempt` (E1), `single_element_or_empty_tuple_exempt` (E1), `tuple_inside_collection_exempt` (E2), `callable_tuple_argument_exempt` (E2), `annotated_metadata_tuple_exempt` (E9), `variadic_args_exempt` (E6), `variadic_kwargs_exempt` (E6), `receiver_parameter_exempt` (E6), `override_method_exempt` (E3), `overload_signature_exempt` (E3), `abstractmethod_exempt` (E3), `property_setter_exempt` (E3), `pytest_fixture_exempt` (E3), `protocol_method_exempt` (E3), `abc_method_exempt` (E3), `data_model_dunder_exempt` (E3), `singledispatch_register_exempt` (E3), `local_class_shadows_typing_tuple` (E10).
- **Python `fail`**: `two_element_tuple_parameter`, `three_element_typing_tuple_parameter`, `keyword_only_tuple_parameter`, `positional_only_tuple_parameter`, `optional_pep604_tuple_parameter`, `typing_optional_and_annotated_tuple_parameter`, `init_constructor_tuple_parameter`, `new_constructor_tuple_parameter`, `call_dunder_tuple_parameter`, `aliased_typing_tuple_parameter`.
- **Rust `pass`**: `struct_parameter`, `unit_or_single_element_tuple_exempt` (E1), `tuple_inside_vec_or_slice_exempt` (E2), `trait_declaration_parameter_exempt` (E4), `trait_impl_parameter_exempt` (E4), `inline_test_parameter_exempt` (E8).
- **Rust `fail`**: `two_element_tuple_parameter`, `borrowed_tuple_parameter`, `mutable_borrowed_tuple_parameter`, `option_tuple_parameter`, `boxed_tuple_parameter`, `destructured_tuple_parameter`, `inherent_method_tuple_parameter`.

### 4.2 Planned Test Grid for `positional-tuple-attribute`

- **Python `pass`**: `named_record_attributes`, `variadic_tuple_attribute_exempt` (E1), `single_element_or_empty_tuple_attribute_exempt` (E1), `tuple_inside_collection_attribute_exempt` (E2), `annotated_metadata_tuple_exempt` (E9), `protocol_class_attribute_exempt` (E5), `abc_class_attribute_exempt` (E5), `module_variable_annotation_exempt` (E7), `function_local_variable_annotation_exempt` (E7), `local_class_shadows_typing_tuple` (E10).
- **Python `fail`**: `dataclass_tuple_attribute`, `typing_tuple_attribute`, `optional_tuple_attribute`, `classvar_tuple_attribute`, `final_tuple_attribute`, `annotated_wrapper_tuple_attribute`, `init_var_tuple_attribute`, `init_instance_tuple_attribute`, `private_tuple_attribute`, `typed_dict_tuple_field`.
- **Rust `pass`**: `named_struct_fields`, `unit_or_single_element_tuple_field_exempt` (E1), `tuple_inside_vec_field_exempt` (E2), `tuple_struct_unnamed_fields_exempt` (D3), `inline_test_struct_exempt` (E8), `local_let_tuple_binding_exempt` (E7).
- **Rust `fail`**: `struct_tuple_field`, `option_tuple_field`, `borrowed_tuple_field`, `boxed_tuple_field`, `enum_variant_record_tuple_field`.

### 4.3 Planned Test Grid for `adhoc-type-annotation`

- Configured in `rule_test!` with:

  ```rust
  banned: &[
      "tuple[Width, Height] => Dimensions",
      "tuple[Bound | None, Bound | None] => ValueRange",
      "tuple[Iterator[Dimensions], ...] => MergeInputs",
      "tuple[HttpClient | FakeClient, Credentials | None] => ClientSetup",
      "tuple[anyio.Event, list[Response | BaseException]] => PendingRequest",
      "Generator[_, None, None] => collections.abc.Iterator",
      "(Width, Height) => Dimensions",
      "(Option<Bound>, Option<Bound>) => ValueRange",
  ]
  ```

- **Python `pass`**: `domain_types_used`, `different_tuple_element_types`, `unbanned_generator_send_type`, `override_method_exempt` (E3), `protocol_class_exempt` (E5), `local_variable_annotation_exempt` (E7), `annotated_metadata_ignored` (E9), `local_class_shadowing_stdlib_generator_exempt` (E10), `unrelated_event_module_exempt`.
- **Python `fail`**: `dimensions_tuple_parameter`, `typing_tuple_dimensions_return`, `value_range_reversed_union_order_attribute`, `value_range_typing_optional_return`, `merge_inputs_ellipsis_wildcard_return`, `client_setup_union_parameter`, `pending_request_nested_subscript_attribute`, `generator_yield_only_wildcard_return`, `qualified_collections_abc_generator_parameter`, `nested_inside_list_parameter`, `instance_attribute_in_method`.
- **Rust `pass`**: `domain_structs_used`, `different_tuple_types`, `trait_impl_exempt` (E4), `inline_test_exempt` (E8), `local_let_binding_exempt` (E7).
- **Rust `fail`**: `dimensions_tuple_parameter`, `dimensions_tuple_return`, `dimensions_tuple_struct_field`, `value_range_option_tuple_return`, `nested_vec_of_dimensions_parameter`, `enum_variant_dimensions_field`.
- **Unit tests in `semantic::type_annotations`**:
  - Verify empty `banned` set (`base: &[]`) returns zero matches (E11).
  - Verify pattern without `=>` formats `{replacement}` as `"a dedicated domain type"`.
  - Verify pattern with `=>` formats `{replacement}` as `` `Dimensions` ``.
