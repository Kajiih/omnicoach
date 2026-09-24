# Phase 7 — Learnings & Architectural Insights

Status: **complete**.

This document captures the key architectural, technical, and engineering insights gained from executing the combined **Rule Layering & Enforced Abstraction Levels** project (ROADMAP items #1 and #2).

---

## 1. The Anti-Pattern of Module-Root Facades (Re-Exports)

### The Problem
In Rust crates, developers often create module-root facades (e.g., `pub use child::Item;` in `mod.rs` or `lib.rs`) under the assumption that it improves import ergonomics. In this project, `src/code_lint.rs` and `src/command_lint.rs` re-exported types from primitives (`diagnostic`), semantic engines (`bindings`, `calls`), rule contracts (`rule`), and orchestration (`runner`).

This caused severe architectural degradation:
1. **Hidden Layer Violations**: Callers throughout the crate simply imported `crate::code_lint::*`, obscuring which layer they were actually coupling to.
2. **False Cycles**: When `code_lint/runner.rs` needed `CodeRule` and rules needed `ParsedFile`, re-exporting through `code_lint.rs` created apparent cycles at the module root.
3. **Impaired Architecture Fitness**: Static dependency checkers cannot reliably enforce downward layer boundaries if higher-layer modules re-export lower-layer types.

### The Solution: The Single-Path Pattern
Every symbol must have **exactly one canonical import path** across the entire crate:
- Primitives belong to `crate::diagnostic` or `crate::diff`.
- Rules belong to `crate::code_lint::rules::*` or `crate::command_lint::rules::*`.
- Contracts belong to `crate::code_lint::rule` or `crate::command_lint::rule`.
- Runner orchestration belongs to `crate::code_lint::runner` or `crate::command_lint::runner`.

We formalized and enforced this rule in CI via `test_items_have_a_single_path` and `test_no_relative_imports_in_production_code` in `tests/architecture.rs`. Re-exports (`pub use`), `#[macro_export]`, and non-test relative imports (`use super::`) are banned, ensuring that every import is spelled with its canonical `crate::` path.

---

## 2. Automated Architecture Enforcement with `rust_arkitect`

### Key Technical Lessons:
1. **Dual Dependency Spellings**:
   `rust_arkitect` normalizes `crate::module` to `{crate_name}::module` when parsing `use` trees, but preserves `crate::module` verbatim when analyzing inline expressions or type paths (e.g. `crate::diagnostic::Diagnostic`). To prevent bypasses, architecture rules must forbid both spellings:
   ```rust
   fn dependency_spellings(module: &str) -> [String; 2] {
       [format!("{CRATE_NAME}::{module}"), format!("crate::{module}")]
   }
   ```
2. **Scoping File Discovery**:
   `rust_arkitect` recursively walks all subdirectories if passed a root directory. When corpora, benchmarks, or scratch workspaces exist in the project root, discovery must be explicitly restricted to `src/` to prevent false positive scans on fixture files.
3. **Execution Speed**:
   Full architecture validation across the entire crate takes $\sim 0.6$ seconds, making it ideal as a mandatory pre-commit and CI verification gate.

---

## 3. Declarative Policy vs. AST Grammar Encapsulation (ROADMAP #1)

### The Anti-Pattern: Granular Node Inspection in Rules
In the baseline codebase, rules directly navigated raw Tree-sitter / `ast_grep_core` nodes:
- Rules manually checked `node.kind() == "function_definition"`, inspected `node.field("name")`, filtered children for decorators, and recursed through nested statements.
- This coupled policy rules to parser syntax details, making rules verbose and difficult to audit.

### The Solution: Opaque Handles & Batch Domain Queries
1. **Opaque Type Wrappers**:
   Raw `ast_grep_core` types are crate-private. External modules and rules only see `ParsedFile` and `AstNode<'a>`.
2. **Inversion of Control via Batch Queries**:
   Instead of rules traversing nodes, the AST grammar module (`ast::python`, `ast::rust`) provides high-level domain extractors:
   - `find_nested_functions(file)`
   - `find_unwrapped_multiline_strings(file)`
   - `collect_test_function_assertion_counts(file)`
   - `extract_classes(file)`
   - `extract_function_signatures(file)`

Rules become pure declarative policy: they declare their target, run the query, and map matches to `Diagnostic` violations. This reduced rule boilerplate by 30-50%.

---

## 4. Avoiding Orphan Code in Refactorings (User Rule 3)

When rules were migrated from granular predicates (`is_nested_function`, `is_multiline_string_literal`) to batch queries (`find_nested_functions`), the old granular AST wrappers became dead code.

### Lesson:
- In multi-step refactorings, transitioning to higher-level abstractions often leaves behind single-node inspection helpers.
- Performing a structured Phase 5 (Clean up) with baseline diff probes (`git diff` / `jj diff`) reliably surfaces these orphans so they can be deleted surgically before shipping.

---

## 5. Pragmatic Dead Code Triage vs. Premature Deletion

When auditing pre-existing dead code, a purely mechanical "delete everything with zero callers" approach is counter-productive when working with an active roadmap:

1. **Dead bloat**:
   Constructors that are never used because static `const` struct literals are preferred (e.g. `FilterListDefaults::new`) should be deleted immediately.
2. **Roadmap building blocks**:
   Helper methods that directly serve upcoming roadmap features (e.g., `PythonClassInfo::inherits_from` and `PythonParameterInfo::is_variadic` for the legacy linter rules like `FakeMustInheritProtocolRule` and `SignatureConcreteTypeRule`) and already have thorough unit tests should be retained.

By cross-referencing candidate items with the legacy rule reference (`scratch/legacy_reference/legacy_lints.py`), we avoided both bloat accumulation and premature deletion of valuable tested logic.

---

## Summary Matrix

| Objective | Baseline State | Final State | Long-Term Benefit |
| :--- | :--- | :--- | :--- |
| **Abstraction Layering** | Ad-hoc imports across modules; facades creating cycles. | Strict 6-layer downward hierarchy enforced by CI tests. | Impossible to introduce cycles or reverse dependencies. |
| **Symbol Paths** | Multiple paths per symbol (`crate::code_lint::RuleName`, etc.). | Single canonical path per item; zero re-exports. | Predictable, clear code navigation and refactoring. |
| **Rule Purity** | Rules contained raw AST traversals and tree-sitter queries. | Rules express declarative policy only. | Rules are trivial to write, review, and maintain. |
| **Parser Engine Independence** | Rules directly depended on `ast_grep_core`. | 0 rules import `ast_grep_core`. | Underlying parser or grammar can be replaced with zero rule changes. |
