# Phase 7: Learn — Durable Lessons from `banned_type_annotations`

## 1. Decompose Hardcoded Domain Rules into Structural Arity Rules + Configurable Pattern Rules

When migrating or evaluating a legacy rule that hardcodes domain-specific shapes (such as `BannedTypeAnnotationsRule` matching five project-specific tuple signatures plus `Coroutine[object, object, ...]`), resist the binary choice between porting project-specific names and dropping the rule entirely:

1. **Generalize the structural invariant across all AST sites**: If `positional-tuple-return` catches unnamed multi-element tuples in return position, the same structural smell exists at parameter (`positional-tuple-parameter`) and attribute/field (`positional-tuple-attribute`) boundaries. Extending the structural check to parameters and attributes catches every ad-hoc multi-element tuple without configuring a single type name.
2. **Provide a zero-default structural pattern matcher for project-specific shapes**: A single configurable rule (`adhoc-type-annotation`, `banned: ListOption` with `base: &[]`) lets downstream repositories ban domain-specific composite types (`tuple[Width, Height] => Dimensions`) or verbose generic signatures (`Generator[_, None, None] => collections.abc.Iterator`) while matching over normalized AST `TypeShape` rather than fragile regexes.

---

## 2. Normalize Wrapper Nodes Before Emitting AST Candidates

When a rule inspects both top-level type annotations and nested type arguments (`list[tuple[Width, Height]]`, `Annotated[tuple[Width, Height], "px"]`, `(&mut (Width, Height))`):

- Transparent AST containers that share the same normalized `TypeShape` as their inner expression (`Annotated[T, ...]` in Python, `ast::Type::ParenType` in Rust, and nested union nodes `Optional[A | B]`) **must be unwrapped before emitting a `TypeAnnotationCandidate`**, and union recursion must walk the flattened leaf branches rather than intermediate binary `|` nodes.
- Otherwise, both the wrapper node and its inner payload produce separate candidates with identical `TypeShape`, causing duplicate diagnostics or forcing rules to deduplicate spans downstream.

---

## 3. Canonicalize Standard-Library Type Aliases Without Collapsing Distinct Modules

When matching user-configured type patterns against import-resolved `WrittenSymbol`s:

- **Alias equivalence (`list` $\leftrightarrow$ `typing.List`) requires explicit canonicalization**, not stripping module prefixes whenever a terminal name appears in a flat `STD_TYPE_CONSTRUCTORS` set.
- Stripping prefixes blindly causes cross-namespace collisions when two distinct standard-library modules export the same terminal name (`typing.Set` / `builtins.set` vs. `collections.abc.Set`, or `std::rc::Weak` vs. `std::sync::Weak`), and falsely matches a qualified pattern (`std::sync::Mutex<T>`) against an unimported bare identifier (`Mutex<T>`).
- Map known `typing` aliases (`typing.List` $\to$ `"list"`, `typing.Tuple` $\to$ `"tuple"`, `typing.Set` $\to$ `"set"`, `typing.FrozenSet` $\to$ `"frozenset"`, `typing.Dict` $\to$ `"dict"`, `typing.Type` $\to$ `"type"`) to a single canonical name while preserving the full module path for non-alias qualified names.

---

## 4. Keep General Type-Annotation Helpers in `ast/<lang>/annotations.rs` and Free of Rule Thresholds ([ADR 012](../decisions/012_module_placement_by_abstraction.md))

When an existing narrow module (`ast/rust/returns.rs` or `ast/python/returns.rs`) already contains helpers for unwrapping `Option<T>`, pointer wrappers (`&T`, `Box<T>`, `Arc<T>`), or tuple arity, and new parameter/attribute extractors need the same facts:

- Move the general type-annotation helpers into `ast/<lang>/annotations.rs` so `returns.rs` imports from `annotations.rs`, never the reverse. A helper whose contract is stated in terms of general type annotations (`ast::Type` or `Expr`) belongs in `annotations.rs` even if `returns.rs` was its first caller.
- Keep threshold policies out of `ast` extractors ([ADR 012](../decisions/012_module_placement_by_abstraction.md) P6): when `positional_tuple_arity` hardcoded `arity >= 2` inside `ast`, it shadowed the rule's own `arity >= min_elements` (`default: 2`) guard and produced a surviving mutant under `cargo mutants`. Reporting the general syntactic fact from `ast` (any non-unit fixed-length tuple, `arity >= 1`) and enforcing `arity >= min_elements` in the rules eliminated the duplicated threshold and made the rule guard 100% mutation-tested.
