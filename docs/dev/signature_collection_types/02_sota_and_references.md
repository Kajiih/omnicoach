# Phase 2: SOTA & References — Signature & Attribute Collection Type Rules

This document records **Phase 2 (Gather Resources and Reference)** for the signature and attribute collection type rule family. It builds on the validated [01_understand.md](01_understand.md) (goals G1–G5, non-goals NG1–NG4, decisions D1–D5, and open questions Q1–Q8).

> Status: **VALIDATED** (2026-10-02). SOTA comparison, CPython `collections.abc` capability lattice, and synthesis for all 5 candidates accepted.

Confidence markers: ✅ verified against official docs/source/CPython this session · ⚠️ from recalled docs/source · ❌ searched, not found.

---

## 1. External State of the Art

### 1.1 Comparison Matrix

| Tool / Specification | Target Scope | What It Enforces / Recommends | How It Handles Nested Types, Returns, and Attributes | Relation to Our 5 Candidate Rules |
| :--- | :--- | :--- | :--- | :--- |
| **Mypy Docs** ("Standard duck types", "Invariance vs covariance", "Incompatible overrides") ✅ | Python signatures & classes | Recommends `Iterable`, `Sequence`, `Mapping`, `MutableMapping` for parameters because `list` and `dict` are **invariant** (`list[Sub]` is not a subtype of `list[Super]`). | **Parameters**: abstract (`Sequence`, `Iterable`, `Mapping`). **Returns**: models concrete returns (`def f(ints: Iterable[int]) -> list[str]`) and notes narrower return types (`list[str]` overriding `Sequence[str]`) satisfy Liskov substitution. **Class/instance attributes**: models concrete `list[str]` (`audit_log: list[str]`, `ClassVar[list[str]]`). | Strongly supports **Candidate 1** (concrete parameter types), **Candidate 2** (`Mapping` vs. `MutableMapping`), and **Candidate 3** (`Iterable` vs. `Sequence`). Highlights why **Candidate 4** (return types) and **Candidate 5** (attributes) need false-positive mitigation when used for end-to-end covariant interfaces. |
| **PEP 484 & PEP 585** ✅ | Python type annotations | Defines variance of generic types (`list`/`dict`/`set` invariant; `Sequence`/`Mapping`/`AbstractSet`/`Iterable` covariant) and moves standard ABC generics to `collections.abc` in Python 3.9+. | Recommends `collections.abc` (`Sequence`, `Mapping`, `Set`, `Iterable`) for parameter annotations; uses `collections.abc.Set` (or `typing.AbstractSet`) to avoid shadowing builtin `set`. | Normative foundation for **Candidate 1–3** and for preferring `collections.abc` imports in rule documentation and fixes. |
| **Google Python Style Guide §2.21** ✅ | Python type annotations | Requires explicit type parameters on generic ABCs (`Sequence[int]`, `Mapping[int, str]`, never bare `Sequence` or `Mapping`). | Uses `Sequence` and `Mapping` in signature examples when mutability is not required. | Reinforces preserving type arguments in diagnostics and examples (`Sequence[T]`, `Mapping[K, V]`). |
| **`flake8-kotoha` (`KTH001`–`KTH003`)** ✅ | Python parameter annotations | Flags concrete `list` / `typing.List` and `dict` / `typing.Dict` in function parameter annotations, suggesting `Sequence` / `Iterable` and `Mapping`. | Inspects **parameter annotations only** (does not flag return types or class/instance attributes). Pure AST signature check without body mutation analysis. | Closest existing Python linter precedent to **Candidate 1** (with D5's pure signature check). |
| **Ruff (`ANN`, `UP`, `PYI`)** ✅ | Python annotations & stubs | `UP006`/`UP035` modernize `typing.List` → `list` and `typing.Sequence` → `collections.abc.Sequence`; `PYI045` checks `__iter__` return types in stubs. **No rule** for `list`/`dict`/`set` → `Sequence`/`Mapping`/`AbstractSet` in parameters ❌. | Does not flag concrete collection types in parameters, returns, or attributes. | Confirms **Candidate 1–3** fill a genuine gap not covered by Ruff. |
| **Rust Clippy `clippy::ptr_arg`** ✅ | Rust function parameters | Flags `&Vec<T>`, `&String`, `&PathBuf`, `&Cow<T>` in function parameters (suggesting `&[T]`, `&str`, `&Path`), and `&mut Vec<T>` when only `DerefMut` (`&mut [T]`) methods are used. | **Parameters only** (never return types or struct fields). Skips trait implementation methods (`impl Trait for Type`) because the signature is fixed by the trait contract. | Direct Rust equivalent (confirming NG1/D3) and precedent for skipping `@override`/protocol/trait methods (`Q6`) and for **Candidate 2**'s body-usage check on mutable references. |
| **`@typescript-eslint/prefer-readonly-parameter-types`** ✅ | TypeScript function parameters | Requires all function parameters (including nested properties and array elements) to be `readonly` (`readonly T[]`, `Readonly<T>`). | **Parameters only**. Recursively checks nested properties/elements, which is widely documented as its main source of friction when callers pass types from third-party libraries or invariant wrappers. | Precedent for **Parameters only** and a cautionary tale for **Q2** (deep recursive annotation checking vs. top-level / union / optional checking). |
| **Java ErrorProne `MixedMutabilityReturnType` / `ImmutableMemberCollection`** ✅ | Java method bodies & fields | `MixedMutabilityReturnType` flags methods whose `return` statements mix mutable (`new ArrayList<>()`) and immutable (`Collections.emptyList()`) collections. | Analyzes *return expressions* and *field initializers* with full Java type resolution, not return type annotations in isolation. | Shows why the legacy linter's return/attribute rules struggled: mutability of returns/fields is a property of the returned/stored *values* and their callers, not the signature annotation alone. |

---

### 1.2 Findings on Open Questions (`Q1`–`Q8`)

#### Q1: Parameters vs. Return Types vs. Class/Instance Attributes

1. **Function & Method Parameters (Candidates 1, 2, 3) — Strong Consensus ✅**:
   - **Variance trap**: In Python's type system, `list[T]`, `dict[K, V]`, and `set[T]` are **invariant** in `T` and `K`/`V` because they support in-place mutation ([Mypy: Invariance vs covariance](https://mypy.readthedocs.io/en/stable/common_issues.html#invariance-vs-covariance), [Mypy: Generics](https://mypy.readthedocs.io/en/stable/generics.html#variance-of-generic-types)). Consequently, if `Dog` subclasses `Animal`, a caller holding `list[Dog]` **cannot** pass it to `def feed(animals: list[Animal]) -> None`—Mypy and Pyright reject the call. Changing the parameter to `Sequence[Animal]` makes the parameter covariant in its element type and accepts `list[Dog]`, `tuple[Dog, ...]`, and `Sequence[Dog]`.
   - **Caller ergonomics**: Concrete parameter types also reject immutable standard types (`tuple`, `frozenset`, `types.MappingProxyType`, `range`, `dict.keys()`) even when the function only reads or iterates over the input.
2. **Return Types (Candidate 4) — Interface Composition Benefits vs. Ownership False Positives**:
   - **Why abstract return types (`Sequence[T]`, `Mapping[K, V]`, `AbstractSet[T]`) make interfaces easier to work with**:
     - **End-to-end zero-copy composition**: Once functions accept `Sequence[T]` or `Mapping[K, V]` (Candidate 1), any function that forwards, slices, or returns a parameter (or returns an attribute typed as `Sequence[T]`) cannot be annotated `-> list[T]` without forcing a `list(...)` copy just to satisfy the type checker.
     - **Covariant composition across API boundaries**: Because `Sequence[T]` is covariant while `list[T]` is invariant, returning `Sequence[Dog]` composes directly into downstream APIs, unions, and containers expecting `Sequence[Animal]` without invariance friction.
     - **Static read-only contract & implementation freedom**: Returning `Sequence[T]` or `Mapping[K, V]` statically prevents callers from mutating returned state (`get_items().append(x)` is rejected by Mypy/Pyright) and lets the implementation switch freely between `list`, `tuple`, cached views, or custom sequences.
   - **Why false positives arise (and how to mitigate them)**:
     - Mypy's default tutorial ([Type hints cheat sheet](https://mypy.readthedocs.io/en/stable/cheat_sheet_py3.html#standard-duck-types)) models `-> list[str]` for factory/builder functions that allocate a new mutable list specifically for the caller to mutate (`.sort()`, `.append()`).
     - **Mitigation levers for Phase 3**: (a) separate rule identity so return types can use `enforcement-mode = "require-explanation"` (requiring an explanatory comment when a function intentionally returns a mutable concrete collection), (b) exempting `@override` / dunders / protocol stubs, or (c) intra-file caller/return heuristics.
3. **Class & Instance Attributes (Candidate 5) — Constructor/Boundary Ergonomics vs. Internal Mutable State**:
   - **Why abstract attribute types (`items: Sequence[T]`, `config: Mapping[K, V]`) make interfaces easier to work with**:
     - **Synthesized `__init__` ergonomics (`@dataclass`, `NamedTuple`, `attrs`, Pydantic)**: On dataclasses and record types, field annotations directly generate the `__init__` parameter signature. If a field is typed `items: list[str]`, callers holding a `tuple[str, ...]`, a `list[SubStr]`, or a `Sequence[str]` parameter from Candidate 1 cannot construct `MyRecord(items=items)` without copying `list(items)`.
     - **Read-only public object interface**: Typing public attributes as `Sequence[T]` / `Mapping[K, V]` prevents external callers from mutating `obj.items.append(...)` behind the object's back and allows subclasses/implementations to satisfy `Protocol` attributes covariantly.
   - **Why false positives arise (and how to mitigate them)**:
     - Classes with internal mutable state (`self._buffer: list[str] = []` where methods call `self._buffer.append(...)`) need a concrete or `Mutable*` type so Mypy allows internal mutation ([Type hints cheat sheet §Classes](https://mypy.readthedocs.io/en/stable/cheat_sheet_py3.html#classes)).
     - **Mitigation levers for Phase 3**: (a) exempting private attributes (`_attr`) or attributes mutated within the class body (`self.attr.append(...)`, `self.attr[k] = v`), (b) targeting public/dataclass fields, and (c) supporting `enforcement-mode = "require-explanation"` when public mutable state is intentional.

---

#### Q2: Annotation Variance & Syntactic Positions

By subtyping rules ([Mypy: Variance of generic types](https://mypy.readthedocs.io/en/stable/generics.html#variance-of-generic-types)):
- **Covariant wrapper positions in parameters**:
  - Top-level annotation: `x: list[int]`
  - PEP 604 union (`binary_operator` with `|`): `x: list[int] | None`, `x: int | list[str] | None`
  - `typing.Optional[T]` and `typing.Union[T1, T2, ...]`: `x: Optional[list[int]]`, `x: Union[list[int], str]`
  - `typing.Annotated[T, meta1, ...]`: the underlying type is the **first** subscript argument (`T`); subsequent subscript arguments are value metadata and must not be traversed as types.
- **Nested container type arguments (`Mapping[str, list[int]]`, `Sequence[list[int]]`)**:
  - Even though `Sequence[T]` is covariant in `T` and `Mapping[K, V]` is covariant in `V`, `Mapping` is **invariant** in its key type `K` (and `dict[K, V]` / `list[T]` are invariant in all type arguments).
  - More importantly, if the outer container is invariant (`MutableMapping[str, list[int]]`, `list[list[int]]`, or a user-defined invariant `Generic[T]`), `dict[str, list[int]]` is **not** a subtype of `MutableMapping[str, Sequence[int]]`. Calling `fn(my_dict_of_lists)` would **fail Mypy type checking** if the inner `list[int]` were changed to `Sequence[int]` inside an invariant outer generic.
- **Contravariant positions (`Callable[[Arg1, ...], Ret]`)**:
  - `Callable` is **contravariant** in its parameter types. In `def run(cb: Callable[[list[int]], None])`, `Callable[[Sequence[int]], None]` is a **subtype** of `Callable[[list[int]], None]`, not a supertype. Replacing `Callable[[list[int]], None]` with `Callable[[Sequence[int]], None]` makes `run` **more restrictive** (rejecting callbacks that expect a `list[int]`), which is the exact opposite of what the rule intends.
- **Implication for Q2**:
  - Descending blindly into arbitrary subscript slices (`ast.Subscript.slice`) is **unsound** inside invariant generics (`MutableMapping[K, list[V]]`, `CustomGeneric[list[T]]`) and contravariant `Callable[[list[T]], R]` argument lists.
  - Conversely, descending into **transparent type wrappers**—top-level, `|` unions, `Optional[...]`, `Union[...]`, and the first argument of `Annotated[T, ...]`—is 100% variance-safe for all three parameter rules (Candidates 1, 2, and 3) and fixes the legacy linter's `Optional[list[int]]` / `Union[list[int], None]` bug without introducing variance errors on nested generics.

---

#### Q3 & Q4: Ground-Truth `collections.abc` Capability Hierarchy (Verified via CPython 3.14 ✅)

Inspecting CPython's `collections.abc` hierarchy and `typeshed` definitions establishes the exact methods and operators at each level:

| ABC (`collections.abc` / `typing`) | Direct Bases | Methods & Operators Provided by This Level | Builtin Concrete Methods **Missing** from the ABC |
| :--- | :--- | :--- | :--- |
| **`Iterable[T]`** | `object` | `__iter__` (single-pass `for x in it`, comprehensions, unpacking, `iter(it)`, `list(it)`, `tuple(it)`, `set(it)`, `sum(it)`, `any(it)`, `all(it)`, `min(it)`, `max(it)`, `sorted(it)`, `enumerate(it)`, `zip(it)`, `map(_, it)`, `filter(_, it)`, `str.join(it)`). **Note**: `bool(it)` / `if it:` on an `Iterator`/`Generator` is always `True`! | `__len__`, `__contains__`, `__reversed__`, `__getitem__`, safe multi-pass iteration, truthiness emptiness check |
| **`Reversible[T]`** | `Iterable[T]` | `__reversed__` (`reversed(x)`) | `__len__`, `__contains__`, `__getitem__` |
| **`Collection[T]`** | `Sized`, `Iterable[T]`, `Container[T]` | `__len__` (`len(x)`, truthiness `if x:` / `bool(x)`), `__contains__` (`v in x`, `v not in x`), multi-pass iteration over a sized materialized container | `__reversed__`, `__getitem__`, `.index()`, `.count()` |
| **`Sequence[T]`** | `Reversible[T]`, `Collection[T]` | `__getitem__` (`x[i]`, `x[i:j]`), `.index(v)`, `.count(v)`, `__reversed__` (`reversed(x)`), sequence `match` pattern (`case [a, b]:`) | `.copy()`, `.sort()`, and all in-place mutation methods |
| **`MutableSequence[T]`** | `Sequence[T]` | `__setitem__` (`x[i] = v`), `__delitem__` (`del x[i]`), `__iadd__` (`x += it`), `.append(v)`, `.clear()`, `.extend(it)`, `.insert(i, v)`, `.pop([i])`, `.remove(v)`, `.reverse()` | **`.sort()`** and **`.copy()`** exist on `list`, **not** on `MutableSequence` ([Mypy Common Issues L515–524](https://mypy.readthedocs.io/en/stable/common_issues.html#invariance-vs-covariance)) |
| **`Mapping[K, V]`** | `Collection[K]` | `__getitem__` (`m[k]`), `.get(k[, d])`, `.keys()`, `.items()`, `.values()`, `__eq__`, `__reversed__` | **`.copy()`** and **`\|`** (`__or__`) exist on `dict`, **not** on `Mapping` |
| **`MutableMapping[K, V]`** | `Mapping[K, V]` | `__setitem__` (`m[k] = v`), `__delitem__` (`del m[k]`), `.clear()`, `.pop(k[, d])`, `.popitem()`, `.setdefault(k[, d])`, `.update(...)` | **`\|=`** (`__ior__`) and **`.copy()`** exist on `dict`, **not** on `MutableMapping` |
| **`Set[T]`** (`typing.AbstractSet[T]`) | `Collection[T]` | Binary operators `&`, `\|`, `-`, `^`, comparisons `<=`, `<`, `>=`, `>`, `==`, `.isdisjoint(other)` | Named methods **`.union()`**, **`.intersection()`**, **`.difference()`**, **`.symmetric_difference()`**, **`.issubset()`**, **`.issuperset()`**, and **`.copy()`** exist on `set`/`frozenset`, **not** on `collections.abc.Set` / `AbstractSet`! |
| **`MutableSet[T]`** | `Set[T]` | `__iand__` (`&=`), `__ior__` (`\|=`), `__isub__` (`-=`), `__ixor__` (`^=`), `.add(v)`, `.clear()`, `.discard(v)`, `.pop()`, `.remove(v)` | Named methods **`.update()`**, **`.intersection_update()`**, **`.difference_update()`**, **`.symmetric_difference_update()`** exist on `set`, **not** on `MutableSet`! |

**Critical Insights from this Table**:
1. **`collections.abc.Set` vs. concrete `set` named methods**: Unlike `list` → `Sequence` and `dict` → `Mapping` (where almost all read-only methods exist on the ABC), `collections.abc.Set` / `typing.AbstractSet` only defines `isdisjoint` and the binary operators (`|`, `&`, `-`, `^`, `<=`, `<`, `>=`, `>`), **not** the named methods `.union()`, `.intersection()`, `.difference()`, `.issubset()`, `.issuperset()`. Any rule documentation or suggestion for `set` → `AbstractSet` / `collections.abc.Set` should note that `AbstractSet` uses operator syntax (`|`, `&`, `-`, `<=`) rather than named `.union()` / `.intersection()` methods.
2. **Sharpness of Candidate 2 (`MutableSequence` / `MutableMapping` / `MutableSet` → `Sequence` / `Mapping` / `AbstractSet`)**:
   - Because the parameter is *already* typed with an abstract `Mutable*` ABC (not concrete `list`/`dict`/`set`), the type checker already restricts direct operations on the parameter to the `Mutable*` ABC interface.
   - The delta between `MutableSequence` and `Sequence`, `MutableMapping` and `Mapping`, and `MutableSet` and `Set` consists strictly of:
     1. **In-place mutating methods**: `append`, `clear`, `extend`, `insert`, `pop`, `remove`, `reverse` (`MutableSequence`); `clear`, `pop`, `popitem`, `setdefault`, `update` (`MutableMapping`); `add`, `clear`, `discard`, `pop`, `remove` (`MutableSet`).
     2. **In-place subscript/augmented writes**: `param[k] = v`, `param[k] += v`, `del param[k]`, `param += x`, `param |= x`, `param &= x`, `param -= x`, `param ^= x`.
     3. **Aliasing / escaping**: passing `param` to another function/method or constructor (unless it is a known non-mutating builtin like `len(param)`, `iter(param)`, `reversed(param)`, `sorted(param)`, `enumerate(param)`, `zip(param)`, `any(param)`, `all(param)`, `sum(param)`, `min(param)`, `max(param)`, `list(param)`, `tuple(param)`, `set(param)`, `frozenset(param)`, `dict(param)`, `bool(param)`), assigning `param` to another variable/attribute/container (`self.x = param`, `other = param`), returning or yielding `param`, or capturing `param` in a nested `def`/`lambda`/`class` where it is mutated or escapes.
3. **Complexity of Candidate 3 (`Sequence` / `Collection` → `Collection` / `Iterable`)**:
   - Narrowing `Sequence[T]` to `Collection[T]` or `Iterable[T]` requires proving the absence of:
     - Indexing/slicing (`param[i]`, `param[i:j]`), `.index()`, `.count()`, `reversed(param)`, sequence `match` patterns (`case [a, b]:`), and `isinstance` checks.
     - For `Iterable[T]` specifically: `len(param)`, `v in param` / `v not in param`, **truthiness checks** (`if param:`, `if not param:`, `while param:`, `bool(param)`, `param and ...`, `param or ...`, ternary `x if param else y`), and **multi-pass iteration** (≥ 2 iteration sites, or any iteration site nested inside a loop/`for`/`while`/comprehension/closure).
     - Any call forwarding `param` to a function that requires `Sequence` or `Collection` (which is **impossible to know intra-procedurally** for user-defined functions or methods like `", ".join(param)` vs. `helper(param)`).
   - Notice the fundamental asymmetry between **Candidate 2** and **Candidate 3**:
     - In **Candidate 2**, most helper functions do *not* require `MutableSequence`; and if `param` is passed to *any* unknown function `helper(param)`, Candidate 2 conservatively treats `param` as escaping (exempt) and *still* catches all functions that only iterate, index, or read `param` locally.
     - In **Candidate 3**, if `param: Sequence[int]` is passed to any helper function or if we conservatively exempt any parameter passed to an unknown function, any function that calls `helper(param)` is exempt, while for functions that don't call helpers, replacing `Sequence[T]` with `Iterable[T]` changes the public contract to accept **single-use iterators/generators**, which is often a deliberate API design choice (many APIs intentionally type parameters as `Sequence[T]` rather than `Iterable[T]` so callers cannot accidentally pass an exhausted generator, or so the implementation can later add `if not items:` or `len(items)` without a breaking signature change).

---

## 2. Internal Codebase References & Constraints

### 2.1 Reusable AST & Rule Infrastructure

| Need | Existing Building Block | Location |
| :--- | :--- | :--- |
| Extracting Python function signatures & parameters | `extract_function_signatures`, `PythonFunctionSignature`, `PythonParameterInfo`, `PythonParameterKind` | [src/code_lint/ast/python.rs](../../../src/code_lint/ast/python.rs) |
| Skipping `*args` and `**kwargs` | `PythonParameterInfo::is_variadic()` (retained specifically for the legacy linter signature rules) | [src/code_lint/ast/python.rs](../../../src/code_lint/ast/python.rs) |
| Skipping `self` and `cls` receivers | `PythonParameterKind::Receiver` | [src/code_lint/ast/python.rs](../../../src/code_lint/ast/python.rs) |
| Checking `@override`, `@overload`, `@abstractmethod`, `@fixture` decorators | `has_decorator(func_node, ...)` and `has_override_decorator` | [src/code_lint/ast/python.rs](../../../src/code_lint/ast/python.rs), [src/code_lint/rules/identical_positional_types.rs](../../../src/code_lint/rules/identical_positional_types.rs) |
| Checking `Protocol` and `ABC` enclosing classes | `extract_classes`, `PythonClassInfo::inherits_from("Protocol")`, `inherits_from("ABC")` | [src/code_lint/ast/python.rs](../../../src/code_lint/ast/python.rs) |
| Write-target detection (`x[i] = ...`, `del x[i]`, `x += ...`) | `is_write_target`, `TARGET_CONTAINER_KINDS`, `MUTATING_METHODS` | [src/code_lint/ast/python.rs](../../../src/code_lint/ast/python.rs) |
| Classification topics | `Topic::STATIC_TYPING` | [src/rule_declaration.rs](../../../src/rule_declaration.rs) |

### 2.2 Registry & Style Constraints ([tests/registry.rs](../../../tests/registry.rs))

Every rule added to `src/code_lint/rules/` is validated by [tests/registry.rs](../../../tests/registry.rs) and [naming_and_message_style_guide.md](../naming_and_message_style_guide.md):
1. **Rule name grammar** ([tests/registry.rs](../../../tests/registry.rs)):
   - `kebab-case`, **at most 4 words** (`MAX_RULE_NAME_WORDS = 4`).
   - Must **name the flagged pattern**, never the fix or policy (`no-`, `prefer-`, `enforce-`, `banned-`, `max-`, `min-`, `-enforced` are forbidden).
   - File stem must be the `snake_case` of a rule declared in the file, and single-rule files expose `pub const RULE`.
2. **Template fields** ([tests/registry.rs](../../../tests/registry.rs)):
   - `summary`: 1 sentence ending with `.`, starts with uppercase or backtick, no single/double quotes outside backticks, no fix verbs (`use`, `replace`, `add`, `rename`, `remove`), no judgement words (`banned`, `forbidden`, `discouraged`, `illegal`, `must`), no `e.g.`/`i.e.`.
   - `rationale`: 1–2 sentences ending with `.`, no `should` / `must`, does not start with a fix verb.
   - `suggestion`: 1–2 sentences ending with `.`, starts with an allowed verb from `SUGGESTION_VERBS` (`Replace`, `Narrow`, `Specify`, etc. — or extend the list if a new verb is needed).
   - `RuleDoc::summary`: 1 sentence starting with `Flags ` or `Requires `.
3. **Placeholders** ([tests/registry.rs](../../../tests/registry.rs)):
   - Current shared placeholders include `{function}`, `{class}`, `{name}`, `{expression}`, `{token}`, etc. If a new placeholder (such as `{annotation}` or `{replacement}`) is needed, [naming_and_message_style_guide.md](../naming_and_message_style_guide.md) explicitly instructs: *"when a message needs a thing none of these names, add a row here and an entry in the vocabulary test, in the same change."*
4. **Test harness (`rule_test!`)**:
   - Every `fail` test case must produce **exactly 1 diagnostic** (and 2 when duplicated by the harness). Therefore, if a rule emits **1 diagnostic per offending parameter** (or 1 consolidated diagnostic per function), each `fail` test snippet must trigger exactly 1 diagnostic.

---

## 3. Initial Synthesis for Phase 3 (Test Matrix & Prototypes)

Based on the external SOTA and CPython `collections.abc` analysis above, here is how the 5 candidates stand entering Phase 3:

| Candidate | SOTA & Type-Theory Signal | Key Technical Questions to Prototype in Phase 3 |
| :--- | :--- | :--- |
| **1. Concrete parameter types** (`list`, `dict`, `set`, `typing.List`, `Dict`, `Set`) | **Highest signal ✅**: backed by Mypy, PEP 484/585, Google Style Guide, `flake8-kotoha`, and `clippy::ptr_arg`. | - Compare annotation traversal depths (top-level + `\|` / `Optional` / `Union` / `Annotated` vs. covariant nested generics).<br>- Compare per-parameter vs. per-function diagnostic aggregation.<br>- Verify exemptions (`*args`/`**kwargs`, `@override`, dunders, FastAPI/Typer/Click/Pydantic decorators if applicable). |
| **2. Unused mutable abstract parameter types** (`MutableSequence`, `MutableMapping`, `MutableSet`) | **Strong signal ✅**: backed by Mypy ("Standard duck types") and `clippy::ptr_arg` (`&mut Vec<T>`). Closed, exact set of mutating methods/operators on `Mutable*` ABCs. | - Prototype single-pass intra-procedural mutation + escape detector in `ast::python` against a comprehensive test matrix (method calls, subscript/augmented writes, call argument forwarding, aliasing, closures, stubs/protocols/abstract methods). |
| **3. Overly specific read-only parameter types** (`Sequence` / `Collection` → `Collection` / `Iterable`) | **Low / conflicted signal ⚠️**: no external linter implements this; narrowing `Sequence` to `Iterable` admits single-use generators, breaks if callers pass `Sequence` to unknown helpers, and conflicts with intentional API contracts. | - Evaluate concrete false-positive cases in the Phase 3 test matrix (helper forwarding, truthiness `if items:`, multi-pass loops, defensive `Sequence` contracts) to decide whether to include or defer to `ROADMAP.md`. |
| **4. Concrete return types** (`-> list[T]`, `-> dict[K, V]`, `-> set[T]`) | **High interface value with ownership FP risk ⚠️**: enables end-to-end zero-copy composition with `Sequence`/`Mapping` parameters and covariant read-only return contracts, but triggers FPs on builder/factory functions that return freshly owned mutable collections. | - Prototype in Phase 3 test matrix.<br>- Compare mitigation strategies: separate rule (`enforcement-mode = "require-explanation"` compatibility), intra-file caller/return mutation heuristics, and `@override`/dunder exemptions. |
| **5. Concrete class/instance attribute types** (`items: list[T]`) | **High boundary value (especially `@dataclass` / `NamedTuple` `__init__` fields) with internal-state FP risk ⚠️**: allows passing `Sequence`/`Mapping` directly into synthesized constructors without `list(...)` copies, but triggers FPs on internal mutable attributes (`self._buf.append(...)`). | - Prototype in Phase 3 test matrix.<br>- Compare mitigation strategies: exempting private attributes (`_attr`), exempting attributes mutated inside the class body (`self.attr.append(...)`), targeting public/dataclass fields, and `require-explanation` mode. |

---

## 4. Sources

- Mypy Documentation — *Type hints cheat sheet (Standard "duck types" & Classes)*: https://mypy.readthedocs.io/en/stable/cheat_sheet_py3.html
- Mypy Documentation — *Common issues and solutions (Invariance vs covariance & Incompatible overrides)*: https://mypy.readthedocs.io/en/stable/common_issues.html
- Mypy Documentation — *Generics (Variance of generic types)*: https://mypy.readthedocs.io/en/stable/generics.html
- Python Standard Library — `collections.abc` (*Abstract Base Classes for Containers*): https://docs.python.org/3/library/collections.abc.html
- PEP 484 — *Type Hints*: https://peps.python.org/pep-0484/
- PEP 585 — *Type Hinting Generics In Standard Collections*: https://peps.python.org/pep-0585/
- Google Python Style Guide §2.21 (*Type Annotated Code*): https://google.github.io/styleguide/pyguide.html#221-type-annotated-code
- `flake8-kotoha`: https://pypi.org/project/flake8-kotoha/
- Rust Clippy `clippy::ptr_arg`: https://rust-lang.github.io/rust-clippy/master/index.html#ptr_arg
- TypeScript ESLint `@typescript-eslint/prefer-readonly-parameter-types`: https://typescript-eslint.io/rules/prefer-readonly-parameter-types/
- Java ErrorProne `MixedMutabilityReturnType`: https://errorprone.info/bugpattern/MixedMutabilityReturnType
