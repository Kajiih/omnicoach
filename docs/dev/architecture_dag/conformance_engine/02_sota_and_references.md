# Phase 2: Gather Resources and Reference — Native CST Conformance Engine & Idiomatic Rust Policies

This document records **Phase 2 (Gather Resources and Reference)** for replacing `rust_arkitect` with a native Tree-sitter CST conformance engine and enforcing idiomatic Rust module/import rules.

**Status**: Validated — proceeding to **Phase 3 (Design/Plan)**.

---

## 1. Internal Codebase & `tree-sitter-rust` Grammar Reference

### 1.1. Current `src/code_lint/ast/rust.rs` Traversal & Duplication Analysis
Currently, [src/code_lint/ast/rust.rs](../../../../src/code_lint/ast/rust.rs) exposes six separate functions used by [tests/architecture_conformance.rs](../../../../tests/architecture_conformance.rs):
1. `collect_inline_test_ranges(file)` (lines 410–427): Walks the CST to find byte spans of items preceded by `#[cfg(test)]` or test attributes (`#[test]`, `#[rstest]`, `#[test_case]`). Also used by `src/code_lint/runner.rs`.
2. `collect_macro_invocations(file)` + `macro_terminal_name` + `extract_macro_arguments` (lines 453–571): Used to extract `architecture_component!(...)` declarations and by `no_assertion_packing.rs`.
3. `collect_second_path_declarations(file)` (lines 731–764): Scans for `#[macro_export]` and visible `use_declaration`s (`pub use`, `pub(crate) use`).
4. `collect_relative_use_declarations(file)` (lines 768–777): Scans for `use_declaration` nodes containing a `super` token.
5. `collect_external_mod_declarations(file)` (lines 781–788): Collects top-level `mod_item` nodes without a `body` (`mod foo;`).
6. `collect_non_namespace_items(file, allow_macro_definitions)` (lines 793–818): Collects top-level production items that are not external `mod` declarations.

In `tests/architecture_conformance.rs`, each helper re-parses the source string with `ParsedFile::rust(source)` and recomputes `collect_inline_test_ranges(&parsed)`—resulting in **6+ Tree-sitter parses and 2+ `syn` parses per file** across the test suite (~3.9s total).

### 1.2. `tree-sitter-rust` CST Node Taxonomy for Module & Path Extraction

| Rust Construct | `tree-sitter-rust` CST Representation | Extraction & Normalization Strategy |
| :--- | :--- | :--- |
| **`use` declarations** | `use_declaration` with optional `visibility_modifier` (`pub`, `pub(crate)`, `pub(super)`) and `argument` (`identifier`, `scoped_identifier`, `use_as_clause`, `use_wildcard`, `scoped_use_list`, `use_list`) | Recursively expand prefix trees (e.g., `use a::b::{self, c as d, e::*}` $\to$ `["a::b", "a::b::c", "a::b::e"]`). Record visibility (`is_visible`) and line number. |
| **Inline qualified paths (expressions, patterns, attributes, macro names)** | `scoped_identifier` (`field("path")` + `"::"` + `field("name")`) | Extract outermost `scoped_identifier` nodes whose parent is not another `scoped_identifier`, `scoped_type_identifier`, or `use` tree node. |
| **Inline qualified types & trait bounds** | `scoped_type_identifier` (`field("path")` + `"::"` + `field("name")`) | Extract outermost `scoped_type_identifier` nodes. For turbofish/generics (`a::b::Foo::<c::d::Bar>::baz()`), `a::b::Foo` and `c::d::Bar` are separate outermost scoped nodes, so both are captured automatically. |
| **Paths inside macro `token_tree` arguments** | Flat token sequence inside `token_tree`: `crate`/`super`/`self`/`identifier` followed by one or more `"::"` + `identifier` tokens | Scan `token_tree` children for contiguous `<seg> (:: <seg>)+` token runs (an improvement over `syn`/`rust_arkitect`, which leaves macro argument token streams unparsed). |
| **External `mod` declarations** | Top-level `mod_item` with `field("body").is_none()` and optional `visibility_modifier` | Extract module name (`field("name")`) and whether it has a `visibility_modifier` (`is_public_or_restricted` vs. private `mod name;`). |

---

## 2. External State-of-the-Art (SOTA) References

| Reference Project | Ecosystem | Key Mechanism | Relevance & What We Adopt / Improve |
| :--- | :--- | :--- | :--- |
| **1. `ruff` (`flake8-tidy-imports` / `TID252` & `oxc` `ModuleRecord`)** | Rust (Python / JS / TS linters) | - **Single-Pass Module Record**: Extracts all imports, re-exports, and symbol references into a flat per-file struct during a single CST/AST pass.<br>- **Scoped Relative Import Policy (`ban-relative-imports = "parents"`)**: Allows local relative imports within a cohesive package boundary while banning relative imports that climb out into parent/other packages. | **Direct Reference for D2 & D4**:<br>- We adopt the single-pass `RustFileSummary` (analogous to `ModuleRecord`) cached in a `OnceLock`.<br>- We improve on depth-based `ban-relative-imports` by using our **explicit `ArchitectureComponent` DAG**: `super::`/`self::` is valid iff the resolved target belongs to the *same* `ArchitectureComponent` as the source file. |
| **2. `import-linter` / `grimp` & `dependency-cruiser`** | Python / TypeScript | - **Upfront Path Canonicalization**: Normalizes all relative imports (`..foo`, `./bar`) against the importing file's module path into canonical package paths *before* evaluating graph contracts (`layers`, `independence`, `forbidden`). | **Direct Reference for G3 (Path Canonicalization)**:<br>- By normalizing `crate::`, `omni::`, `self::`, and `super::` against `file_module_path` during CST extraction, the conformance rules only ever compare canonical module prefixes (eliminating `dependency_spellings` and closing the inline `super::` blind spot). |
| **3. `cargo-modules` & Rust API Guidelines (`C-REEXPORT` / Facade Pattern)** | Rust | - Distinguishes **private implementation submodules** (`mod imp;`) whose items are re-exported at the parent facade (`pub use self::imp::PublicType;`) from duplicate public paths (`pub mod imp;` + `pub use imp::PublicType;`). | **Direct Reference for D3 (Private-Child Facade Re-Exports)**:<br>- A `pub use` / `pub(crate) use` in module `M` is an idiomatic single-path facade iff its target resolves to `M::<child>::...` where `mod <child>;` is declared in `M` with **private** visibility (no `visibility_modifier`). |
| **4. `rust-analyzer` (`hir_def::nameres` Uniform Paths)** | Rust | - Edition 2018+ uniform path resolution: `crate::X` anchors at the crate root; `self::X` anchors at `current_module`; `super::X` pops one segment from `current_module` per leading `super::` token; bare `child::X` in a `use` declaration in module `M` resolves to `M::child::X` if `mod child;` is declared in `M`. | **Direct Reference for `resolve_module_path`**:<br>- Checking `local_mods` (`mod child;`) also allows resolving bare child facades (`pub use child::Item;` in addition to `pub use self::child::Item;`). |

---

## 3. Synthesis & Improvements Over References

1. **Over `rust_arkitect` (`0.3.7`)**:
   - Eliminates `syn` dual-parsing and the `!Sync` `RustFile` bottleneck.
   - Replaces string byte-blanking (`strip_inline_tests`) with native CST test-subtree skipping (during the `RustFileSummary` walk, any node with `#[cfg(test)]` or `#[test]` is simply not recursed into—zero string allocation or byte mutation!).
   - Inspects paths inside macro `token_tree` arguments (`assert!(crate::foo::bar())`), which `syn` ignores.
   - Resolves `super::` and `self::` in both `use` declarations and inline paths.
2. **Answering Phase 1 Open Questions**:
   - **Q1 (CST Path Extraction Completeness)**: Handled by combining (a) recursive `use_declaration` tree expansion, (b) outermost `scoped_identifier` and `scoped_type_identifier` extraction, and (c) `token_tree` `::`-chain scanning, all skipping `#[cfg(test)]` / `#[test]` CST subtrees directly during traversal.
   - **Q2 (Domain Router Classification)**: Because `RustFileSummary` already extracts `external_mods` and `non_namespace_items` in the single cached pass at zero extra cost, we combine **both** invariants in `test_all_source_files_declare_architecture_component`:
     1. *Topological Router Invariant*: Every unannotated router module (`lib.rs`, `code_lint`, `command_lint`) must be an ancestor of at least one declared `ArchitectureComponent` root (preventing dead/orphan routers).
     2. *CST Purity Invariant*: Every router module must contain zero production code or imports (`non_namespace_items.is_empty()`).
     This completely resolves the *Domain Router Classification vs. CST Content Verification* roadmap item as well.
