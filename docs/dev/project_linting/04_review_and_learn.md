# Project-Aware Linting & Architectural Conformance — Phases 6 & 7: Review, Verification, and Lessons Learned

> Phases 6 (Review and Audit) and 7 (Learn) of the project-aware linting cycle, completing the
> seven-phase engineering workflow started in [01_understand.md](01_understand.md),
> [02_references.md](02_references.md), and [03_design.md](03_design.md).

## TL;DR

- **Phase 6 Independent Multi-Reviewer Audit:** Three independent pedantic reviewers audited the 7-commit
  implementation across `CodeLintAst`, `CodeLintProject`, `CodeLintContract`, `CodeLintRunner`, and the
  11 new/upgraded `ProjectCodeRule`s. Every correctness, boundary, and performance finding was either
  resolved in code with regression tests or recorded in [ROADMAP.md](../../../ROADMAP.md).
- **End-to-End Release Benchmarks:** On CPython `Lib` (`1,929` Python files) and `gauge-sh/tach`
  (`144` Python files), `omnicoach code` with all 76 rules enabled runs at **`1.14x` median (`1.15x`
  minimum)** and **`1.20x` median (`1.07x` minimum)** of the single-file-only baseline (inside the
  `<= 1.2x` target), while `omnicoach hook antigravity pre-tool-use` completes in **`5.4–6.7 ms`
  median (`6.6–7.5 ms` p90)** on Python repositories and **`43.5 ms` median (`46.6 ms` p90, `42.3 ms`
  minimum)** on `omnicoach`'s own 24-component Rust codebase (`241.4 ms` CLI median vs `186.1 ms`
  single-file baseline, `1.30x`).
- **Self-Hosting & Test Verification:** All library unit tests, CLI integration tests
  (including `test_self_dogfooding_code_lint`), rule registry invariants,
  `architecture_conformance` tests (exercising the production `Topic::ARCHITECTURE` rules and
  `evaluate_file_architecture`), and `cargo clippy --all-targets -- -D warnings` pass with zero warnings.

---

## 1. End-to-End Release Benchmark Verification

All measurements below were collected using the release binary (`cargo build --release --bin omnicoach`)
across 5–9 repetitions per target on CPython `Lib` (`1,929` Python files), `gauge-sh/tach` (`144`
Python files in a mixed Python + Rust repository), and `omnicoach` itself (`140` Rust files in `src/`
plus `tests/`).

| Corpus & Workload | All 76 Rules Median | Single-File Baseline Median | Wall-Time Ratio | Hook Pre-Tool-Use Median (p90) | Acceptance Gate |
| :--- | :--- | :--- | :--- | :--- | :--- |
| **CPython `Lib` (`1,929` Python files)** | `1,623.5 ms` (`1,448.8 ms` min) | `1,429.5 ms` (`1,262.9 ms` min) | `1.14x` (`1.15x` min) | `6.7 ms` (`7.5 ms` p90) | `<= 1.2x` CLI, `<= 35 ms` hook |
| **`gauge-sh/tach` (`144` Python files)** | `47.6 ms` (`38.7 ms` min) | `39.5 ms` (`36.1 ms` min) | `1.20x` (`1.07x` min) | `5.4 ms` (`6.6 ms` p90) | `<= 1.2x` CLI, `<= 35 ms` hook |
| **`omnicoach` (`140` Rust files, 24-component DAG)** | `241.4 ms` (`207.0 ms` min) | `186.1 ms` (`145.3 ms` min) | `1.30x` (active 24-component DAG) | `43.5 ms` (`46.6 ms` p90) | `0` self-dogfooding violations |

**Context, Analysis & Interpretation:**

- **What was tested:** Full release CLI runs (`omnicoach code`) comparing all 76 rules against
  `--ignore architecture,uninherited-protocol,copied-base-docstring,partial-override-docstring,deferral-docstring`,
  plus live `omnicoach hook antigravity pre-tool-use` single-file edit invocations via stdin JSON.
- **What moved and why:**
  1. **Lazy candidate short-circuits in Python cross-file rules:** Initially, `deferral-docstring`,
     `copied-base-docstring`, and `partial-override-docstring` extracted classes and resolved cross-file
     C3 MROs on every Python file before checking whether any method carried `@override`, and
     `uninherited-protocol` resolved `imported_protocols_in_module` before checking whether the file had
     any candidate class with `>= min_methods` public non-property methods. Guarding the three
     `@override` rules on `file.source_text().contains("override")` and guarding `uninherited-protocol`
     on candidate class presence cut CPython `Lib` cross-file overhead by over `600 ms`.
  2. **Language-separated architecture specification lookup:** In mixed Python + Rust repositories like
     `tach` (`tach/python/` + `tach/src/`), linting Python files previously triggered
     `ProjectIndex::rust_headers_map()`, scanning and parsing `src/*.rs` even though Rust
     `define_architecture!` macros never apply to Python modules. Splitting `python_architecture_spec()`
     (TOML-only) from `architecture_spec()` eliminated all Rust file reads during Python lint runs,
     reducing `omnicoach code python` on `tach` from `131 ms` to `56 ms` (`1.12x` baseline).
  3. **Macro-candidate filtering in `rust_headers_map()` & precomputed `ArchitectureSpec` tables:**
     Filtering Rust header scans to modules whose text mentions `architecture_component!` or
     `define_architecture!` (or a configured `isolate-children` root), then letting the body-free
     `summarize_rust_module_header` decide which mentions are top-level, non-test declarations
     (checking outer `#[cfg(test)]` attributes on direct `SourceFile` children rather than building a
     full-file `rust_inline_test_ranges` index), and precomputing `component_roots`,
     `allowed_dependencies`, `component_by_name`, `known_components`, `rust_sibling_rules`, and
     `ExportPattern` `GlobMatcher`s once inside `finalize_architecture_spec` cut `omnicoach` CLI wall
     time from `597 ms` to `241 ms` (`+55 ms`, `1.30x` for full 24-component DAG verification across
     140 Rust files) and `omnicoach hook` latency to `43.5 ms` median (`46.6 ms` p90). An earlier
     line-based prefilter that stopped at the first `#[cfg(test)]` line could drop a declaring file;
     the substring prefilter never does, and costs about `4 ms` more per hook run on `omnicoach`
     (61 alternating runs on a loaded machine: median `55 ms` vs `47 ms`, minimum `36 ms` vs `33 ms`).
  4. **Unifying `CodeLintProject` foundation primitives and conformance testing:**
     `ProjectLayout`, `config::lexical_normalize`, `resolve_relative_python_package`,
     `resolve_relative_python_module`, `rust_module_from_relative`, and `ArchitectureSpec::resolve_component_of_module`
     now serve as the single source of truth across `ProjectIndex`, `CodeLintGraph` (`facts.rs`),
     `layout.rs`, and `hook/runner.rs`, and `tests/architecture_conformance.rs` delegates directly
     to the production `Topic::ARCHITECTURE` rules and `evaluate_file_architecture` rather than
     maintaining a duplicate conformance engine.
  5. **Baseline re-lint scoped to spillover rules in `omnicoach hook`:** When a post-edit diagnostic
     lands on an unmodified line, the hook lints the pre-edit source to discount pre-existing
     findings. That baseline runs only the rules that reported spillover (every rule when a
     suppression audit is among them, since audits judge directives against the rules that ran), so
     a file with pre-existing single-file findings no longer builds `ProjectIndex` twice. On
     `omnicoach` with one pre-existing `single-letter-name` finding, the hook median dropped from
     `105 ms` to `63 ms` (minimum `51 ms` to `37 ms`; 61 alternating runs on a loaded machine).
  6. **`define_architecture!` read only from the crate's `architecture` module:** The crate-wide
     header scan of item 3 runs only when `src/architecture.rs` or `src/architecture/mod.rs`
     declares `define_architecture!`; probing that module costs at most one file read and parses
     its header only when the text mentions the macro, while `invalid-architecture-component` still
     flags stray `architecture_component!` calls in the edited file without scanning the crate. On
     a clean edit of `ripgrep`'s `crates/ignore/src/dir.rs` (no `architecture` module), the hook
     median is `45 ms` against `52 ms` for `main@origin` (p90 `54 ms` vs `61 ms`); on `omnicoach`'s
     `src/config.rs`, which keeps the full scan, it is `30 ms` against `11 ms` (p90 `32 ms` vs
     `11 ms`). Both use 41 alternating runs of the release binaries.

---

## 2. Phase 6 Multi-Reviewer Audit Triage

Three independent reviewers audited the implementation along orthogonal slices:

1. **Reviewer 1 (`CodeLintAst` & `CodeLintProject`):** `src/code_lint/ast/python/`,
   `src/code_lint/ast/rust/`, `src/code_lint/project/`, `src/config.rs`.
2. **Reviewer 2 (`CodeLintContract`, `CodeLintRunner` & Test Harness):** `src/code_lint/contract.rs`,
   `src/code_lint/runner.rs`, `src/rule_declaration.rs`, `src/rule_catalog.rs`, `src/test_utils.rs`.
3. **Reviewer 3 (`Rules`, `Tests` & `Docs`):** All 11 `ProjectCodeRule` implementations,
   `tests/architecture_conformance.rs`, `docs/dev/project_linting/`, `ROADMAP.md`.

| Slice | Finding | Resolution |
| :--- | :--- | :--- |
| **`CodeLintProject` (`index.rs`)** | `seed_from_parsed` could allow a `.py` file to shadow a sibling `.pyi` stub, violating PEP 561 stub precedence. | Fixed in [index.rs](../../../src/code_lint/project/index.rs): `seed_from_parsed` checks `.with_extension("pyi")` before seeding `.py`. |
| **`CodeLintProject` (`index.rs`, `context.rs`)** | `resolve_python_class` checked in-file `summary.classes` before `summary.imports` when `before_class_index` was `None` (`from other import Store; class Store(Store):`). | Fixed in [index.rs](../../../src/code_lint/project/index.rs) and [context.rs](../../../src/code_lint/project/context.rs): `compute_local_class_mro` passes the number of module-level classes defined before the subclass, and `resolve_python_class` clamps to that prefix and falls back to `summary.imports` when shadowed by a later class of the same name. |
| **`CodeLintProject` (`architecture.rs`)** | `check_rust_external_owner` and `check_python_external_owner` must distinguish third-party packages from relative paths (`super::`, `self::`, `crate::`) and the current crate's own package name while sharing one `ComponentSpec::owns_external` table across `.omnicoach.toml` and `define_architecture!`. | Added explicit relative-prefix and self-crate guards before external-owner lookup and unified `.omnicoach.toml` `owns-external` and `define_architecture!` `(owns_external: [...])` into `ComponentSpec::owns_external` in [architecture.rs](../../../src/code_lint/project/architecture.rs). |
| **`CodeLintProject` (`architecture.rs`)** | `PrivateComponentImportFinding.name` held the full dotted module/symbol path instead of the terminal symbol/module name; multi-leaf imports (`use a::{b, c};`) could emit duplicate findings on the same statement span. | Fixed in [architecture.rs](../../../src/code_lint/project/architecture.rs): `name` extracts the terminal identifier, and `deduplicate_findings` deduplicates per rule by statement `SourceSpan`. |
| **`CodeLintProject` (`architecture.rs`)** | Relative Python imports climbing to `<root>` without a module suffix (`from .. import other_pkg` inside `pkg/sub.py`) were dropped instead of flagged by `cross-component-relative-path`. | Fixed in [architecture.rs](../../../src/code_lint/project/architecture.rs) with regression test `relative_import_climbing_to_root_without_module_flagged` in [cross_component_relative_path.rs](../../../src/code_lint/rules/cross_component_relative_path.rs). |
| **`CodeLintAst` (`rust/file_summary.rs`)** | `summarize_rust_file` recorded `architecture_component!` and `define_architecture!` macro calls in `non_namespace_items`, causing false `unmapped-architecture-module` findings on pure router modules. | Fixed in [file_summary.rs](../../../src/code_lint/ast/rust/file_summary.rs) with regression test `pure_architecture_definition_module_exempt` in [unmapped_architecture_module.rs](../../../src/code_lint/rules/unmapped_architecture_module.rs). |
| **`CodeLintRunner` (`runner.rs`, `config.rs`)** | `lint_virtual_file` did not lexically normalize `project_root`/`target_path` before keying `OverlayFileSource`, and `Config::is_test_path` called `std::env::current_dir()` uncached on every file. | Fixed in [runner.rs](../../../src/code_lint/runner.rs) (lexical normalization of both paths). [config.rs](../../../src/config.rs) now reads the working directory once per process (`CACHED_CURRENT_DIR`), which assumes the process never changes directory: true of the CLI and the hook, not of a long-lived embedding such as a Python extension module, which must pass the root explicitly instead. |
| **`CodeLintAst` (`python/classes.rs`)** | Public non-property method extraction was duplicated between `ast/python/file_summary.rs` and `rules/uninherited_protocol.rs`. | Consolidated into `PythonClassInfo::public_non_property_method_names(&self)` in [classes.rs](../../../src/code_lint/ast/python/classes.rs). |

A second audit of the whole stack found four more defects:

| Slice | Finding | Resolution |
| :--- | :--- | :--- |
| **`CodeLintProject` (`index.rs`)** | `compute_local_class_mro` indexed the module summary (module-level classes only) with an index into all classes of the file, nested ones included, and let a module-level class inherit from a same-named class local to a function. | Classes are matched to the summary by name span; a module-level subclass only sees module-level classes, and imports resolve against the module-level classes defined before it. `summarize_python_file` also collects classes in `for` and `while` bodies, which are module scope. |
| **`CodeLintProject` (`index.rs`)** | `c3_merge` concatenated the remaining sequences when the hierarchy has no C3 linearization, inventing ancestors for a class Python rejects with `TypeError`. | An inconsistent hierarchy yields the class alone, so no rule claims an inherited method or docstring. |
| **`CodeLintProject` (`index.rs`, `architecture.rs`), `CodeLintRules` (`invalid_architecture_component.rs`)** | Every Rust file mentioning an architecture macro was parsed on every project-aware run, `architecture_component!` alone synthesized a component DAG so any crate paid for a crate-wide scan, and invalid component markers were folded into `unmapped-architecture-module`. | `define_architecture!` is read only from the crate's `architecture` module (`src/architecture.rs` or `src/architecture/mod.rs`); without it, only `[architecture]` roots with `isolate-children` are read. `architecture_component!` only assigns roots, only `define_architecture!` in `architecture` conflicts with `[[architecture.components]]`, and `invalid-architecture-component` flags stray, unknown, nested, non-top-level, and duplicate `architecture_component!` calls. |
| **`CodeLintRunner` (`runner.rs`), `CodeLintProject` (`layout.rs`)** | A root `.omnicoach.toml` `[architecture]` did not apply to a sub-package with its own `pyproject.toml`, and configured `source-roots` resolved against the sub-package instead of the configuration root. | The configuration applies when its root is the project root or an ancestor of it, and `configured_source_roots` anchors roots at `Config::root_dir`. |

---

## 3. Durable Lessons Learned (Phase 7)

1. **Prototype competing loading strategies on real-scale corpora before designing the index:**
   Benchmarking `EagerParallel`, `OnDemand`, and `Hybrid` on CPython `Lib` (`1,929` files) in Phase 3
   proved that eager whole-project indexing fails both the CLI `1.2x` wall-time budget and the `35 ms`
   agent-hook latency budget. Hybrid on-demand indexing with fast lexical pre-filters (`contains("override")`,
   candidate method count, and Rust architecture macro mentions) keeps project-aware rules near zero
   cost on files that do not trigger cross-file queries. A lexical pre-filter may only skip work that
   cannot change the result; the syntax tree decides.
2. **Encode evaluation state (`Option<Vec<Diagnostic>>`) in the rule trait return type:**
   Returning `None` from `AnyCodeRule::check_file_in_project` when `ProjectContext` is `None` prevents
   `evaluated_rules.insert(...)` from running in single-file contexts, eliminating false
   `unused-suppression` diagnostics by construction without special-casing rule names in the suppression
   tracker.
3. **Separate language-specific architecture resolution in polyglot repositories:**
   In mixed Python + Rust repositories (such as `gauge-sh/tach`), Python files can only obtain their
   architecture specification from `.omnicoach.toml` `[architecture]`, never from Rust `define_architecture!`
   macros. Keeping `python_architecture_spec()` separate from `architecture_spec()` prevents Python
   lint runs from scanning Rust source trees.
4. **Enforce single-target diagnostics and sibling cleanliness in the multi-file test harness:**
   Having `rule_test!` lint every file in a multi-file fixture while asserting that sibling context
   files emit zero diagnostics caught multiple subtle bugs during development (such as unmapped module
   findings on helper files and duplicate findings on multi-leaf import statements).
