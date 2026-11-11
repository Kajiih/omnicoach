# Crate Split & Workspace Extraction Roadmap

This document records the candidate crate boundaries within `omnicoach`, the in-place architectural seams to resolve before extracting them, and the criteria for deciding what stays inside the main crate versus what moves into standalone workspace crates.

Related design documents:

- [ADR 006: Architectural DAG Specification and Conformance Checking](decisions/006_architectural_dag_and_conformance.md)
- [ADR 007: Faceted Rule Taxonomy, Hierarchical Selection, and Tag-Free Rule Execution](decisions/007_rule_taxonomy_and_selection.md)
- [ADR 011: Colocated Top-Down Abstraction Ordering](decisions/011_colocated_abstraction_ordering.md)
- [ADR 012: Module Placement by Abstraction](decisions/012_module_placement_by_abstraction.md)
- [Abstraction Graph 03 Design](abstraction_graph/03_design.md) and [05 Decisions](abstraction_graph/05_decisions.md)

---

## 1. Guiding Principles for Crate Extraction

1. **Extract for cross-project reuse or strict dependency isolation, not internal layering**: Inside `omnicoach`, [ADR 006](decisions/006_architectural_dag_and_conformance.md) (`define_architecture!` in [src/architecture.rs](../../src/architecture.rs) and [tests/architecture_conformance.rs](../../tests/architecture_conformance.rs)) already enforces component DAG reachability, sibling subtree isolation, single public paths, and relative-path boundaries. Splitting an internal component into a crate is warranted only when another repository will consume it directly or when isolating heavy third-party parser dependencies (`ruff_python_parser`, `ra_ap_syntax`, `brush-parser`).
2. **Separate reusable mechanisms from `omnicoach` domain vocabulary ([ADR 012](decisions/012_module_placement_by_abstraction.md) P1, P7)**: A standalone crate owns the domain-independent data structures, algorithms, and verification checks; `omnicoach`-specific rule registries, concrete lint facets, and `.omnicoach.toml` schema wiring stay in `omnicoach`.
3. **One derivation per fact across crate boundaries ([ADR 012](decisions/012_module_placement_by_abstraction.md) P6)**: Before extracting a crate, eliminate duplicate fact derivations between test-time conformance and runtime analysis so the extracted crate exposes a single canonical implementation.

---

## 2. Pre-Split Architectural Seams (In-Place Cleanups)

One remaining seam inside `src/` couples foundational layers to higher-level `omnicoach` concepts and should be resolved before or during crate extraction (project-topology configuration has already been decoupled into `CodeLintProject`; see §2.2):

### 2.1. Split Source Span Primitives out of `diagnostic` ([ADR 012](decisions/012_module_placement_by_abstraction.md) P1)

- **Current state**: [src/diagnostic.rs](../../src/diagnostic.rs) (`Diagnostic`) bundles two distinct abstractions:
  1. **Source coordinate primitives**: `Language`, `SourceSpan`, `LineColumn`, `LineIndex`, `SourceLocation`.
  2. **Linter violation reporting**: `RuleName`, `LanguageText`, `ViolationTemplate`, `ViolationMessage`, `Diagnostic`, terminal and JSON formatters.
- **Consequence**: In [src/architecture.rs](../../src/architecture.rs), `CodeLintAst => [Diagnostic]`, `CodeLintProject => [CodeLintAst, Diagnostic]`, `CodeLintGraph => [CodeLintProject, CodeLintAst, Diagnostic]`, and `CommandLintCommand => [Diagnostic]`, even though none of these layers formats linter diagnostics—they only need `Language`, `SourceSpan`, `LineColumn`, `LineIndex`, and `SourceLocation`.
- **Target**: Extract the source coordinate primitives into a zero-dependency foundation component (`Source` / `span`) at the bottom of the DAG so `CodeLintAst`, `CodeLintProject`, `CodeLintGraph`, `CommandLintCommand`, and any extracted AST/graph/command crate do not depend on linter diagnostic types.

### 2.2. Completed: `ProjectConfig` in `CodeLintProject` ([ADR 012](decisions/012_module_placement_by_abstraction.md) P1, P7)

- **Resolved state**: Project-topology configuration (`ProjectConfig`, `ContextConfig`, `ArchitectureConfig`, `ArchitectureComponentConfig`, `DEFAULT_TEST_PATTERNS`) lives in [src/code_lint/project/config.rs](../../src/code_lint/project/config.rs) inside `CodeLintProject`, while rule-selection `Config` in [src/config.rs](../../src/config.rs) wraps `pub project: ProjectConfig` alongside `disabled_rules`, `per_file_ignores`, and `rule_options`. File-to-module derivation (`rust_module_path`), ancestor `architecture_component!` inheritance (`resolve_inherited_component_with_root`), and syntactic reference path normalization (`resolve_rust_reference_path`) live in [src/code_lint/project/layout.rs](../../src/code_lint/project/layout.rs), `FileSource` ([src/code_lint/project/source.rs](../../src/code_lint/project/source.rs)) encapsulates directory walking (`walk_files`, owning `ignore`), and `ProjectIndex` ([src/code_lint/project/index.rs](../../src/code_lint/project/index.rs)) owns cross-module symbol and re-export resolution across Rust and Python. In [src/architecture.rs](../../src/architecture.rs), `CodeLintProject => [CodeLintAst, Diagnostic]` and `CodeLintGraph => [CodeLintProject, CodeLintAst, Diagnostic]` have zero dependency on `Config` or `RuleDeclaration`.

---

## 3. Candidate Crates

### 3.1. Declarative Architecture & Reflexion Conformance (`omni-arch` / `arch-dag`)

- **Purpose**: Let any Rust project declare its component DAG in code (`define_architecture!`) and annotate module roots (`architecture_component!`) with compile-time acyclicity guarantees and single-call CST conformance verification in `cargo test`.
- **Contents**:
  - **Core (`[dependencies]`, lightweight / no parser)**:
    - `define_architecture!` macro, `architecture_component!` macro, and `ComponentDefinition` (from [src/architecture.rs](../../src/architecture.rs) and [src/lib.rs](../../src/lib.rs)).
    - Compile-time topological order assertion (`E0080` on forward/cyclic edges) and `strum` metadata derives.
  - **Conformance Runner (`[dev-dependencies]` or feature `check`)**:
    - Single-pass Rust CST summary (`RustFileSummary`, `ExternalModDeclaration`, `VisibleUseDeclaration`, `RustPathReference`) and project module/path resolver.
    - Conformance checks from [src/code_lint/project/architecture.rs](../../src/code_lint/project/architecture.rs) and [tests/architecture_conformance.rs](../../tests/architecture_conformance.rs):
      1. Every source file belongs to a declared component subtree or is a CST-verified pure namespace router (`mod`-only).
      2. Component names match `PascalCase` of their root module path.
      3. Transitive DAG reachability across all `use` trees, inline paths, turbofish args, and macro `token_tree` paths.
      4. Universal intra-component sibling subtree isolation.
      5. Single public path per item (private-child facade `mod child; pub use self::child::Item;` allowed; duplicate public paths and non-root `#[macro_export]` rejected).
      6. Relative paths (`super::` / `self::`) stay within their enclosing component root subtree.
      7. Configurable external-crate encapsulation rules (`owns_external` for restricting parser or I/O crates to designated adapter components).

### 3.2. Multi-Level Abstraction Graph (`omni-graph`, per [03_design.md](abstraction_graph/03_design.md))

- **Purpose**: Extract per-file and multi-file declaration hierarchies, entrypoint ownership (`Roots(h)`), [ADR 011](decisions/011_colocated_abstraction_ordering.md) topological abstraction levels, recursion groups (SCCs), and Murphy et al. Reflexion Model component summaries, and render them into compact structural outlines for humans and AI coding agents (`AnnotatedStubs`, `CompactBi`).
- **Contents**:
  - **Fact extractors**: `ScopeFacts`, `ItemFacts`, `RelationFact`, `OrderingEdge` ([src/code_lint/ast/scope_facts.rs](../../src/code_lint/ast/scope_facts.rs) and [src/code_lint/ast/call_clusters.rs](../../src/code_lint/ast/call_clusters.rs)) + multi-file `ProjectFacts` ([src/code_lint/project/facts.rs](../../src/code_lint/project/facts.rs)).
  - **Validated graph model & index (Layer L1)**: `AbstractionGraph`, `DeclarationNode`, `GroupNode`, `SummaryEdge`, `ReflexionStatus`, `ModelError` ([src/code_lint/graph/model.rs](../../src/code_lint/graph/model.rs)), and `GraphIndex` ([src/code_lint/graph/index.rs](../../src/code_lint/graph/index.rs)).
  - **Graph builder (Layer L2), format renderers & queries (Layer L3)**: `build_file_graph`, `build_project_graph`, `ProjectDepth` ([src/code_lint/graph/builder.rs](../../src/code_lint/graph/builder.rs)), `OutputFormat` renderers ([src/code_lint/graph/render.rs](../../src/code_lint/graph/render.rs)), and `find` / `slice` queries ([src/code_lint/graph/query.rs](../../src/code_lint/graph/query.rs)).
- **Decoupling needed**: Extract `SourceSpan` and `Language` from `Diagnostic` (§2.1).

### 3.3. Faceted Tag & Hierarchical Selector Engine (`tagonomy`)

- **Purpose**: Provide a reusable faceted classification and hierarchical selector resolution engine (`select` / `ignore` / `explain`) with compile-time and test-time taxonomy invariants for any rule, check, test, or plugin registry.
- **Contents**:
  - **Taxonomy primitives**:
    - Hierarchical `Topic` (`label`, `parent: Option<&'static Topic>`, `description`, `scope_note`, `synonyms`, `ancestors()`, `path()`, compile-time cycle prevention via `const` evaluation).
    - Trait or descriptor for facet values and classified items (mapping an item to its leaf identifier and root-to-leaf tag branches).
  - **Selection & explanation engine**:
    - Precedence Model B resolver (`Plan::decide` $\to$ `RuleStatus` / `SelectionStatus` from [src/rule_selection.rs](../../src/rule_selection.rs)): nearest selector wins along each branch, `ignore` beats `select` across branches, item name is the implicit leaf of every branch.
    - Loud selector parser: global label lookup, synonym canonicalization, `select` $\cap$ `ignore` conflict detection, facet-label rejection (listing valid facet values), and case-insensitive optimal string alignment `closest_match` typo suggestions ([src/rule_declaration.rs](../../src/rule_declaration.rs)).
  - **Reusable taxonomy verification suite**:
    - Invariant checks from [src/rule_selection/taxonomy.rs](../../src/rule_selection/taxonomy.rs): every item has at least one topic, no item lists a topic together with its ancestor or twice, every tag matches at least one item, every topic has non-empty documentation, and all canonical labels/synonyms/item names are globally unique `kebab-case` and never collide with facet labels.
- **What stays in `omnicoach`**:
  - Concrete `omnicoach` facets (`Precision`, `Consensus`, `ImpactedQuality`, `Derived`) and `Topic::*` constants in [src/rule_declaration/taxonomy.rs](../../src/rule_declaration/taxonomy.rs).
  - `.omnicoach.toml` `RawConfig` deserialization, `per-file-ignores` glob compilation, and `[rules.<name>]` option override parsing.

### 3.4. Syntax & Fact Layer (`code_lint::ast`) and Shell Command Parser (`command_lint::command`)

- **`code_lint::ast` ([src/code_lint/ast.rs](../../src/code_lint/ast.rs))**:
  - After the [ADR 012](decisions/012_module_placement_by_abstraction.md) refactoring, `code_lint::ast` is a pure language-fact layer over `ruff_python_parser` and `ra_ap_syntax`.
  - However, because new code lint rules frequently add or refine fact extractors (`bindings`, `calls`, `returns`, `test_analysis`, `docstrings`, `annotations`), extracting all of `code_lint::ast` into a separate crate too early would force `pub` visibility on `pub(in crate::code_lint)` items and turn single-rule additions into cross-crate edits.
  - **Strategy**: Keep linter-specific AST extractors in `omnicoach` (or an internal workspace crate if parser compile times ever warrant isolation), and only extract the structural/module-graph subset if required by `omni-arch` / `omni-graph`.
- **`command_lint::command` ([src/command_lint/command.rs](../../src/command_lint/command.rs))**:
  - Self-contained shell AST (`brush-parser`) + byte-offset span tracker (`ParsedCommand`, `InterceptedCommand`, `ShellScope`, `CommandRedirect`, `EnvAssignment`) + declarative CLI flag/positional parser (`ProgramCliSchema`, `ParsedArgs`).
  - Once `SourceSpan` is split from `diagnostic`, `command_lint::command` has zero `omnicoach` dependencies and can be extracted whenever another command-guard or agent-hook project needs it.
