# Phase 1: Understand — Signature & Attribute Collection Type Rules

This document records **Phase 1 (Understand)** of the exploration cycle for porting the legacy linter's collection type annotation rules (`SignatureConcreteTypeRule`, `SignatureMutableTypeRule`, `SignatureSpecificTypeRule`, `AbstractCollectionSuggestionsRule`, and `AbstractMappingSuggestionsRule`) to Omni.

> **Status**: **VALIDATED** (2026-10-02). Goals G1–G5, Non-Goals NG1–NG4, Decisions D1–D5, and Open Questions Q1–Q8 accepted.

---

## 1. Context & Problem Statement

In the legacy linter ([legacy_lints.py](../../../scratch/legacy_reference/legacy_lints.py)), five rules inspect collection type annotations across function parameters, function return values, and class/instance attributes:

1. **`AbstractCollectionSuggestionsRule`** (L1344–1366, warning) & **`AbstractMappingSuggestionsRule`** (L1368–1384, warning):
   - Flag top-level `list`, `List`, `set`, `dict`, `Dict` on parameters, return values, and class/instance attributes.
2. **`SignatureConcreteTypeRule`** (L2756–2898, error):
   - Flags `list`, `dict`, `set` in parameter, return, and attribute annotations.
   - Uses local body inspection (`is_variable_mutated`, `get_required_readonly_type`) for parameters and a whole-repository pre-pass (`SignatureMutationTracker.discover_mutations`, L2414–2594) for return types and attributes to decide whether to suggest a read-only ABC (`Sequence`, `Mapping`, `AbstractSet`, `Collection`, `Iterable`) or a mutable ABC (`MutableSequence`, `MutableMapping`, `MutableSet`).
3. **`SignatureMutableTypeRule`** (L2900–3005, error):
   - Flags `MutableSequence`, `MutableMapping`, `MutableSet` in parameter, return, and attribute annotations when the value is not detected as mutated.
4. **`SignatureSpecificTypeRule`** (L3007–3046, error):
   - Flags `Sequence` and `Collection` in parameter annotations when local body inspection concludes that a more general read-only ABC (`Collection` or `Iterable`) suffices.

### 1.1 Structural Tensions & Known Pitfalls in the legacy linter's Implementation

Stepping back from the legacy linter's implementation reveals several design and technical issues that must be researched and resolved before deciding what to build in Omni:

1. **Bundled Antipatterns Across Positions**:
   - The legacy linter checks parameters (inputs), return annotations (outputs), and class/instance attributes (state) inside the same rules (`SignatureConcreteTypeRule`, `SignatureMutableTypeRule`).
   - Under [Rule Design Guide §1](../rule_design_guide.md) (*1 Rule = 1 Antipattern; The Split Test*), inputs, outputs, and state have distinct rationales, trade-offs, and ecosystem consensus (e.g., Postel's Law / Mypy guidance recommends abstract types for parameters and concrete types for return values, whereas strict encapsulation styles prefer abstract return types too).
2. **Whole-Repository Pre-Pass (`SignatureMutationTracker`) vs. Per-File Linting**:
   - The legacy linter scans every `.py` file in `src/` and `tests/` before linting to guess whether callers mutate returned values or class attributes. Omni's runner evaluates each `ParsedFile` independently and in parallel (`rayon`), and single-repo caller scanning still fails on public APIs consumed outside the module or repo.
3. **Aggressive Call-Escape Heuristics (`is_variable_escaping`)**:
   - In the legacy linter, passing a parameter `x` to any function outside a 14-item builtin allowlist causes `is_variable_escaping` to return `True`, which `is_variable_mutated` treats as a mutation—causing `SignatureConcreteTypeRule` to falsely claim *"Argument is mutated ... Use `MutableSequence`"* whenever a read-only parameter is forwarded to a helper function.
4. **Incomplete Annotation AST Traversal (`get_annotation_base_types`)**:
   - The legacy linter's `get_annotation_base_types` recurses into `Subscript.value` and `|` (`BitOr`), but never into `Subscript.slice`. As a result, `list[int] | None` is flagged while `Optional[list[int]]`, `Union[list[int], None]`, and `Mapping[str, list[int]]` are silently ignored. Conversely, naive full-tree traversal would walk into contravariant `Callable[[list[int]], None]` parameter lists or non-type metadata in `Annotated[T, ...]`.
5. **Generator / Iterator Pitfalls in `SignatureSpecificTypeRule` (`Sequence` → `Iterable`)**:
   - Narrowing `Sequence[T]` or `Collection[T]` to `Iterable[T]` permits single-pass iterators/generators (`Iterator[T]`). The legacy linter's `get_required_readonly_type` does not check for truthiness tests (`if items:`, `if not items:`, `bool(items)`—which always evaluate to `True` on a generator!), multiple iterations (`for x in items:` twice, or iterating inside a loop), `reversed(items)` (which requires `Reversible` / `Sequence`, not `Collection` or `Iterable`), `.index()` / `.count()`, or stub/protocol bodies (`...`, `pass`, `raise NotImplementedError`).

---

## 2. Goals & Explicit Non-Goals

### Goals

- **G1 — SOTA-Grounded Rule Decomposition**:
  - *Reason*: Evaluate all five candidate checks (concrete parameter types, unused mutable parameter types, overly specific read-only parameter types, concrete return types, and concrete attribute types) against Python typing specifications, style guides, and SOTA linters so each rule we adopt targets one well-defined antipattern ([Rule Design Guide §1](../rule_design_guide.md)).
- **G2 — Explicit Test-Case Matrix & Prototype-Driven Decisions**:
  - *Reason*: Define concrete code cases and ideal outcomes (true positives, true negatives, edge cases, variance positions, body-usage patterns) and test them with prototypes before deciding which rules to ship, how deep annotation matching should go, and which heuristics are reliable enough.
- **G3 — High Signal-to-Noise Ratio (Eliminate the legacy linter's False Positives)**:
  - *Reason*: A linter rule that suggests breaking changes (e.g., `Iterable` on a truthiness-checked or twice-iterated parameter, `Callable` contravariance inversion, or `MutableSequence` on a forwarded read-only parameter) erodes trust and forces mass suppressions.
- **G4 — Pure Single-File Execution within Omni's Architectural DAG**:
  - *Reason*: All AST extraction and body/annotation analysis must live in `code_lint::ast` (or `code_lint::semantic`), keeping rules in `code_lint::rules` as declarative policy over opaque `AstNode` handles ([Rule Design Guide §7](../rule_design_guide.md)) with fast per-file execution.
- **G5 — Orthogonal, Actionable Diagnostics & Extensible Options**:
  - *Reason*: Every adopted rule must provide orthogonal `summary` / `rationale` / `suggestion` templates ([Rule Design Guide §2](../rule_design_guide.md), [Naming and Message Style Guide](../naming_and_message_style_guide.md)) and configurable type lists where appropriate ([Rule Design Guide §5](../rule_design_guide.md)).

### Explicit Non-Goals

- **NG1 — Rust Language Support for This Rule Family**:
  - *Reason*: Validated in D3. Rust's default `clippy::ptr_arg` (`&Vec<T>`, `&String`, `&PathBuf`, `&Cow<T>`) and `clippy::needless_pass_by_value` already enforce the Rust equivalents with full compiler type and borrow-checker information.
- **NG2 — Cross-File / Whole-Repository Mutation Tracking (`SignatureMutationTracker`)**:
  - *Reason*: Cross-file pre-passes break Omni's per-file parallel runner model and remain unsound for public interfaces whose callers live outside the analyzed files.
- **NG3 — Full Static Type Inference**:
  - *Reason*: Omni operates on Tree-sitter CSTs (`ast-grep`) without a type-checker symbol solver; rules must rely on syntactic annotations and local scope AST structure.
- **NG4 — Automatic Code Fixes (`--fix`)**:
  - *Reason*: Replacing a collection annotation often requires adding an import from `collections.abc` and choosing between `Sequence`, `Iterable`, or `MutableSequence`; autofix is tracked separately on [ROADMAP.md](../../../ROADMAP.md).

---

## 3. Numbered Decisions (D1–D5)

- **D1 — Exploration-First Workflow**: Follow the 7-phase process sequentially (`01_understand.md` → `02_sota_and_references.md` → `03_design_plan.md` → ...), getting user validation at the end of each phase before moving to the next. Do not lock in rule boundaries, annotation traversal depth, or heuristic inclusion until Phase 2 (SOTA research) and Phase 3 (ideal test-case matrix + prototypes) provide the evidence.
- **D2 — Work Directory**: Store all phase documents and exploration artifacts in `docs/dev/signature_collection_types/`.
- **D3 — Python-Only Target (`SupportLang::Python`, `RuleTarget::SourceOnly`)**: Target Python source files only (excluding test files, where pytest fixtures and parameterized inputs routinely use concrete types); leave Rust to `clippy::ptr_arg` (NG1).
- **D4 — Investigate All 5 Candidate Areas Before Selecting the Implementation Set**:
  1. Concrete collection types in function/method parameters (`list`, `dict`, `set`, `typing.List`, `Dict`, `Set`)
  2. Unused mutable abstract collection types in parameters (`MutableSequence`, `MutableMapping`, `MutableSet`)
  3. Overly specific read-only abstract collection types in parameters (`Sequence`, `Collection` → `Collection`, `Iterable`)
  4. Concrete/mutable collection types in return annotations
  5. Concrete/mutable collection types in class and instance attribute annotations
  Any candidate found in Phases 2–3 to have an unfavorable complexity-to-signal ratio will be documented and deferred to `ROADMAP.md`.
- **D5 — Pure Signature Check for Concrete Parameter Types**: When flagging concrete `list` / `dict` / `set` in parameter annotations, do not run the legacy linter's body mutation/escape heuristic to guess whether the suggestion should be `Sequence` vs. `MutableSequence`; the rule is an exact signature check (`Precision::Exact`) whose suggestion covers both read-only (`Sequence`, `Mapping`, `AbstractSet`) and in-place mutating (`MutableSequence`, `MutableMapping`, `MutableSet`) replacements.

---

## 4. Open Questions for Phase 2 (SOTA Research) & Phase 3 (Test Matrix & Prototypes)

- **Q1 (SOTA & Ecosystem Standards Across Positions)**:
  - How do Python typing specifications (PEP 484, PEP 585), major style guides (Google Python Style Guide, Mypy docs, Effective Python), Python linters (`flake8-kotoha`, `flake8-pyi`, Ruff, Pylint), and other ecosystems (TypeScript `@typescript-eslint/prefer-readonly-parameter-types`, Java ErrorProne `MixedMutabilityReturnType`, Rust Clippy `ptr_arg`) treat **parameters** vs. **return types** vs. **class/instance attributes**?
- **Q2 (Annotation AST Traversal Depth & Variance Positions)**:
  - What are all the syntactic positions inside a Python type annotation (`list[int]`, `list[int] | None`, `Optional[list[int]]`, `Union[...]`, `Annotated[T, ...]`, `Final[T]`, `ClassVar[T]`, nested covariant type args like `Mapping[str, list[int]]` or `Awaitable[list[int]]`, contravariant `Callable[[list[int]], R]`, `Literal[...]`, `TypeAlias`, type bounds)?
  - For each candidate rule, what is the ideal outcome on each annotation shape, and what traversal algorithm best captures it?
- **Q3 (Feasibility & Accuracy of Body-Usage Analysis for Rule 2: `MutableSequence` / `MutableMapping` / `MutableSet`)**:
  - How can a single-file AST pass reliably detect whether a parameter is mutated or escapes in Python (method calls, subscript writes/deletes, augmented assignments, unpacking writes, call argument forwarding, attribute/variable aliasing, returning/yielding, closures, comprehensions)?
  - Should method calls on the parameter use a denylist of mutating methods (like the legacy linter) or an allowlist of known read-only `Sequence` / `Mapping` / `AbstractSet` methods?
- **Q4 (Feasibility & Accuracy of Capability Analysis for Rule 3: `Sequence` / `Collection` → `Iterable`)**:
  - What is the complete capability lattice separating `Iterable`, `Collection` (`Sized + Iterable + Container`), `Reversible`, and `Sequence` in Python?
  - Can single-file AST analysis accurately detect multi-pass iteration (multiple loops, loops inside loops/comprehensions/closures), truthiness checks (`if x:`, `not x`, `bool(x)`, `x and y`, `while x:`), `reversed(x)`, `.index()`, `.count()`, and sequence pattern matching (`match x: case [...]`) without excessive complexity or false positives?
- **Q5 (Class & Instance Attributes: Public vs. Private, Dataclasses, `ClassVar`)**:
  - For attribute annotations (`class C: x: list[int]` and `self.x: list[int]`), how do public vs. private (`_x`) attributes, `@dataclass` / `@dataclass(frozen=True)`, `NamedTuple`, `TypedDict`, Pydantic `BaseModel`, and `ClassVar` affect whether a concrete collection type is an antipattern or required?
- **Q6 (Function & Class Exemptions)**:
  - Which definitions must be exempted across the rules because their signature or body is constrained externally or is a stub (`@override`, `@overload`, `@abstractmethod`, `@fixture`, `Protocol`, `ABC`, stub bodies `...` / `pass` / `raise NotImplementedError`, dunder methods, runtime-inspected framework decorators)?
- **Q7 (Granularity, Rule Naming, Spans & Templates)**:
  - When multiple parameters or nested type nodes in a single function/annotation match, should diagnostics be emitted per offending type node, per parameter, or per function signature (and how does that interact with `rule_test!` single-diagnostic fail cases and [Rule Design Guide §6](../rule_design_guide.md))?
  - What are the exact rule names, `Classification` facets (`Topic`, `Precision`, `Consensus`, `ImpactedQuality`), `ViolationTemplate` wording, and `RuleOptions` for the selected rules?
- **Q8 (Shared AST Infrastructure for Connexe the legacy linter Rules)**:
  - How can the AST extractors designed here also support the remaining the legacy linter annotation/class rules (`InstanceAttributeAnnotationRule`, `AvoidOptionalOrNoneRule`, `FakeMustInheritProtocolRule`, `RequireExplicitProtocolInheritanceRule`, `BannedTypeAnnotationsRule`)?
