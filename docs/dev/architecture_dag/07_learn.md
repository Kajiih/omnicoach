# Phase 7: Learn — Key Lessons & Reusable Patterns

This document records **Phase 7 (Learn)** of the 7-phase workflow for `omni`'s architectural DAG specification, compile-time component colocation, and structural CST conformance engine.

**Status**: Validated by User.

---

## 1. Architectural & Rust Design Lessons

1. **Compile-Time Macro Validation Beats Text-Only Tagging**:
   - Expanding `architecture_component!(Variant)` to a typed module-scoped constant (`const _ARCHITECTURE_COMPONENT: $crate::architecture::ArchitectureComponent = $crate::architecture::ArchitectureComponent::$component;`) gives immediate `rustc` validation during `cargo check` / `cargo build`:
     - Typos in component names fail with a standard `rustc` enum variant diagnostic at the exact call site.
     - Duplicate `architecture_component!` declarations in the same file fail with `E0428: the name _ARCHITECTURE_COMPONENT is defined multiple times`.
     - Placing `architecture_component!(...)` inside a binary crate (`src/bin/*.rs`) works identically via `omni::architecture_component!(ApplicationBinaries);` without copy-pasted no-op macro stubs.
2. **Single-Pass Declarative Macro Generation Eliminates Bijection Drift**:
   - Having `define_architecture!` generate `pub enum $enum_name` and `pub const $graph_const` from a single token stream guarantees a 1-to-1 correspondence between enum variants and DAG nodes by construction (`rustc` rejects duplicate enum variants if a node is listed twice), while delegating array construction to `architecture_graph!` keeps the graph builder reusable for lightweight unit tests (`architecture_graph! { "alpha" => ["beta"], "beta" => ["alpha"] }`).
3. **Dogfooding the CST Layer Eliminates Line-Based String Smells**:
   - Line-based heuristics (`lines().take_while(|line| !line.contains("#[cfg(test)]"))`, `trim().starts_with("use super::")`, `contains("architecture_component!(")`) inevitably fail on multi-line formatting, raw string fixtures in tests, and mid-file `#[cfg(test)]` helper items.
   - Dogfooding `ParsedFile::rust` and `code_lint::ast::rust` inside `tests/architecture.rs` while keeping raw tree-sitter node kinds (`use_declaration`, `macro_invocation`, `attribute_item`) encapsulated inside `src/code_lint/ast/rust.rs` (`AST_GREP_OWNERS`) made all 4 structural checks in `tests/architecture.rs` exact, concise, and immune to string/comment false positives.

---

## 2. Process & Workflow Lessons

1. **Apply a Strict Anti-Speculation Filter During Phase 6 (Review & Audit)**:
   - Adversarial/pedantic reviewers naturally surface edge cases for hypothetical syntax or impossible states (e.g., inline `super::foo::bar()` expression calls that nobody writes, braced/commented `architecture_component! { /* ... */ }` invocations, or runtime tests checking invariants already guaranteed by a macro's expansion).
   - Blindly accepting every reviewer finding violates **Simplicity First (Rule 2)** and bloats the codebase with defensive code. Every Phase 6 finding should be explicitly challenged with three questions before triage:
     1. *Can this state actually occur given our compiler guarantees and codebase conventions?*
     2. *Does fixing it simplify the code or add speculative complexity?*
     3. *If it did occur, would it fail loudly or bypass checks silently?* Silent bypasses that don't justify code today go on the `ROADMAP.md` watch list with a trigger (`F2`, `F3-extra`); loud failures and impossible states are simply rejected (`F6`, `F7-bijection`).
2. **Separate Exploration Cycles from Implementation Cycles**:
   - Running the 7-phase loop twice—first to establish the architectural DAG taxonomy, SOTA comparison, and macro DSL (`D1`–`D8`), and second to resolve the compile-time placement and CST dogfooding design (`D9`–`D12`)—prevented premature coding while keeping the implementation diff surgical.
