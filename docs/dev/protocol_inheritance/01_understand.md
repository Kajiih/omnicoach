# Explicit Protocol Inheritance: 01 Understand

> [!NOTE]
> **Status: VALIDATED** (user, 2026-10-14; implemented). Open questions resolved with the recommended answers.

## 1. What the legacy linter checks

### 1.1 `RequireExplicitProtocolInheritanceRule` (`legacy_lints.py` lines 1016–1177)

The legacy linter runs a two-pass check across `src/` and `tests/`:

1. **Pass 1 (`discover_protocol_signatures`, lines 1063–1110):**
   - Walks every `.py` file under `src/` and `tests/` and inspects every `ast.ClassDef`.
   - Identifies a class as a `Protocol` when `"Protocol"` appears among its direct base terminal names (`_get_base_name`, lines 1016–1024).
   - Extracts public members (`_extract_protocol_members`, lines 1027–1041):
     - `ast.FunctionDef` and `ast.AsyncFunctionDef` whose name does not start with `"_"` (excluding both single-underscore private methods and dunder methods such as `__init__`, `__call__`, `__enter__`, `__exit__`, `__iter__`, and `__len__`).
     - `ast.AnnAssign` in the class body whose `target` is an `ast.Name` not starting with `"_"`.
   - Unions transitive members across base protocols via a global `dict[str, set[str]]` keyed by bare class name (`_resolve_transitive_protocol_members`, lines 1044–1060).
   - Keeps only protocols with at least `MIN_MEMBERS_FOR_NOMINAL = 2` members (line 1013).

2. **Pass 2 (`RequireExplicitProtocolInheritanceRule.check`, lines 1113–1176):**
   - Skips any `ast.ClassDef` that has `"Protocol"` in its direct base names.
   - Collects `method_names` directly defined in `node.body` (`ast.FunctionDef` and `ast.AsyncFunctionDef` only).
   - Computes `all_inherited_bases` by walking a global `dict[str, set[str]]` (`_CLASS_BASES`) keyed by bare class name.
   - Reports a violation whenever `protocol_name not in all_inherited_bases` and `all(m in method_names for m in sig_methods)`.

### 1.2 Flaws and limitations in the legacy linter's implementation

| # | Flaw | Consequence |
| :-- | :--- | :--- |
| P1 | **Attribute / method extraction asymmetry.** `_extract_protocol_members` adds `AnnAssign` attributes (`name: str`) to `sig_methods`, so a protocol with 1 method + 1 annotated attribute passes `len >= 2`. However, `check()` only collects `FunctionDef` / `AsyncFunctionDef` from `node.body` into `method_names`. | A protocol that declares any `AnnAssign` attribute (`name: str`) can **never** match a concrete implementation that initializes `self.name = ...` in `__init__` or declares `name: str` as a class/dataclass attribute; it only matches if the class implements `name` as a `@property` method. |
| P2 | **`@property` vs. method conflation.** Both `_extract_protocol_members` and `check()` treat `@property def status(self) -> str:` identically to `def status(self) -> str:`. | A protocol `@property` satisfied on a concrete class by a plain attribute (`self.status = "ok"`) is missed, whereas a candidate class with `@property def read(self)` falsely matches a protocol method `def read(self)`. |
| P3 | **Global bare-name table collisions.** `raw_members` and `_CLASS_BASES` are indexed globally by bare class name (`node.name`). | Two classes or protocols in different modules with the same name (`Store`, `Client`, `Config`, `Handler`) overwrite or merge their members and base classes across the repository. |
| P4 | **Unresolved `"Protocol"` terminal name.** `_get_base_name` checks `name == "Protocol"` without import resolution. | A class inheriting from `asyncio.Protocol` (a concrete `object` subclass in `asyncio.protocols`) or a domain `Protocol` class is falsely treated as a `typing.Protocol` interface. |
| P5 | **Sub-protocol cascade.** When `SubProtocol(BaseProtocol, Protocol)` extends `BaseProtocol(Protocol)`, any class implementing `SubProtocol` necessarily implements `BaseProtocol`. | The legacy linter reports **two** violations on the same class (`Inherit from BaseProtocol` and `Inherit from SubProtocol`), even though inheriting from `SubProtocol` already transitively inherits from `BaseProtocol` (and listing `[BaseProtocol, SubProtocol]` in that order raises a Python MRO `TypeError`). |
| P6 | **No exemption for `ABC`, `Enum`, `TypedDict`, or `NamedTuple`.** The legacy linter only skips classes with `"Protocol"` in their direct bases. | Abstract base classes (`ABC`) are interfaces, not concrete implementations; `Enum`, `TypedDict`, and `NamedTuple` classes cannot inherit from a `typing.Protocol` at runtime without raising `TypeError: metaclass conflict`. |
| P7 | **Repository-wide pre-pass.** The legacy linter walks `src/` and `tests/` before linting. | In Omni's architecture ([decisions/001_architecture_and_design.md](../decisions/001_architecture_and_design.md)), code rules operate on a single `ParsedFile`; cross-file protocol resolution belongs to the project-wide symbol/call index on [ROADMAP.md](../../../ROADMAP.md). |
| P8 | **Diagnostic span on whole class.** The legacy linter anchors on the `ast.ClassDef` node (`line 1, col 0`). | Omni's class rules anchor on the class identifier (`class.name_node`). |

### 1.3 Brief evaluation of `UnusedInterfaceMethodRule` (`legacy_lints.py` lines 3483–3536)

The legacy linter's `UnusedInterfaceMethodRule` pre-scans every `.py` file under `src/<package>` to collect all accessed `ast.Attribute.attr` names into `CALLED_ATTRIBUTES`, then flags any non-dunder method on a `Protocol` or `ABC` class whose name never appears in `CALLED_ATTRIBUTES`.

- **Why no single-file counterpart exists:** `Protocol` and `ABC` classes exist to define contracts across module boundaries. In the module that defines a `Protocol` or `ABC`, its methods are normally uncalled (or called only in other modules), so a single-file check would flag almost every interface method in the codebase.
- **Why even a project-wide rule requires a type-aware symbol/call index:** Matching bare attribute names (`node.attr`) across all files causes both false negatives (any unrelated `dict.get()`, `file.read()`, or `task.run()` call anywhere in the repository silences an unused `Protocol.get`, `Protocol.read`, or `Protocol.run`) and false positives (dynamic dispatch via `getattr`, framework callbacks, or public library interfaces called outside the repository).
- **Verdict for `UnusedInterfaceMethodRule`:** Purely a project-wide symbol/call-index candidate for [ROADMAP.md](../../../ROADMAP.md); do not implement as a single-file rule.

---

## 2. What problem the check actually solves (and the core tension)

### 2.1 The core tension: why use a `Protocol` if implementations inherit from it?

A `typing.Protocol` serves two distinct roles in Python ([PEP 544 §Explicitly Declaring Implementation](https://peps.python.org/pep-0544/#explicitly-declaring-implementation)):

1. **Consumer-owned structural interfaces for external or pre-existing types** (`SupportsInt`, `Iterable`, `ReadSeekBinary` satisfied by stdlib `io.BytesIO`, or adapters over third-party SDK objects whose base classes the author cannot modify) and **single-method duck-typed capabilities** (`SupportsClose`, callback protocols). Here, implicit structural subtyping is the entire point of `Protocol`.
2. **First-party collaborator contracts** (`ContentStore`, `PyInfoCache`, `StorageBackend`) where a codebase uses `Protocol` instead of `abc.ABC` to avoid `@abstractmethod` boilerplate on every method and to allow structural satisfaction by external callers, while also defining first-party concrete implementations, default/null providers, caching wrappers, or in-memory implementations in the same module or package.

When a **first-party class** (especially one colocated in the same module as a multi-method `Protocol`) implements every method of that `Protocol` without listing it in its base classes, five concrete failure modes arise:

1. **Silent signature drift at class definition time (`mypy` and `pyright`):**
   Without `(ProtocolName)` in the base list, neither `mypy` nor `pyright` checks the class's method signatures against the `Protocol` at class definition time. An incompatible parameter type or return type (`def read(self, key: int) -> str:` vs. `def read(self, key: str) -> bytes:`) produces **zero errors** unless and until an instance is passed to a `ProtocolName`-annotated variable or parameter. If the class is instantiated via dependency injection, untyped fixtures, or tested directly by its concrete type, the mismatch goes undetected. With explicit inheritance, both `mypy` (`[override]`) and `pyright` (`reportIncompatibleMethodOverride`) flag the mismatch on the `def` line immediately, even with zero call sites.
2. **Silent partial implementation when the `Protocol` evolves (`[abstract]` / `reportAbstractUsage`):**
   Even when `Protocol` methods are written as plain stubs (`def write(self, key: str, data: bytes) -> None: ...`) without `@abstractmethod`, both `mypy` and `pyright` treat every unimplemented method and attribute of a `Protocol` as abstract on an explicit subclass (`class DiskStore(StorageProtocol):`) and reject `DiskStore()` at instantiation time (`Cannot instantiate abstract class`). Without inheritance, adding a third method to a `Protocol` leaves existing uninherited implementations unchecked wherever they are instantiated without an immediate `Protocol`-typed sink.
3. **Incompatibility with PEP 698 `@typing.override` and `explicit-override`:**
   Both `mypy` (`Method "read" is marked as an override, but no base method was found with this name [misc]`) and `pyright` (`Method "read" is marked as override, but no base method of same name is present`) **reject `@override` as a type error** on a structural implementation that does not inherit from the `Protocol`. Consequently, authors cannot mark protocol implementations with `@override`, cannot enable `mypy --enable-error-code explicit-override` or `pyright` `reportImplicitOverride` to detect stale methods after a protocol method is renamed, and lose IDE go-to-base navigation.
4. **Broken runtime docstring inheritance (`inspect.getdoc`):**
   Python's `inspect.getdoc()` walks `cls.__mro__` to inherit method docstrings from base classes when an overriding method omits a docstring. Because a structural `Protocol` is absent from `__mro__`, `inspect.getdoc(Concrete.read)` returns `None`.
5. **Broken intra-suite coordination with Omni's class and signature rules:**
   Omni's own rules rely on explicit in-file base classes (`local_ancestors` and `has_nontrivial_base_class`):
   - `undocumented-nontrivial-function` exempts methods that inherit a docstring from an in-file base class (`has_inherited_method_docstring`); without `(ProtocolName)`, a nontrivial implementation method must duplicate the protocol's docstring or trigger a violation.
   - `copied-base-docstring`, `partial-override-docstring`, and `deferral-docstring` only recognize overrides of in-file base classes via `local_ancestors`.
   - Omni's signature rules (`identical-positional-types`, `concrete-collection-parameter`, `mutable-collection-parameter`, `specific-collection-parameter`, `positional-tuple-return`, `nullable-collection-return`, `concrete-collection-return`, `mutable-collection-return`) exempt methods whose signature is constrained by a base class (`has_nontrivial_base_class`). Without explicit `Protocol` inheritance, an implementation is flagged for signature choices dictated by the protocol contract.

---

## 3. Relationship to existing Omni rules

- **`fake-without-protocol`** (`src/code_lint/rules/fake_without_protocol.rs`):
  - Targets `Fake*` classes in test and `conftest.py` files (`RuleScope::TestAndConftest`, plus `test-helper-paths`) that have **no** collaborator base class (`!has_collaborator_base`, i.e., no base class other than `object`, `Generic[...]`, or `Any`).
  - Works by class naming convention (`Fake*`) even when the collaborator `Protocol` or `ABC` lives in another module and is not imported in the test file, and even when the fake defines 0 or 1 method.
- **`uninherited-protocol`** (this rule):
  - Targets any class (such as `MemoryStorage`, `DiskCache`, `NullProvider`, `_HttpRangeReader`) that implements every public method of a `Protocol` defined in the **same file** (with at least `min-methods` public methods, default `2`) without inheriting from that `Protocol`.
  - Partition: bare `Fake*` classes without a collaborator base class (`is_fake_class_name && !has_collaborator_base`) are already owned by `fake-without-protocol` and are exempted from `uninherited-protocol` so the two rules never double-report the same class header.

---

## 4. Split test

Per [rule_design_guide.md](../rule_design_guide.md) §1:

- **Why it is bad:** Every flagged class implements all public methods of an in-file `Protocol` without listing that `Protocol` as a base class, losing class-definition signature verification, abstract-member instantiation checks, `@override` support, and docstring inheritance.
- **How to fix it:** Every violation has the same fix — add the `Protocol` class to the class's base class list.
- **Conclusion:** One defect, one fix, one `ViolationTemplate` → **one rule** (`uninherited-protocol`).
