# Phase 7: Learn — Declaration Ordering (`Topic::DECLARATION_ORDER`)

> [!IMPORTANT]
> **Status**: **COMPLETED**

---

## Reusable Engineering Lessons

1. **Decompose Call-Graph Ordering into Orthogonal, Priority-Partitioned Rules (`P1 -> P2 -> P3`)**:
   - A monolithic "stepdown rule" or bottom-up `call-before-definition` rule fails on real codebases because visibility tiers, component colocation, and caller-callee direction collide whenever a file has multiple public entrypoints sharing private helpers (`|Roots(h)| >= 2`).
   - Partitioning private callables by their public entrypoint reachability `Roots(h)` (stopping at public boundaries) and evaluating three orthogonal rules in strict priority order — `uncolocated-helper` (P1, exclusive helpers `|Roots(h)| == 1` contiguous with their owner), `private-before-public-function` (P2, private helpers after their public roots `max(Roots(h))`), and `callee-before-caller` (P3, top-down caller-before-callee within the same tier and group, outside Tarjan SCC cycles) — guarantees zero contradictory advice and at most one diagnostic per misplaced function ([ADR 011](../../../decisions/011_colocated_abstraction_ordering.md)).

2. **Bridge Module Call Graphs Through Visitor / Helper Struct `impl` Blocks While Respecting Method-Local Bindings**:
   - In both Python and Rust, module-level public entrypoints frequently instantiate a module-private visitor or state struct (`LiteralOccurrenceCollector`, `ParameterUseVisitor`, `BindingVisitor`) whose methods call module-level private helpers. Bridging call edges through module-local classes and `impl` blocks accurately attributes those helpers to the owning public entrypoint.
   - When collecting module-level function references inside a Rust `impl` block (`collect_rust_impl_callees` in [src/code_lint/ast/rust.rs](../../../src/code_lint/ast/rust.rs)), local bindings (`collect_rust_fn_local_names`) must be collected per `ast::AssocItem::Fn` method so that method parameters and closure variables (`|collection_type|`) are never mistaken for calls to same-named module functions.

3. **2-Tier Visibility (`Exported` vs. `Private`) Preserves Restricted-Constructor Encapsulation in Rust**:
   - In Rust inherent `impl` blocks, types are frequently constructed via subsystem-scoped constructors (`pub(crate) fn new`, `pub(in crate::...) fn from_span`, `pub(super) fn new_from_str`) while exposing `pub fn` accessors.
   - A 3-tier visibility rule (`pub -> pub(...) -> fn`) would force `pub(crate) fn new` below `pub fn` getters, directly contradicting `constructor-after-method`. Partitioning callables into 2 tiers (`Exported`: any `visibility().is_some()`, plus `fn main`; vs. `Private`: bare `fn`) aligns visibility ordering with constructor-first lifecycle ordering.

4. **Rust `ra_ap_syntax` Outer Doc Comments Precede Outer Attributes Inside `SyntaxNode::text_range()`**:
   - Unlike `tree-sitter-rust` (where outer attributes and doc comments are preceding siblings of an item), `ra_ap_syntax` attaches both `///` doc comments and `#[...]` outer attributes as leading children inside the item's `SyntaxNode`.
   - Any range collector over `ra_ap_syntax` nodes (such as `collect_inline_test_ranges_rec` in [src/code_lint/ast/rust.rs](../../../src/code_lint/ast/rust.rs)) must start at `node.text_range().start()` rather than the first `ast::Attr`'s start offset so that `item.syntax().text_range().start()` on documented `#[cfg(test)]` items is covered by the range.

5. **Segment Module Call Scopes on Redefined Function Names**:
   - Both `rule_test!`'s doubled-snippet check (`format!("{code}\n{second_copy}")`) and conditional platform definitions in real code can define the same function name twice in a single module body. Starting a new `CallableScope` segment whenever a non-overload, non-property function name is redefined keeps each segment's name-to-index map 1:1 and ensures `rule_test!` reports exact diagnostics in both copies.
