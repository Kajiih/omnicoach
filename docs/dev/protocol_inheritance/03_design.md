# Explicit Protocol Inheritance: 03 Design

> [!NOTE]
> **Status: VALIDATED** (user, 2026-10-14; implemented). Open questions resolved with the recommended answers.

## 1. Verdict and Decisions

- **Verdict:** **Narrow** the legacy linter's repo-wide `RequireExplicitProtocolInheritanceRule` to a single-file rule **`uninherited-protocol`** (alternative name: `implicit-protocol-implementation`), and record the cross-file counterpart plus `UnusedInterfaceMethodRule` on `ROADMAP.md` under project-wide symbol/call index candidates.

| # | Decision | Why |
| :-- | :--- | :--- |
| D1 | **One single-file rule (`uninherited-protocol`), plus two `ROADMAP.md` entries** (cross-file protocol inheritance and `UnusedInterfaceMethodRule`). | Matches Omni's single-file rule architecture ([decisions/001_architecture_and_design.md](../decisions/001_architecture_and_design.md)) and the split test ([01_understand.md](01_understand.md) §4): every single-file violation has the same rationale and the same fix (add the `Protocol` to the class's bases). |
| D2 | **A class is a `Protocol` (`PythonClassInfo::is_protocol()`) iff a direct base resolves to `typing.Protocol` or `typing_extensions.Protocol`** after peeling `Expr::Subscript` (`Protocol[T]`). Unresolved or non-`typing` bases named `Protocol` (such as `asyncio.Protocol`) are not protocols (E7). | Follows [PEP 544](https://peps.python.org/pep-0544/#merging-and-extending-protocols): a subclass of a protocol is only itself a protocol if `Protocol` is in its direct bases; otherwise it is a concrete implementation. Import resolution fixes the legacy linter's P4 (`asyncio.Protocol`). |
| D3 | **Protocol method set = public non-property methods (`def` / `async def` not starting with `_`), unioned across in-file ancestor protocols (`local_ancestors`), gated by `min-methods` (default `2`, E4).** | A sub-protocol `class ReadWriteStore(ReadStore, Protocol):` requires both `ReadStore`'s methods and its own. The `min-methods = 2` default filters out 80.3% of protocols (0- and 1-method structural/duck-typing protocols) and achieved a 100% true-positive rate across 13,198 scanned files ([02_references.md](02_references.md) §4.2). |
| D4 | **Exclude `_`-prefixed/dunder methods (E5), `@property` getters, and `AnnAssign` attributes (E6) from the matched method set.** | Dunder methods (`__init__`, `__repr__`, `__enter__`, `__exit__`, `__iter__`, `__call__`) are either universal object lifecycle hooks or structural operator/context-manager protocols (46 of 122 protocols in the corpus). Excluding `@property` and `AnnAssign` fixes the legacy linter's P1/P2, where a protocol with `name: str` failed to match concrete classes that set `self.name = ...` in `__init__` or via `@dataclass` fields, and avoids matching a candidate's `@property` against a protocol's callable method. See Q2. |
| D5 | **Match against the candidate class's direct public non-property methods (`class.methods()`).** | Fixes the subclass cascade found on `pkg_resources.NullProvider` ([02_references.md](02_references.md) §4.3): `NullProvider` defines all 12 methods of `IResourceProvider`, while 8 subclasses (`EggProvider(NullProvider)`, etc.) inherit them and override only 1–3 methods. Checking direct methods flags `NullProvider` once; once `NullProvider` inherits from `IResourceProvider`, all 8 subclasses inherit it transitively. |
| D6 | **Candidate classes with other non-protocol base classes (`class CachedStore(LoggingMixin):`) are still checked.** | Unlike `fake-without-protocol` (which cannot see cross-file base classes and therefore accepts any collaborator base), `uninherited-protocol` knows the exact in-file `Protocol` name and checks whether that `Protocol` is in the candidate's bases or `local_ancestors`. Normal class bases (`type`) and `ABC` bases (`ABCMeta`) have no metaclass conflict with `Protocol` (`_ProtocolMeta`). See Q1. |
| D7 | **Exempt candidate classes that are `Protocol` or `ABC` interfaces (`is_protocol() \|\| is_abc()`, E2), or `Enum`, `TypedDict`, or `NamedTuple` classes (`is_enum() \|\| is_typed_dict() \|\| is_named_tuple()`, E3).** | `Protocol` and `ABC` classes are interface definitions, not concrete implementations. `Enum`, `TypedDict`, and `NamedTuple` classes have metaclasses (`EnumType`, `_TypedDictMeta`, `NamedTupleMeta`) that raise `TypeError: metaclass conflict` at runtime if combined with a `Protocol` base ([02_references.md](02_references.md) §2).Fixes the legacy linter's P6. |
| D8 | **Most-derived protocol wins (E8):** when a candidate class matches both a base protocol `BaseProto` and a sub-protocol `SubProto(BaseProto, Protocol)` in the same file, suppress `BaseProto` and report only `SubProto`. | Inheriting from `SubProto` already transitively inherits from `BaseProto`, and adding both as `class C(BaseProto, SubProto):` raises a Python MRO `TypeError` ([02_references.md](02_references.md) §2). Fixes the legacy linter's P5 (`NullProvider` matching both `IMetadataProvider` and `IResourceProvider`). |
| D9 | **Scope and partition with `fake-without-protocol` (E9):** `RuleTarget::AllFiles`, exempting bare `Fake*` classes that have no collaborator base class (`is_fake_class_name && !has_collaborator_base`). | Bare `Fake*` classes without a base class are already owned by `fake-without-protocol`. Exempting them by AST predicate prevents duplicate findings when a test file defines both `Store(Protocol)` and `class FakeStore:`, while still catching `class FakeStore(BaseFake):` (which has a non-protocol helper base) and non-`Fake*` test doubles (`StubStore`, `InMemoryStore`). See Q3. |
| D10 | **Lexical protocol binding for `RepeatCheck::Enforce`:** for a candidate class at `index`, deduplicate protocols by `name`, binding each protocol `name` to the last protocol with that name before `index` (or the first after `index` if none precedes it), and treat `protocol.name` as inherited (E1) when any direct base of `class` or any ancestor in `local_ancestors(classes, index)` has `name == protocol.name`. | When `rule_test!` duplicates `{code}\n{code}` into `[Store_0, MemoryStore_1, Store_2, MemoryStore_3]`, each `MemoryStore` matches against a single `Store` binding and reports once (`[span, span + offset]`), and `local_ancestors` on `MemoryStore_3` resolves `Store` to `Store_2`. |
| D11 | **Anchor on `class.name_node`.** | Matches `fake-without-protocol` and Omni's class-level diagnostic convention (fixes the legacy linter's P8). |
| D12 | **Classification:** `Topic::STATIC_TYPING`, `Precision::Heuristic`, `Consensus::Opinionated`, `ImpactedQuality::Reliability`. Default enforcement mode: `ban`. | Method matching is name-based without signature type unification (`Precision::Heuristic`); PEP 544 permits structural subtyping (`Consensus::Opinionated`); uninherited implementations bypass class-definition type checking and abstract instantiation checks (`ImpactedQuality::Reliability`). |

---

## 2. Exemptions

Every standalone exemption below gets a dedicated `pass` case in `rule_test!` and a corresponding `Mutation` entry in `scripts/exemptions/`.

| # | Exemption | Pass case (sketch) |
| :-- | :--- | :--- |
| E1a | Candidate class directly inherits from the `Protocol` (including generic `Store[int]`). | `class MemoryStore(Store):` (and `class IntStore(Store[int]):`) implementing `read` and `write`. |
| E1b | Candidate class transitively inherits from the `Protocol` through an in-file base class (`local_ancestors`). | `class BaseStore(Store): ...` and `class CachingStore(BaseStore):` implementing `read` and `write`. |
| E2a | Candidate class is itself a `Protocol` (`is_protocol()`). | `class AsyncStore(Protocol):` declaring the same `read` and `write` methods as `Store(Protocol)`. |
| E2b | Candidate class is an `ABC` (`is_abc()`, via `ABC`/`ABCMeta` base or `metaclass=ABCMeta`). | `class AbstractStore(ABC):` declaring `read` and `write`. |
| E3a | Candidate class is an `Enum` (`is_enum()`). | `class StorageMode(Enum):` defining `read` and `write` methods. |
| E3b | Candidate class is a `TypedDict` (`is_typed_dict()`). | `class StoreRecord(TypedDict):` in a file with a protocol. |
| E3c | Candidate class is a `NamedTuple` (`is_named_tuple()`). | `class StoreEntry(NamedTuple):` defining `read` and `write` methods. |
| E4 | `Protocol` has fewer than `min-methods` (default `2`) public non-property methods. | `class Closable(Protocol): def close(self) -> None: ...` and `class Resource: def close(self) -> None: pass`. |
| E5 | Private (`_`-prefixed) and dunder (`__`-surrounded) methods on a `Protocol` are not counted as protocol methods. | `class Managed(Protocol):` with `def open(self)` + `def __enter__(self)` + `def _reset(self)` (only 1 public method < 2) and a class defining `open`, `__enter__`, `_reset`. |
| E6 | `@property` getters and `AnnAssign` attributes on a `Protocol` are not counted as protocol methods; `@property` getters on a candidate class do not count as method implementations. | `class NamedReader(Protocol):` with `def read(self)` + `@property def name(self)` (only 1 public method < 2), and a candidate class where `read` is a `@property` instead of a regular method. |
| E7 | Base class named `Protocol` that does not resolve to `typing.Protocol` or `typing_extensions.Protocol` (such as `asyncio.Protocol`) is not a structural `Protocol`. | `from asyncio import Protocol` with `class FramingProtocol(Protocol):` (`data_received`, `connection_lost`) and a class defining those methods. |
| E8 | Base `Protocol` is suppressed when the candidate class also matches a derived sub-`Protocol` in the same file that inherits from it. | `class Reader(Protocol):` (`read`, `seek`), `class ReadWriter(Reader, Protocol):` (`write`), and `class Buffer:` defining `read`, `seek`, `write` → reports only `ReadWriter` (verified as a `fail` case asserting a single finding for `ReadWriter`, plus mutation). |
| E9 | Bare `Fake*` class without a collaborator base class (`is_fake_class_name && !has_collaborator_base`), deferred to `fake-without-protocol`. | `class FakeStore:` (no base class) implementing `Store(Protocol)`'s `read` and `write` methods. |

---

## 3. Violation Message

```rust
const TEMPLATE: ViolationTemplate = violation_template! {
    summary: "Class `{class}` implements every method of `Protocol` `{name}` without inheriting from it.",
    rationale: "Without `{name}` in the base class list, the type checker only verifies `{class}` against `{name}` at typed call sites, misses omitted methods on instantiation, and rejects `@override` on its methods.",
    suggestion: "Add `{name}` to the base classes of `{class}`.",
};
```

- **Placeholders:** `{class}` and `{name}` are both in the closed placeholder list (`tests/registry.rs`).
- **Opening suggestion verb:** `Add` is in `SUGGESTION_VERBS`.
- **No mode-coupled wording:** neither `ViolationTemplate` nor `RuleDoc` contains `"comment"` or `"require-explanation"`.

---

## 4. Documentation Content (Prediction Test)

- **`summary`:** `"Flags Python classes that implement every method of an in-file Protocol without inheriting from it."`
- **`what_it_does`:**
  Flags a Python class that defines every public method of a `typing.Protocol` (or `typing_extensions.Protocol`) declared in the same file, when the `Protocol` has at least `min-methods` public methods (including methods inherited from in-file base protocols) and the class does not inherit from that `Protocol` directly or through an in-file base class.

  Private (`_`-prefixed) methods, dunder methods, `@property` getters, and annotated attributes do not count toward `min-methods`. Not checked: `Protocol` and `ABC` classes, `Enum`, `TypedDict`, and `NamedTuple` classes (which raise a `TypeError` metaclass conflict if combined with `Protocol`), and bare `Fake*` classes without a base class (covered by `fake-without-protocol`).
- **`why_is_this_bad`:**
  While `Protocol` supports structural subtyping for external types you do not control, omitting explicit inheritance on a class defined alongside the `Protocol` gives up static checks at the class definition site:
  - `mypy` and `pyright` do not check parameter or return types on the class's methods until an instance is passed to a `Protocol`-annotated call site;
  - adding a new method or attribute to the `Protocol` does not flag `ClassName()` instantiation as abstract;
  - `@typing.override` is rejected on the class's methods because no base method exists in the MRO;
  - runtime `inspect.getdoc()` and Omni's override-aware rules cannot see that the methods implement the `Protocol`.
- **`known_problems`:**
  - Matching is single-file and based on public method names (`Precision::Heuristic`): two unrelated classes in the same file that share `min-methods` method names with a local `Protocol` will be flagged even if their parameter or return types are intentionally different.
  - Cross-file `Protocol` definitions imported from another module are not visible to single-file analysis.
  - A protocol whose contract consists primarily of `@property` getters or data attributes plus fewer than `min-methods` regular methods is not checked.
- **`references`:**
  - `PEP 544 — Protocols: Explicitly Declaring Implementation` (`https://peps.python.org/pep-0544/#explicitly-declaring-implementation`)
  - `Python Typing Specification — Protocols` (`https://typing.python.org/en/latest/spec/protocol.html`)
- **`examples`:**
  - Flagged (`flagged_span: "MemoryStore"`):
    ```python
    from typing import Protocol

    class KeyValueStore(Protocol):
        def get(self, key: str) -> bytes | None: ...
        def put(self, key: str, value: bytes) -> None: ...

    class MemoryStore:
        def get(self, key: str) -> bytes | None:
            return None

        def put(self, key: str, value: bytes) -> None:
            pass
    ```
  - Fixed:
    ```python
    from typing import Protocol

    class KeyValueStore(Protocol):
        def get(self, key: str) -> bytes | None: ...
        def put(self, key: str, value: bytes) -> None: ...

    class MemoryStore(KeyValueStore):
        def get(self, key: str) -> bytes | None:
            return None

        def put(self, key: str, value: bytes) -> None:
            pass
    ```

---

## 5. Options

One `CountOption` wrapped in `RuleOptions::code_rule(MIN_METHODS)`:

```rust
const MIN_METHODS: CountOption = CountOption {
    key: "min-methods",
    doc: "Minimum public methods on a `Protocol` for explicit inheritance to be required.",
    default: LanguageDefaults::new(2, &[]),
};
```

---

## 6. Shared AST Helpers Needed (`src/code_lint/ast/python/classes.rs`)

`src/code_lint/ast/python/classes.rs` already provides `extract_classes`, `PythonClassInfo`, `PythonBaseClass`, `local_base_classes`, `local_ancestors`, `PythonClassInfo::is_enum()`, `is_protocol_or_abc_class`, and `is_typed_dict_class`. The following small additions on `PythonClassInfo` and `PythonFunctionSignature` are proposed for Phase 4 so that every exemption `E1..E9` stays visible and individually mutable in `src/code_lint/rules/uninherited_protocol.rs`:

| # | Helper | Description |
| :-- | :--- | :--- |
| H1 | `PythonClassInfo::is_protocol(&self) -> bool` | `true` when any direct base (peeling `Expr::Subscript` for `Protocol[T]`) resolves via `file.resolve_python_call` to `typing.Protocol` or `typing_extensions.Protocol`. |
| H2 | `PythonClassInfo::is_abc(&self) -> bool` | `true` when any direct base (peeling `Expr::Subscript`) resolves to `abc.ABC` or `abc.ABCMeta`, or `metaclass=ABCMeta` is present in class keywords. (`is_protocol_or_abc_class` becomes `is_protocol_class \|\| is_abc_class`.) |
| H3 | `PythonClassInfo::is_typed_dict(&self) -> bool` and `PythonClassInfo::is_named_tuple(&self) -> bool` | Expose the existing `is_typed_dict_class` flag on `PythonClassInfo`, plus `is_named_tuple` (direct base resolving to `typing.NamedTuple` or `typing_extensions.NamedTuple`). |
| H4 | `PythonFunctionSignature::is_property(&self) -> bool` | `true` when `decorators` contains `property`, `cached_property`, or `abstractproperty` (already matched in `classes.rs` line 778 for property getters). |
| H5 | `pub(crate) fn is_fake_class_name(name: &str) -> bool` and `pub(crate) fn has_collaborator_base(class: &PythonClassInfo, file: &ParsedFile) -> bool` | Share or call the existing predicate from `fake_without_protocol.rs` (or keep a 6-line helper in `uninherited_protocol.rs` so E9 can be mutated directly in `uninherited_protocol.rs` without touching `fake_without_protocol.rs`). |

---

## 7. Open Questions for the User

| # | Question | Recommendation |
| :-- | :--- | :--- |
| Q1 | Should a candidate class that already inherits from another concrete or mixin base class (`class CachedStore(BaseCache):`) still be required to add the in-file `Protocol` to its base classes (`class CachedStore(BaseCache, Store):`), or should any non-`object` base class exempt it? | **Require the `Protocol` base (D6).** Normal classes have no metaclass conflict with `Protocol`, and a class inheriting from a generic mixin or helper base (`LoggingMixin`, `BaseFake`) still loses static protocol verification unless the `Protocol` is in its MRO. |
| Q2 | Should `@property` getters and class-body `AnnAssign` attributes on a `Protocol` be excluded from the matched member set (D4/E6), or should `@property` / attributes also be matched against candidate class attributes and properties? | **Exclude them and match on public methods only (D4).** Fixes the legacy linter's P1/P2 bugs where protocols with annotated attributes failed to match classes initializing `self.attr` in `__init__`, and achieved 100% precision on the 13,198-file corpus. |
| Q3 | How should `uninherited-protocol` partition with `fake-without-protocol`? Options: (a) `RuleTarget::AllFiles` and exempt bare `Fake*` classes (`is_fake_class_name && !has_collaborator_base`) covered by `fake-without-protocol`; (b) `RuleTarget::SourceOnly`; (c) `RuleTarget::AllFiles` with no `Fake*` exemption. | **Option (a) (D9, E9).** Avoids duplicate diagnostics on bare `class FakeStore:` while still catching `class FakeStore(BaseFake):` and non-`Fake*` test doubles (`InMemoryStore`, `StubStore`) in test files. |
| Q4 | Rule name: `uninherited-protocol` vs. `implicit-protocol-implementation`? | **`uninherited-protocol`** (2 words, matches `unslotted-dataclass`, `uncolocated-helper`, `unstructured-task`). |

---

## 8. ROADMAP Additions (for Phase 4 / Lead Integration)

1. **Cross-file `uninherited-protocol` (under *Rules needing a project-wide symbol or call index*):**
   - Already noted under *Test double without a collaborator contract* (`ROADMAP.md` line 86: *"or a class that implements every method of a project `Protocol` without inheriting from it"*); update that bullet once `uninherited-protocol` lands to state that single-file `Protocol` matching is implemented as `uninherited-protocol` and only cross-file `Protocol` resolution remains.
2. **Unused `Protocol` or `ABC` interface method (`UnusedInterfaceMethodRule`, under *Rules needing a project-wide symbol or call index*):**
   - Add a bullet for detecting `Protocol` / `ABC` methods with no call sites across the project via a type-aware call index (noting why bare attribute-name matching across files produces false negatives on common method names like `get`, `read`, and `run`).
3. **Rust analogue:**
   - None needed: Rust traits are strictly nominal (`impl Trait for Type` is mandatory in Rust; a struct with matching inherent methods never satisfies a trait bound without an explicit `impl` block).
