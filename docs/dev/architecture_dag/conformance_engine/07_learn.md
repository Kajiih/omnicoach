# Phase 7: Learn — Native CST Conformance Engine & Idiomatic Rust Policies

This document records the durable architectural, performance, and process lessons distilled from replacing `rust_arkitect` with our native Tree-sitter CST conformance engine and enforcing idiomatic Rust module patterns.

---

## 1. Key Technical & Architectural Lessons

1. **Single-Pass CST Extraction Beats Multi-Parser Layering**:
   - Maintaining two separate Rust parsers (`tree-sitter-rust` via `ast-grep` for lint rules and `syn` via `rust_arkitect` for architectural conformance) imposed both a dependency cost (`rust_arkitect` + `syn`) and a runtime cost (~3.98s in debug mode due to repeated disk reads and dual parsing).
   - Extracting a single-pass `RustFileSummary` directly from the existing `ParsedFile` CST (`summarize_rust_file`) and caching it once across all conformance tests via `LazyLock` (`SOURCE_FILES`, `DECLARED_COMPONENTS`, `ARCHITECTURE_CONFORMANCE_RULES`) reduced `tests/architecture_conformance.rs` debug runtime from **~3.98s to ~0.45s (~9x speedup)** while removing 4 single-purpose test CST walkers (`collect_second_path_declarations`, `collect_relative_use_declarations`, `collect_external_mod_declarations`, `collect_non_namespace_items`).

2. **Watch for Hidden $O(N^2)$ Sibling Walks in Tree-Sitter Traversal**:
   - Profiling revealed that `collect_inline_test_ranges_rec` previously called `node.prev()` in a backwards sibling loop for every child node and recursed into `token_tree` nodes (`macro_rules!`, `rule_test!`). On large files with big declarative test macros, this created a quadratic hotspot during every `ParsedFile::from_str("rs", ...)` call.
   - Replacing backwards `node.prev()` scanning with a single forward attribute-tracking pass (`pending_attr_start` / `pending_has_test_attr`) and skipping `token_tree` nodes sped up both `tests/architecture_conformance.rs` and `omni-code-lint` self-dogfooding across the entire repository.

3. **Align Architectural Enforcement with Idiomatic Language Boundaries**:
   - Blocking all `super::`/`self::` imports and all `pub use` re-exports conflicted with idiomatic Rust inside multi-file components (`src/code_lint/ast.rs` + `src/code_lint/ast/*.rs`).
   - Scoping relative paths to the enclosing `ArchitectureComponent` root (verifying both the resolved target and the shallowest ancestor reached while climbing `super::` stay within the component root) and allowing private-child facade re-exports (`mod detail; pub use self::detail::Item;`) preserves a single canonical public path per item while supporting idiomatic Rust module encapsulation.

4. **RICR Over Speculative Syntax Coverage**:
   - Independent review surfaced both genuine soundness/simplification wins (collapsing a 5-variant `PathOrigin` enum into 2 variants while catching escape-and-reenter `super::super::ast::...` paths) and hypothetical Rust syntax edge cases (root-anchored `::omni::...` paths and cross-component calls inside unexpanded `macro_rules!` bodies).
   - Implementing the simplifying RICR fixes immediately while recording hypothetical syntax constructs on the `ROADMAP.md` watch list keeps the production extractor minimal and free of dead or speculative branches.
