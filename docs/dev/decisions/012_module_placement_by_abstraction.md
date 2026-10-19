# ADR 012: Module Placement by Abstraction

## 1. Problem Statement

[ADR 006](006_architectural_dag_and_conformance.md) fixes the direction of dependencies between components, and [ADR 011](011_colocated_abstraction_ordering.md) fixes the order of items inside a scope. Neither says **which module an item belongs to**. Without that rule, placement defaults to its consumers:

1. **"Shared" as a placement criterion**: an item used by two sibling modules moved up to their parent (`ast/python.rs`, `ast/rust.rs`). The parents became grab-bags of unrelated helpers: node lookup, name resolution, decorators, mutation detection, comment collection, statement spans.
2. **Relays**: children then re-imported those helpers, and each other's, through the parent (`use self::functions::…` in `python.rs`, read back as `use super::…`). The real owner of an item was invisible at its use site.
3. **"Single consumer" as a placement criterion**: the opposite mistake moves an item into its only caller, even when the item is a concept of its own that happens to have one caller today.
4. **Hidden cycles**: `classes` used `functions` (methods, receivers) and `functions` used `classes` (the enclosing class's kind). Each edge was locally reasonable; nothing named the conflation behind them.

## 2. Decision

**An item belongs to the abstraction whose vocabulary states its contract. Who calls it, and what it is built on, do not decide placement.**

To place an item, state its contract in one sentence (inputs, output, meaning). The abstraction whose nouns that sentence uses owns the item.

- **P1. A module is one abstraction**: a concept with a contract stated in its own vocabulary. The number and size of modules follow from the abstractions, never the reverse.
- **P2. Placement by contract vocabulary**: callers are irrelevant (shared or single-use). Implementation dependencies are irrelevant: a function stated in terms of classes that uses name resolution belongs to classes, and name resolution is its dependency.
- **P3. A cycle reveals conflated abstractions**: dependencies point from an abstraction to the abstractions it is defined in terms of. When two modules depend on each other, at least one holds two abstractions; split it until the graph is acyclic.
- **P4. Parent modules route**: a module whose children are the abstractions (`mod` + `pub use`) is not itself an abstraction and holds no items.
- **P5. Names follow abstractions**: the same abstraction has the same name in every language. Distinct abstractions keep their own names even when they play the same role.

### Worked examples (`code_lint::ast`)

| Item | Contract | Owner | Not |
|---|---|---|---|
| `is_protocol_or_abc_class` | the `class` statement inherits `Protocol`/`ABC` or declares `metaclass=ABCMeta` | class declarations | `names` (a dependency); `functions` (a caller) |
| `direct_function_definitions` | the `FunctionDef` statements that are immediate children of a body | syntax-tree navigation | `scopes`, `classes`, `functions` (callers) |
| `has_override_decorator` | the function overrides a base method | functions | decorators (the mechanism) |
| `fstring_segments_and_interpolations` | an f-string split into literal text and replacement fields | format strings | string literals in general |
| `attribute_terminal_name` (Rust) | the terminal name of an attribute's path | Rust attributes | `names` (a dependency) |

`classes` ↔ `functions` (P3): the enclosing class a function needs is the class **declaration** (name, bases, kind, decorators, docstring). The members a class needs are functions. So the class declaration and the class model are two abstractions: `functions → class declarations`, and `class model → functions, class declarations`.

Python decorators and Rust attributes (P5) both mark definitions, but a decorator is an expression applied to the definition (resolved through imports, possibly called with arguments), while an attribute is declarative metadata (outer or inner, never applied). They are different abstractions, so `decorators` and `attributes` keep their language's name.

## 3. Relation to ADR 006 and ADR 011

- ADR 006's "shared logic moves down to a lower shared layer" says *when* to extract and *which direction* dependencies point. This ADR says *where* the extracted item lands: in the abstraction that owns its vocabulary, which is lower than its consumers because they are stated in terms of it.
- ADR 011 orders items within the module this ADR selects.

## 4. Consequences

- Each module's doc comment states its abstraction. An item whose contract does not fit that statement is misplaced.
- Imports name the owning module (`use super::functions::…`), never a relay.
- Moving an item is justified by its contract, not by a change in its callers. Adding or removing a caller never moves an item.
- Enforcement of P3/P4 inside a component is a roadmap item; today they are review criteria.
