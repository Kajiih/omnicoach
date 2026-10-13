# Explicit Protocol Inheritance: 02 References

> [!NOTE]
> **Status: VALIDATED** (user, 2026-10-14; implemented). Open questions resolved with the recommended answers.

## 1. Python Standards and Type System Specifications

### 1.1 PEP 544 — Protocols: Structural subtyping (static duck typing)

- **URL:** <https://peps.python.org/pep-0544/>
- **Section: "Explicitly Declaring Implementation"** (<https://peps.python.org/pep-0544/#explicitly-declaring-implementation>):
  - PEP 544 defines `typing.Protocol` so that a class implementing the protocol's members is a structural subtype by default (necessary when the class is defined in an external library, predates the protocol, or avoids cyclic imports).
  - At the same time, PEP 544 explicitly defines **nominal protocol subclassing** (`class Concrete(MyProtocol):`) and states three reasons to inherit explicitly from a `Protocol` when the author intends to implement it:
    1. **Self-documenting intent:** readers and tools can see directly in the class header which protocol contract the class fulfills.
    2. **Static verification of completeness and signature compatibility:** a type checker treats unimplemented protocol attributes and methods as abstract on an explicit subclass and verifies method override signatures at the class definition site rather than waiting for a call site.
    3. **Inheritance of default methods and metadata:** explicit subclasses inherit concrete default methods defined on the protocol as well as method docstrings via `__mro__`.
- **Section: "Merging and extending protocols"** (<https://peps.python.org/pep-0544/#merging-and-extending-protocols>):
  - A subclass of a `Protocol` is **only** itself a `Protocol` if `typing.Protocol` (or `typing_extensions.Protocol`) is listed among its **direct** base classes (`class SubProto(BaseProto, Protocol):`).
  - If `Protocol` is not in the direct bases (`class Concrete(BaseProto):`), the class is a regular concrete class that nominally implements `BaseProto`.

### 1.2 Python Typing Specification — Protocols & PEP 698 (`@typing.override`)

- **URLs:**
  - <https://typing.python.org/en/latest/spec/protocol.html>
  - <https://peps.python.org/pep-0698/>
- **Empirical verification (`mypy 1.19` and `pyright 1.1.408`):**

| Check | Structural implementation (`class Impl:`) | Explicit inheritance (`class Impl(StorageProtocol):`) |
| :--- | :--- | :--- |
| **Incompatible parameter or return type on method** (with no typed call site) | **Silent** (0 errors in `mypy` and `pyright`). | **Immediate error at `def`:** `mypy` reports `[override]` (`Return type "str" of "read" incompatible with return type "bytes" in supertype "StorageProtocol"`); `pyright` reports `reportIncompatibleMethodOverride`. |
| **Missing protocol method or attribute at instantiation `Impl()`** (even when protocol methods are plain `def m(self): ...` without `@abstractmethod`) | **Silent** at `Impl()` (0 errors in `mypy` and `pyright`). | **Immediate error at `Impl()`:** `mypy` reports `[abstract]` (`Cannot instantiate abstract class "Impl" with abstract attributes "name" and "write"`); `pyright` reports `reportAbstractUsage`. |
| **`@typing.override` on protocol method** (PEP 698) | **Rejected as error:** `mypy` reports `Method "read" is marked as an override, but no base method was found with this name [misc]`; `pyright` reports `reportGeneralTypeIssues`. | **Accepted and verified:** if the protocol method is later renamed or removed, `@override` immediately flags the orphaned method on `Impl`. |
| **Missing `@override` enforcement** (`mypy --enable-error-code explicit-override` / `pyright reportImplicitOverride`) | **Silent** (type checker does not know `read` overrides `StorageProtocol.read`). | **Enforced:** flags every protocol method on `Impl` that omits `@override`. |
| **Runtime `inspect.getdoc(Impl.read)`** (Python 3.12+) | **`None`** (`StorageProtocol` is not in `Impl.__mro__`). | **Returns `StorageProtocol.read.__doc__`** when `Impl.read` has no docstring. |

---

## 2. Python Runtime Metaclass Constraints

Verified on CPython 3.14: `typing.Protocol` uses `typing._ProtocolMeta` (a subclass of `abc.ABCMeta`). Combining a `Protocol` base class (in either base order) with standard library special class families that define their own metaclasses disjoint from `ABCMeta` fails at class creation time:

| Candidate class family | Metaclass | Runtime result of `class C(Base, MyProtocol):` or `class C(MyProtocol, Base):` |
| :--- | :--- | :--- |
| `enum.Enum`, `IntEnum`, `StrEnum`, `Flag`, `IntFlag`, `ReprEnum` | `enum.EnumType` | `TypeError: metaclass conflict: the metaclass of a derived class must be a (non-strict) subclass of the metaclasses of all its bases` |
| `typing.TypedDict` | `typing._TypedDictMeta` | `TypeError: metaclass conflict` (and `TypedDict` forbids non-`TypedDict` bases) |
| `typing.NamedTuple` | `typing.NamedTupleMeta` | `TypeError: metaclass conflict` (and `NamedTuple` forbids non-`Generic` bases) |
| `BaseProtocol` + `SubProtocol(BaseProtocol, Protocol)` in `[BaseProtocol, SubProtocol]` order | `typing._ProtocolMeta` | `TypeError: Cannot create a consistent method resolution order (MRO) for bases BaseProtocol, SubProtocol` |

---

## 3. Prior Art in the Linter Ecosystem

| Tool | Rule / Feature | Coverage of uninherited `Protocol` implementations |
| :--- | :--- | :--- |
| **Ruff** | None | No rule in `flake8-pyi` (`PYI`), `pylint` (`PL`), `flake8-bugbear` (`B`), or `ruff` (`RUF`) inspects whether a concrete class implements a `Protocol` without inheriting from it. |
| **Pylint** | `W0223` (`abstract-method`), `W0221` (`arguments-differ`) | Only inspects classes that **already** nominally inherit from a base class; never detects missing `Protocol` inheritance. |
| **Mypy / Pyright** | Structural protocol checking | Checks structural compatibility only at call/assignment sites typed with the `Protocol`; never warns that an in-file or first-party implementation omitted the `Protocol` from its base classes. |
| **Legacy linter** | `RequireExplicitProtocolInheritanceRule` | Repo-wide AST check matching classes that define all members of a `>= 2`-member `Protocol`; suffers from the `AnnAssign`/`FunctionDef` asymmetry bug (P1), `@property` conflation (P2), global bare-name collisions (P3), unresolved `Protocol` base names (P4), sub-protocol cascade (P5), and missing `ABC`/`Enum`/`TypedDict`/`NamedTuple` exemptions (P6). |
| **Omni** | `fake-without-protocol` | Flags `Fake*` classes in test and `conftest.py` files that have no collaborator base class (`Protocol` or `ABC`). Does not cover non-`Fake*` classes (`MemoryStore`, `NullProvider`, `DiskCache`, `StubClient`) or `Fake*` classes that inherit from a generic helper base class (`class FakeStore(BaseFake):`) rather than the in-file `Protocol`. |

---

## 4. Empirical Corpus Analysis

We scanned **13,198 unique Python files** across the Python 3.14 standard library (`/usr/lib/python3.14`), Debian/Google system packages (`/usr/lib/python3`), and cached PyPI wheels (`~/.cache/uv`, including `pytest`, `mypy`, `Pillow`, `pre-commit`, `virtualenv`/`python_discovery`, `imagesize`, `setuptools`/`pkg_resources`, `rich`, `httpx`, `pydantic`).

### 4.1 Protocol distribution

| Metric | Count | Share of `Protocol` classes |
| :--- | ---: | ---: |
| Total unique `.py` files scanned | 13,198 | — |
| Files containing at least one `Protocol` class | 82 | — |
| Total `Protocol` classes | 122 | 100.0% |
| `Protocol` classes with `0` public non-property methods | 68 | 55.7% (46 are dunder-only: `__call__`, `__lt__`, `__iter__`, `__enter__`/`__exit__`) |
| `Protocol` classes with `1` public non-property method | 30 | 24.6% |
| `Protocol` classes with `>= 2` public non-property methods | 24 | 19.7% (`{2: 6, 3: 1, 4: 1, 5: 4, 6: 5, 7: 2, 9: 1, 12: 2, 22: 1, 29: 1}`) |
| `Protocol` classes declaring `@property` getters | 11 | 9.0% |
| `Protocol` classes declaring public `AnnAssign` attributes | 9 | 7.4% |

### 4.2 Uninherited same-file matches by `min-methods` threshold

| Threshold | Unique same-file uninherited matches | Precision assessment |
| :--- | ---: | :--- |
| `min-methods = 1` | 10 (`min == 1`) + 6 (`min >= 2`) | **Noisy on `min == 1`:** 5 of the 6 single-method matches are coincidental name collisions on 1-method protocols (`MappingKey(NamedTuple)`, `MappingValue(NamedTuple)`, and `SequenceItem(NamedTuple)` matching `_Matcher.match` in `pre_commit/yaml_rewrite.py`; `Image` matching `SupportsGetData.getdata` in `PIL/Image.py`). Only 1 (`DummyRewriteHook` matching `RewriteHook.mark_rewrite` in `_pytest/assertion/__init__.py`) is an intended implementation. |
| `min-methods = 2` (default) | **5 concrete classes** (6 raw protocol pairs before sub-protocol deduplication) | **100% true-positive rate:** every matched class in the 13,198-file corpus is a deliberate in-file implementation of the matched `Protocol`. |

### 4.3 Every uninherited same-file match at `min-methods >= 2`

1. **`python_discovery/_cache.py:65` — `class DiskContentStore:` vs. `class ContentStore(Protocol):` (line 19)**
   - Shared public methods (5): `exists`, `locked`, `read`, `remove`, `write`.
   - True positive: `DiskContentStore` is the concrete on-disk implementation of `ContentStore` defined 46 lines above it in the same file.
2. **`python_discovery/_cache.py:115` — `class DiskCache:` vs. `class PyInfoCache(Protocol):` (line 49)**
   - Shared public methods (2): `py_info`, `py_info_clear`.
   - True positive: `DiskCache` is the concrete implementation of `PyInfoCache` in the same module.
3. **`imagesize/imagesize.py:93` & `:570` — `class _HttpRangeReader:` and `class _ExtentReader:` vs. `class ReadSeekBinary(Protocol):` (line 61)**
   - Shared public methods (2): `read`, `seek`.
   - True positives: `ReadSeekBinary` is a custom `Protocol` defined at line 61 of `imagesize.py` specifically for binary readers in that file, and `_HttpRangeReader` and `_ExtentReader` are the two custom reader classes in the same file created to satisfy it.
4. **`pkg_resources/__init__.py:1654` — `class NullProvider:` vs. `class IMetadataProvider(Protocol):` (line 554, 6 methods) and `class IResourceProvider(IMetadataProvider, Protocol):` (line 583, 12 methods)**
   - Shared public methods (12): `get_metadata`, `get_metadata_lines`, `get_resource_filename`, `get_resource_stream`, `get_resource_string`, `has_metadata`, `has_resource`, `metadata_isdir`, `metadata_listdir`, `resource_isdir`, `resource_listdir`, `run_script`.
   - True positive: `NullProvider` is the root concrete implementation of `IResourceProvider` (and its base protocol `IMetadataProvider`) in the same file.
   - **Validates two critical design rules:**
     - **Direct-method matching on candidates (avoids subclass cascade):** `pkg_resources/__init__.py` defines 8 concrete subclasses of `NullProvider` (`EggProvider`, `DefaultProvider`, `EmptyProvider`, `ZipProvider`, `FileMetadata`, `PathMetadata`, `EggMetadata`, `ElfZipProvider`) that inherit `NullProvider`'s methods and override only 1–3 methods each. Inspecting only direct methods on the candidate class flags `NullProvider` once and avoids cascading onto its 8 subclasses (which automatically inherit `IResourceProvider` once `NullProvider(IResourceProvider)` is fixed).
     - **Most-derived protocol filtering (avoids sub-protocol cascade):** `IResourceProvider(IMetadataProvider, Protocol)` extends `IMetadataProvider`. Suppressing any matched protocol that is an in-file ancestor of another matched protocol on the same candidate class reports only `IResourceProvider` on `NullProvider` instead of emitting two redundant diagnostics.
