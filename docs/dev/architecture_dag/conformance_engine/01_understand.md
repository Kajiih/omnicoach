# Phase 1: Understand — Native CST Conformance Engine & Idiomatic Rust Module Policies

This document records **Phase 1 (Understand)** of the implementation cycle to replace `rust_arkitect` with a native Tree-sitter CST conformance engine and align module/import rules with idiomatic Rust.

**Status**: Validated — proceeding to **Phase 2 (Gather Resources and Reference)**.

---

## 1. Context & Problem Statement

In the parent cycle ([01_understand.md](../01_understand.md)–[07_learn.md](../07_learn.md)), we established `omni`'s Reflexion Architectural DAG (`ArchitectureComponent` + `ARCHITECTURE_GRAPH`), component-root `architecture_component!(...)` declarations with subtree inheritance, content-verified pure namespace routers, and universal sibling subtree isolation.

However, the verification engine in [tests/architecture_conformance.rs](../../../../tests/architecture_conformance.rs) still relies on `rust_arkitect` (`0.3.7`), which creates four architectural and ergonomic bottlenecks:

1. **Dual-Parsing & Test Latency (~3.9s)**:
   - Every test in `tests/architecture_conformance.rs` re-reads all 43 `.rs` files in `src/`, parses them once with Tree-sitter (`ParsedFile::rust`) to blank out `#[cfg(test)]` byte ranges, and then parses them a second time with `syn` (`rust_arkitect::RustFile::from_content`).
   - Because `rust_arkitect::RustFile` wraps a `!Sync` `syn::File`, parsed files cannot be cached in a `static OnceLock`, making `tests/architecture_conformance.rs` the slowest test binary in the workspace.
2. **Workarounds for `rust_arkitect` Quirks**:
   - `rust_arkitect` resolves `crate::` to `omni::` in `use` declarations but leaves `crate::` un-normalized in inline expressions and types, requiring the `dependency_spellings` workaround.
   - `rust_arkitect` does not resolve `super::` or `self::` paths at all, leaving inline `super::` paths as a blind spot on the [ROADMAP.md](../../../../ROADMAP.md) Watch List.
3. **Overly Restrictive Idiomatic Rust Bans**:
   - Because `rust_arkitect` was blind to `super::` and cross-layer re-exports:
     - `test_no_relative_imports_in_production_code` banned all `use super::...` imports in production code, even when a child submodule (`code_lint::ast::rust`) imports from its own parent component root (`code_lint::ast`).
     - `test_items_have_a_single_path` banned all `pub use` / `pub(crate) use` declarations, preventing component roots from acting as clean facades over private implementation submodules (`mod child; pub use self::child::Item;`).
4. **Fragmented Multi-Pass CST Helpers in `src/code_lint/ast/rust.rs`**:
   - Five separate functions at the bottom of `src/code_lint/ast/rust.rs` (`collect_inline_test_ranges`, `collect_second_path_declarations`, `collect_relative_use_declarations`, `collect_external_mod_declarations`, `collect_non_namespace_items`) each traverse the CST independently, and `tests/architecture_conformance.rs` re-parses and filters `test_ranges` separately for each check.

---

## 2. Analysis of Open Design Questions (A1 & A2)

### A1: `super::` / `self::` Policy — Intra-Component Only (Option 1) vs. Anywhere Allowed by DAG (Option 2)

| Criterion | Option 1: Allow `super::` / `self::` **Within Same Component Only** (`crate::` Across Components) | Option 2: Allow `super::` / `self::` Anywhere Permitted by the DAG |
| :--- | :--- | :--- |
| **Semantic Clarity (Readability)** | **High**: The import prefix carries immediate architectural signal. `super::` / `self::` means *"internal to my component"*; `crate::` means *"external dependency on another component"*. | **Low**: `use super::semantic::bindings;` in `src/code_lint/rule.rs` looks like an internal sibling import even though it crosses from `CodeRuleContracts` into `CodeSemanticEngines`. |
| **Refactoring Robustness** | **High**: Moving a component or nesting a file deeper never breaks cross-component `crate::` imports; `super::` is only used between a child (`ast/rust.rs`) and its immediate component root (`ast.rs`). | **Low**: Encourages `super::super::diagnostic::Violation` chains that encode filesystem depth rather than architectural identity. |
| **Idiomatic Rust Alignment** | Matches standard Rust practice (e.g. `tokio`, `serde`, `ruff`, `rust-analyzer`), where `super::` is used between a parent module and its tightly coupled child submodules, while cross-subsystem imports use `crate::`. | Permits deep `super::super::` chains that idiomatic Rust style guides discourage. |

**Recommendation (D2)**: **Option 1** is strictly more RICR (Robust, Idiomatic, Cohesive, Readable). Once the CST extractor resolves `super::` and `self::` to canonical crate-relative paths:
- A relative path (`super::` or `self::`) is **valid in production code if and only if** its resolved target belongs to the **same `ArchitectureComponent`** as the enclosing file (and still respects sibling subtree isolation, so `ast/rust.rs` can import `super::AstNode` from `code_lint::ast`, but cannot import `super::python::...`).
- Any cross-component dependency must be written with an explicit `crate::` path.

---

### A2: Facade Re-Exports (`pub use`) — Private-Child Facade (Option 1)

Why did `test_items_have_a_single_path` ban `pub use` originally?
1. **Preventing Duplicate Public Paths**: If `pub mod child;` is public *and* the parent writes `pub use child::Item;`, callers can import either `parent::Item` or `parent::child::Item`.
2. **Preventing Cross-Component Smuggling**: Previously, `rules.rs` re-exported `pub use crate::core::Tag;`, which hid the true defining component (`CoreVocabulary`) from syntactic dependency checkers when callers wrote `use crate::code_lint::rules::Tag;`.

Why **Option 1 (Private-Child Facade: `mod child; pub use self::child::Item;`)** is the exact idiomatic Rust solution:
1. **True Single Public Path**: Because `mod child;` has private visibility, `parent::child::Item` is inaccessible outside `parent`. `parent::Item` is the **only** path visible to the rest of the crate—preserving the "single canonical path per item" invariant 100%.
2. **Component Encapsulation**: A component root can split large implementations across private child files (`mod helpers; pub use self::helpers::MyType;`) without leaking internal filenames into downstream callers' `use` statements.
3. **Zero Cross-Component Smuggling**: Because `child` is a direct private `mod` of `parent`, `parent::child` is guaranteed by subtree inheritance to belong to the **exact same `ArchitectureComponent`** as `parent`. Any attempt to `pub use crate::other_component::Item;` or `pub use` from a `pub mod child;` is still caught and rejected.

---

## 3. Goals & Explicit Non-Goals

### Goals
- **G1 — Eliminate `rust_arkitect` & Dual-Parsing**:
  - *Reason*: Remove the `rust_arkitect` dev-dependency (and transitive `syn` / `log` / `env_logger` crates), eliminating `syn` dual-parsing and `rust_arkitect` quirks (`dependency_spellings`, `strip_inline_tests` byte-blanking).
- **G2 — Sub-100ms Architecture Conformance Suite via Single-Pass `RustFileSummary` Caching**:
  - *Reason*: Consolidate Rust structural/dependency CST extraction into a single-pass `RustFileSummary` in `src/code_lint/ast/rust.rs` returning plain `Send + Sync` data cached via `OnceLock`, resolving the *Architecture Test Parse Caching* item on `ROADMAP.md`.
- **G3 — Complete Path Resolution (`crate::`, `omni::`, `self::`, `super::`)**:
  - *Reason*: Resolve all `use` trees and inline qualified paths (expressions, types, macro calls, attributes) to canonical module paths so inline `super::` paths can never bypass DAG or sibling isolation checks (closing the *Architecture Conformance Watch List* item on `ROADMAP.md`).
- **G4 — Support Idiomatic Intra-Component `super::` / `self::` and Private-Child `pub use` Facades**:
  - *Reason*: Stop fighting idiomatic Rust module patterns when they preserve single-path clarity and component boundaries.

### Explicit Non-Goals
- **NG1 — Full `rustc` Type/Trait Method Resolution (`hir`)**:
  - *Reason*: Syntactic path + import resolution on the CST is fast, deterministic, and sufficient when paired with single-component re-export bounds.
- **NG2 — Mass-Rewriting Existing `crate::` Imports Across `src/`**:
  - *Reason*: Existing `crate::` imports within components remain valid; we enable intra-component `super::` / `self::` and private-child `pub use` facades without churning unrelated files (Surgical Changes).

---

## 4. Numbered Decisions (D1–D6)

- **D1 — Native CST Engine**: Replace `rust_arkitect` with native CST dependency and module structure extraction in `src/code_lint/ast/rust.rs` and remove `rust_arkitect` from `Cargo.toml`.
- **D2 — Intra-Component Relative Paths**: Resolve `super::` and `self::` in both `use` declarations and inline paths. Allow `super::` / `self::` in production code when the resolved target belongs to the **same `ArchitectureComponent`** (subject to sibling isolation); forbid `super::` / `self::` that cross into another component or router.
- **D3 — Private-Child Facade Re-Exports**: Allow `pub use` / `pub(crate) use` in a module when it re-exports from a **private direct child submodule** (`mod child;`, not `pub mod child;` or `pub(crate) mod child;`), preserving a single canonical path per item and guaranteeing intra-component ownership.
- **D4 — Single-Pass `RustFileSummary`**: Consolidate the architecture/structural CST extractors in `src/code_lint/ast/rust.rs` into a cohesive `RustFileSummary` API that extracts production imports/paths, `architecture_component!` declarations, `mod` declarations (with visibility), non-namespace items, and second-path declarations in one pass (keeping `collect_inline_test_ranges` for `src/code_lint/runner.rs`).
- **D5 — `OnceLock` Test Caching**: Cache the parsed `RustFileSummary` map for `src/` in a `static OnceLock` in `tests/architecture_conformance.rs` so all conformance tests share a single CST parse pass.
- **D6 — Work Directory**: Record all Phase 1–7 artifacts for this cycle under `docs/dev/architecture_dag/conformance_engine/`.

---

## 5. Open Questions for Phase 2 & Phase 3

- **Q1 (CST Path Extraction Completeness)**: Which exact `tree-sitter-rust` node kinds can reference a module path in production code (`use_declaration`, `scoped_identifier`, `scoped_type_identifier`, `macro_invocation`, `attribute_item`), and how should nested `use` lists (`use a::{self, b as c, d::*}`) and turbofish paths (`a::b::Foo::<T>::bar()`) be normalized?
- **Q2 (Domain Router Classification Roadmap Item)**: With `RustFileSummary` already extracting `mod` declarations and non-namespace items in the single cached pass, should we keep the CST content-verified pure router check for `lib.rs` / `code_lint.rs` / `command_lint.rs`, or combine it with topological descendant checking?
