# Phase 6: Review & Audit — Native CST Conformance Engine & Idiomatic Rust Policies

This document records **Phase 6 (Review and Audit)** conducted by two independent read-only reviewers:
- **Area 1 Reviewer** (`src/code_lint/ast/rust.rs` CST structural & dependency extractor)
- **Area 2 Reviewer** (`tests/architecture_conformance.rs`, `Cargo.toml`, `decisions/006_architectural_dag_and_conformance.md`, `docs/dev/rule_design_guide.md`, `ROADMAP.md`)

**Status**: Completed & Validated. RICR improvements and simplifications (**F1**, **F3**, **F4** dead-arm removal, **F5**, **F6**, **F8**, **F9**, **F10**) implemented; speculative syntax edge cases (**F2**, **F4** macro body scanning) recorded in `ROADMAP.md` watch list; contrived/historical items (**F7**, **F11**) rejected.

---

## 1. Merged Triage Table

| ID | Area & Location | Severity | Finding Summary | Final Disposition |
| :--- | :--- | :--- | :--- | :--- |
| **F1** | **Area 2**: `tests/architecture_conformance.rs:48-154`, `511-551` | **High** (Soundness & Simplicity) | 1. **Escaping & re-entering component root via `super::`**: `relative_path_boundary_violations_in_entry` checked only the final `canonical_path`, missing paths like `use super::super::ast::AstNode;` in `code_lint::ast::rust` that climb out of `code_lint::ast` into `code_lint` and re-enter.<br>2. **`super::` underflow beyond crate root**: `module_parts.pop()` silently clamped at `[]`.<br>3. **Chained `self::super::...`**: `strip_prefix("self::")` returned early without resolving subsequent `super` segments.<br>4. **Unused `PathOrigin` variants**: Reduced 5-variant `PathOrigin` to 2 variants (`Relative { shallowest_ancestor: Option<String> }` and `NonRelative`), unifying `self`/`super` loop resolution and verifying `shallowest_ancestor` stays inside the component root. | **Implemented** (RICR + Simplification) |
| **F2** | **Area 1**: `src/code_lint/ast/rust.rs:793-797`, `871-878` | **Medium** (Speculative CST Edge Case) | **Root-anchored (`::`-prefixed) paths**: `::omni::...` or `::ast_grep_core::...` has `field("path") == None` on the innermost `scoped_identifier`. Never used in this codebase (`crate::` is canonical). | **Deferred to `ROADMAP.md`** (Avoid speculative complexity) |
| **F3** | **Area 2**: `tests/architecture_conformance.rs:133-148` | **Medium** (Soundness & Simplicity) | **Bare single-segment child-module imports (`use child_mod;`)**: `resolve_reference_path` guarded local child module resolution with a redundant `raw_path.contains("::")`. Removed the redundant guard so bare child-module imports resolve to `{enclosing}::{child_mod}`. | **Implemented** (RICR + Simplification) |
| **F4** | **Area 1**: `src/code_lint/ast/rust.rs:864-885`, `981-983` | **Low–Medium** (Coherence & Simplicity) | **`macro_definition` bodies vs. dead `"metavariable"` arm**: `is_pure_path_segment` and `is_token_path_segment` checked for `"metavariable"` (`$crate`), which was unreachable because `summarize_rust_node` skips `"macro_definition"`. Removed the dead `"metavariable"` arm now; recorded `macro_rules!` body scanning on the `ROADMAP.md` watch list. | **Implemented** (Removed dead `"metavariable"` arm; macro body scanning added to `ROADMAP.md`) |
| **F5** | **Area 1**: `src/code_lint/ast/rust.rs:390-399` | **Low** (Simplicity / Rule 2 & 3) | **Single-use helper `has_matching_attribute`**: After refactoring `collect_inline_test_ranges_rec` to forward attribute tracking, `has_matching_attribute` had only one caller (`has_test_attribute`). Inlined into `has_test_attribute`. | **Implemented** (Simplification) |
| **F6** | **Area 1**: `src/code_lint/ast/rust.rs:966-971` | **Low** (Simplicity) | **Redundant CST check for `is_own_macro_path`**: Re-queried `node.field("argument")` after `expand_use_tree` already produced `target_paths`. Simplified to check `target_paths` directly. | **Implemented** (Simplification) |
| **F7** | **Area 2**: `tests/architecture_conformance.rs:468-485` | **Low** (Contrived Edge Case) | **`is_private_child_facade_reexport` relative segments in `rest`**: `pub use self::private_detail::super::public_child::Item;`. Contrived pattern that never occurs in practice. | **Rejected** (Speculative complexity) |
| **F8** | **Area 2**: `tests/architecture_conformance.rs:599-615` | **Low** (Diagnostic Clarity) | **Contradictory second violation on invalid `architecture_component!`**: Added `!entry.summary.architecture_components.is_empty()` guard in the router loop so malformed declarations aren't also flagged as missing. | **Implemented** (RICR) |
| **F9** | **Area 1 & 2**: `src/code_lint/ast/rust.rs`, `tests/architecture_conformance.rs` | **Low–Medium** (Test Completeness) | 1. Asserted `summary.visible_uses` in `test_summarize_rust_file_extracts_production_structure_and_paths`.<br>2. Added test cases for `super::super::ast::AstNode` (escape-and-reenter) and `self::super::ast::ParsedFile` in `tests/architecture_conformance.rs`. | **Implemented** (RICR) |
| **F10** | **Area 2**: `docs/dev/rule_design_guide.md:54` | **Trivial** (Documentation) | Updated stale reference from `src/rules.rs` to `tests/registry.rs`. | **Implemented** (Documentation Hygiene) |
| **F11** | **Area 2**: `docs/dev/architecture_dag/conformance_engine/03_design_plan.md:140-158` | **Trivial** (Artifact) | Historical Phase 3 sketch shows earlier `ForbiddenDependencyRule` fields (`description`). | **Rejected** (Historical Phase 3 plan record; `decisions/006_architectural_dag_and_conformance.md` is the living spec) |

---

## 2. Areas Verified Sound by Both Reviewers

1. **`collect_inline_test_ranges_rec` (Area 1)**: Forward attribute tracking accurately captures byte spans from the first preceding outer attribute through the end of the attributed item across top-level and nested scopes.
2. **`expand_use_tree` & Turbofish/Generic Path Extraction (Area 1)**: Correctly expands nested `scoped_use_list`, `use_list`, `use_as_clause`, `use_wildcard`, and `self`, and extracts both outer module prefixes and inner turbofish type arguments.
3. **DAG Reachability, Universal Sibling Isolation, and `OnceLock` Caching (Area 2)**: `compute_transitive_reachability`, `architecture_conformance_rules`, and `cached_source_files` are clean, deterministic, and thread-safe.
