# Phase 2: Gather Resources and Reference — SOTA & Internal Audit

This document records **Phase 2 (Gather Resources and Reference)** for `omni`'s architectural specification and conformance system. It surveys academic foundations, external state-of-the-art (SOTA) tools across ecosystems, and internal codebase findings (including an audit of `rust_arkitect` v0.3.7 and our 5 PoC tracks).

**Status**: Validated by user — proceeding to **Phase 3 (Design/Plan)**.

---

## 1. Academic & Conceptual Foundation

### R1 — Software Reflexion Models (Murphy, Notkin & Sullivan, FSE '95 / IEEE TSE '01)
- **Key Idea**: Decouples architectural conformance into three strictly separated artifacts:
  1. **High-Level Model ($H$)**: A directed graph of conceptual components and permitted dependency edges.
  2. **Source Mapping ($M$)**: A mapping assigning concrete source files/modules to nodes in $H$.
  3. **Dependency Extraction & Comparison ($C$)**: Extracting concrete code dependencies and lifting them through $M$ to detect *convergences* (valid edges), *divergences* (forbidden edges), and *absences*.
- **Relevance to `omni`**:
  - $H$ is `ARCHITECTURE_GRAPH`, $M$ is the colocated `architecture_component!(...)` declaration in each `.rs` file, and $C$ is the test-time AST dependency verifier.
  - Keeping $H$, $M$, and $C$ orthogonal ensures that changing how a module is organized on disk ($M$) does not require rewriting the high-level graph ($H$), and testing the graph algorithms on $H$ does not require touching source files ($M$).

---

## 2. External SOTA Projects & Ecosystem Survey

| System / Ecosystem | Specification Mechanism | Granularity & Mapping | Transitive vs. Direct | Slice / Leaf Isolation | External Library Encapsulation |
| :--- | :--- | :--- | :--- | :--- | :--- |
| **Spring Modulith** (Java) | Colocated `@ApplicationModule(allowedDependencies = ...)` on packages | Package root (`package-info.java`) + sub-package encapsulation | Direct or open; verified via `ApplicationModules.verify()` | Sub-packages private by default | Via `@NamedInterface` / Spring bean boundaries |
| **Tach** (Python) | Central `tach.toml` (`[[modules]] depends_on = [...]`) | Package/module paths in TOML | Direct (`strict_deps` style) | Listed per module or glob | External packages checked via `tach check-external` |
| **ArchUnit** (Java / .NET) | Fluent test DSL (`layeredArchitecture()`, `slices()`) | Package matchers (`"..service.."`) | Configurable (`mayOnlyAccessLayers` vs. transitive) | `SlicesRuleDefinition.slices().should().notDependOnEachOther()` | `classes().that().resideOutsideOfPackage(...).should().notDependOnClassesThat()...` |
| **Deptrac** (PHP) | YAML (`layers`, `collectors`, `ruleset`) | Collector predicates (directory, regex, attribute) | Direct per ruleset entry | Separate layers per slice | Dedicated layer for vendor/third-party classes |
| **Import Linter** (Python) | `.importlinter` contracts (`layers`, `independence`, `forbidden`) | Module names | Transitive in `layers`; transitive in `independence` | Dedicated `independence` contract type | `forbidden` contract targeting external modules |
| **Rust `pub(in ...)` / Workspaces** | Compiler visibility (`pub(crate)`, `pub(in crate::...)`) or `Cargo.toml` | Module tree or crate boundary | Compiler-enforced | Cannot forbid sibling imports if parent exposes items `pub(crate)` | `Cargo.toml` per-crate only (not intra-crate) |

### Key Takeaways & Comparisons Across SOTA

1. **Centralized Topology ($H$) vs. Distributed Edges (Tach/Deptrac vs. Spring Modulith)**:
   - Spring Modulith distributes the dependency edges (`allowedDependencies`) across `package-info.java` files. While colocated, you cannot see the whole DAG in one place or visually audit the architecture without running a report generator.
   - Deptrac, Tach, and ArchUnit keep the **topology graph ($H$) centralized** in one readable table, while Deptrac allows **membership ($M$) to be colocated** via attributes/tags in source files.
   - **Synthesis for `omni` (`Q1`)**: Our hybrid model—colocated `architecture_component!(<Variant>)` in `src/` ($M$) + centralized `define_architecture!` in `tests/architecture.rs` ($H$)—combines the best of both worlds: every file self-documents its role, and the entire DAG topology fits on one screen.

2. **Transitive Frontier ($\to^+$) vs. Explicit Direct List**:
   - In Tach and Deptrac, if Layer 5 uses types from Layer 1 (`FoundationPrimitives`), Layer 2 (`CoreVocabulary`), Layer 3 (`CodeSyntaxAdapters`), and Layer 4 (`CodeRuleContracts`), you must repeat Layers 1, 2, 3, and 4 in Layer 5's `depends_on` list.
   - In `omni`, every rule file (`CodeLintRules`) uses `Diagnostic` (`FoundationPrimitives`), `Tag` (`CoreVocabulary`), `ParsedFile` (`CodeSyntaxAdapters`), and `CodeRule` (`CodeRuleContracts`). Computing **transitive reachability ($\to^+$)** over the DAG lets `CodeLintRules` declare only its immediate frontier `[CodeRuleContracts, CodeSuppressionEngine]`, keeping the graph specification minimal and noise-free.
   - **Caveat (`Q3`)**: Transitive reachability means that if `CodeSyntaxAdapters` imports an external crate (`ast_grep_core`), transitive closure does *not* automatically forbid higher layers from importing `ast_grep_core` directly—which is why external crate encapsulation (`AST_GREP_OWNERS`) is a distinct concern from internal DAG reachability.

3. **Orthogonal Leaf Independence (`ArchUnit Slices` / `Import Linter independence`)**:
   - ArchUnit (`SlicesRuleDefinition`) and Import Linter (`independence` contract) treat **sibling isolation** as a first-class concept: modules inside a plugin family (`rules::*`, `semantics::*`, `bin::*`) share the same external dependency frontier in the DAG, but have **zero internal edges** (`allow_internal_dependencies = false` / `(no_internal_dependencies)`).

4. **Compile-Time Annotation Checking (`Spring Modulith` vs. `Q2`)**:
   - In Spring Modulith, `@ApplicationModule` is a real type checked by the compiler and understood by IDEs.
   - In Rust, because Cargo compiles `src/lib.rs` *before* integration tests (`tests/architecture.rs`), an enum defined inside `tests/architecture.rs` is invisible to `src/lib.rs`.
   - **Trade-off Analysis for `Q2`**:
     - *Track A (Test-Only Enum in `tests/architecture.rs`)*: Zero footprint in `src/` beyond the 3-line no-op `macro_rules! architecture_component { ($component:ident) => {}; }`. Typos in `architecture_component!(...)` are caught when running `cargo test --test architecture` (in <0.4s), with no `strum` or architecture enum compiled into the library.
     - *Track B (Crate-Visible Enum in `src/`)*: Moving `ArchitectureComponent` into `src/` allows `architecture_component!` to expand to `const _: ArchitectureComponent = ArchitectureComponent::$component;`, giving `rustc` compile-time checks and IDE hover docs, **but** it forces the architecture test specification (`define_architecture!`) into production `src/` (violating the separation between `src/` and `tests/`).
     - **Conclusion on `Q2`**: Keeping `define_architecture!` and `ArchitectureComponent` in `tests/architecture.rs` (Track A) preserves strict separation between production code and test verification, especially once `extract_architecture_component` uses AST parsing rather than string hacking.

---

## 3. Internal Codebase & `rust_arkitect` Audit (`Q3` & `Q4`)

### R2 — Audit of `rust_arkitect` (`v0.3.7`)
By inspecting the source of `rust_arkitect` in the Cargo cache (`src/rust_file.rs`, `src/dependency_parsing.rs`, `src/rules/must_not_depend_on.rs`), we found:

1. **`RustFile` Already Holds a Full `syn::File` AST**:
   - `rust_arkitect::rust_file::RustFile` parses the file with `syn::parse_str` and exposes `pub ast: syn::File` publicly (`syn` is already in the dependency tree via `rust_arkitect` and `strum`).
2. **Quirks in `rust_arkitect`'s Dependency Extraction**:
   - **`crate::` Spelling Inconsistency**: `collect_dependencies_from_tree` rewrites `use crate::foo` to `"omni::foo"`, whereas `DependencyVisitor::visit_expr_path` and `visit_type_path` keep inline `crate::foo::bar()` as `"crate::foo::bar"`. This is why `tests/architecture.rs` needs `dependency_spellings(module) -> [String; 2]`.
   - **Unaliased External Expression Paths**: `visit_expr_path` only records paths starting with `"crate"`, `"super"`, or an imported alias (`self.aliases.get(other)`). If code calls an external crate function inline without a `use` statement (e.g. `ast_grep_core::AstGrep::some_fn()`), `visit_expr_path` silently ignores it!
   - **Inline `#[cfg(test)]` Modules**: `parse_inline_module` in `rust_arkitect` blindly recurses into all `Item::Mod` blocks—including `#[cfg(test)] mod tests { ... }`—unless those items are stripped before passing `RustFile` to the rules.

### R3 — Audit of Line-Based String Hacks in `tests/architecture.rs` (`Q4`)
Currently, `tests/architecture.rs` uses four line-by-line string heuristics that have real edge-case bugs:

1. **`strip_inline_tests(source: &str)`**:
   - *Current*: Loops over `source.lines()` and `break`s at the first line starting with `#[cfg(test)]`.
   - *Defect*: If a `#[cfg(test)]` attribute is placed on a helper function or struct *above* production functions in a file (or appears at the start of a line inside a raw multiline string literal in a rule test!), `strip_inline_tests` truncates the remainder of the file, **silently exempting any production code below it from architecture verification**!
2. **`extract_architecture_component(source: &str)`**:
   - *Current*: Scans lines with `trimmed.strip_prefix("architecture_component!(")`.
   - *Defect*: Line-based text matching ignores comments (`/* ... */`), string literals, or multi-line formatting, and does not verify that `architecture_component!(...)` is declared at most once per file.
3. **`second_path_declarations(source: &str)`**:
   - *Current*: Checks `line.starts_with("pub use ")` and `source.contains(&format!("macro_rules! {name} {{"))`.
   - *Defect*: Fragile against formatting changes (e.g., `macro_rules! name (` with parentheses or newline before `{`, or multiline `pub use`).
4. **`test_no_relative_imports_in_production_code`**:
   - *Current*: Checks `trimmed.starts_with("use super::")` while toggling a boolean `if trimmed.starts_with("#[cfg(test)]") { in_test_module = true; }`.
   - *Defect*: Same truncation bug as `strip_inline_tests`, and misses inline `super::foo()` path expressions or grouped `use self::super::...`.

### R4 — Leveraging `omni`'s Own `ParsedFile` (`src/code_lint/ast.rs`) or `syn::File` (`Q4`)
Wait: `omni` is itself a Rust/Python static analysis linter!
And in `tests/architecture.rs`, we have two structural AST options available with **zero new dependencies**:
1. **`omni::code_lint::ast::ParsedFile` (Dogfooding `omni`)** or
2. **`RustFile.ast` (`syn::File` already parsed by `rust_arkitect`)**:
   - Notice that `RustFile::from_content` in `rust_arkitect` *already* parses every file in `src/` into `rust_file.ast: syn::File`!
   - Wait: if we strip `#[cfg(test)]` items directly on `rust_file.ast.items` (removing only the specific `syn::Item`s that carry `#[cfg(test)]`), we only parse each `.rs` file **once**, and we never truncate production items that appear after a `#[cfg(test)]` item!

---

## 4. Validated Phase 2 Decisions (`D9`–`D12`)

1. **`D9` (`Q1` — Two-Level Macro Architecture)**:
   - Keep the two-level macro separation: `architecture_graph!` (generic slice builder `&[ComponentDefinition<Component>]` usable directly in unit tests with arbitrary node types) and `define_architecture!` (generating `pub enum ArchitectureComponent` + `pub const ARCHITECTURE_GRAPH` by delegating to `architecture_graph!`).

2. **`D10` (`Q2` — Compile-Time Resolution of `ArchitectureComponent` in `src/`)**:
   - **User Decision**: Move `ArchitectureComponent` into `src/` so `rustc` validates `architecture_component!(Variant)` during `cargo check --lib` (Track B).
   - Expanding `architecture_component!(Variant)` to a typed constant `const _ARCHITECTURE_COMPONENT: $crate::...::ArchitectureComponent = $crate::...::ArchitectureComponent::$component;` gives immediate compile-time checking of variant names (`E0599`), compile-time rejection of duplicate declarations in the same module (`E0428`), IDE hover doc comments, and eliminates the 3 copy-pasted no-op `macro_rules! architecture_component` stubs in `src/bin/*.rs`.

3. **`D11` (`Q3` — External Crate Encapsulation `AST_GREP_OWNERS`)**:
   - **User Decision**: Keep external crate encapsulation (`AST_GREP_OWNERS`) as a separate module-level boundary rule, since only `bin::ast_dumper` (and not all `ApplicationBinaries`) may import `ast_grep_*`.

4. **`D12` (`Q4` — Replace Fragile Line-Based String Heuristics with Structural AST Checks)**:
   - **User Decision**: Fix the mid-file `#[cfg(test)]` truncation bug in `strip_inline_tests`, enforce single-declaration semantics for `architecture_component!(...)` structurally, and replace the line-based string prefix heuristics in `second_path_declarations` and `test_no_relative_imports_in_production_code` with structural `syn::File` AST checks.
