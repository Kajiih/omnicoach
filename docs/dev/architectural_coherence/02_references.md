# Phase 2: SOTA Survey & Full-Repository Empirical Audit

## TL;DR

- **Full-Repository Empirical Audit (`209` `src/` Rust files + `tests/`, `benches/`, `crates/`, `playground/`)**: Prototyping four candidate static AST/project lint rules across the entire `omnicoach` codebase proved that **untyped per-file heuristics for unread fields, downward layer misplacement, recursive child-struct subsets, and lexer-token Dice clones fail precision and soundness requirements**:
  1. **Downward Layer Misplacement (`0` own-component types, `1` lower-component's types)** produced **`59` false positives across `12` components** (`96.6%` false-positive rate)—flagging `16` helpers in `CodeLintRules`, `7` in `RuleCatalog`, and `6` in `CodeLintPolicy` in direct conflict with [ADR 012](../decisions/012_module_placement_by_abstraction.md) P7 (*facts in `ast`, policy in `policy` or the rule*), plus `50%` false positives inside `project/python/architecture.rs` (`routes_to_python_component`, `python_sibling_violation`).
  2. **Write-Only Struct Fields Without Type Inference** had **`0%` recall** on `SubmoduleSpec::depends_on` under untyped `.field` matching (masked by `entry.depends_on` and `component.depends_on` in the same file) and false-positived on unannotated closure parameters, `thiserror` format fields, RAII guards, and Python `NamedTuple`/`TypedDict`/`Pydantic` models under receiver-typed matching.
  3. **Recursive Child Struct Subsets** scored **`33%` precision pre-cleanup** and **`0%` precision on `main`** (`SubmoduleSpec` $\subset$ `ComponentSpec`, `ArchitectureSubmoduleConfig` $\subset$ `ArchitectureComponentConfig`), pressuring agents to collapse distinct domain types and make invalid states representable.
  4. **4-Gram Lexer-Token Dice Shingling** could not separate 15-line `validate_submodule_dag` (`Dice = 0.798`) from 13-line symmetrical helpers (`token_before_double_colon ~ token_after_double_colon`, `Dice = 1.000`, `94` tokens) and violates `rule_test!`'s `{code}\n{code}` repetition invariant.
- **High-Signal Surfaces Confirmed**:
  1. **`omnicoach graph` (`CodeLintGraph` + `ProjectFacts`)** builds the cross-module symbol graph during structural navigation (`0 ms` hook overhead), making it the natural surface for recursive submodule Reflexion edges, `const`/`static` indexing, and explicit `(0 in-scope callers)` rendering on `pub` declarations.
  2. **Workspace Export Liveness Across All Cargo Members** (`src/` production code and rustdoc fences, `tests/`, `benches/`, `crates/`, `playground/`) cleanly distinguishes workspace-member exports (`format_plain` and `explain_with_config`, called solely from `playground/src/lib.rs`) from genuinely unreferenced `pub` items (`ComponentDefinition`, `SubmoduleDefinition`, `ARCHITECTURE_GRAPH`, and `SourceSpan::from_range` in [src/diagnostic.rs](../../../src/diagnostic.rs)).

---

## 1. SOTA & Codebase Analysis by Problem Area

### 1.1 Workspace Export Liveness & Compiler Visibility Boundaries

**Context**: Compares static mechanisms for catching cross-module dead exports (`ComponentDefinition`, `ARCHITECTURE_GRAPH`, `SourceSpan::from_range`) and unread struct fields (`SubmoduleSpec::depends_on`) across Rust and Python workspaces.

| Approach | Mechanism | Empirical Finding on `omnicoach` |
| :--- | :--- | :--- |
| **`rustc` `dead_code` + Narrow Visibility** | Compiler reachability from crate roots; checks private and `pub(crate)`/`pub(in ...)` items and fields, ignoring `#[derive(Debug, Clone, Copy)]`. | Sound and zero-cost when internal types use restricted visibility (`pub(crate)` / `pub(in ...)`) and omit unneeded `#[derive(PartialEq, Eq)]` (which synthesizes `self.field == other.field`). Cannot flag `pub` exports reachable from `src/lib.rs`. |
| **`rustc --warn unreachable-pub`** | Flags `pub` items not reachable via `pub mod` / `pub use` from `src/lib.rs`. | Emitted **`88.8 KB` (~2,500 lines) of warnings** across `src/`, conflicted directly with `clippy::redundant_pub_crate` on private child modules (`mod foundation;`), and missed `src/architecture.rs` (`pub mod architecture;` in `src/lib.rs`). |
| **Whole-Workspace Symbol Reachability (`knip`, SCIP `warnalyzer`)** | Marks exports unused only when unreferenced across all source, test, benchmark, doctest, and workspace-member targets outside explicit SDK entrypoints. | Caught every dead export (`ComponentDefinition`, `SubmoduleDefinition`, `ARCHITECTURE_GRAPH`, plus `SourceSpan::from_range` in `src/diagnostic.rs`) with zero false positives across `playground/`. Cannot run inside single-file `ProjectCodeRule` without exceeding `omnicoach hook`'s `30 ms` latency budget (`ProjectFacts` takes `~460 ms` workspace-wide) or breaking `unused-suppression` in [src/code_lint/runner.rs](../../../src/code_lint/runner.rs). |

**Analysis & Interpretation**: Whole-workspace export liveness belongs in the two surfaces that already operate at multi-file or workspace scope: `omnicoach graph` (surfacing `(0 in-scope callers)` on `pub` declarations and indexing `const`/`static` items) and [tests/architecture_conformance.rs](../../../tests/architecture_conformance.rs) (`test_workspace_public_exports_have_live_consumers`). Untyped AST field-read rules are rejected because `omnicoach` intentionally operates on syntax trees without `ra_ap_hir` or Python type inference.

---

### 1.2 Layer Misplacement (Feature Envy) vs. `ADR 012` P7 & Submodule Graph Visibility

**Context**: Tests whether a static rule flagging functions in module $M$ that reference zero types in $M$'s component and at least one type in a single lower dependency $D$ generalizes across all `209` `src/` files in `omnicoach`.

| Evaluated Scope | Flagged Functions | True Misplacements | False Positives | Primary False-Positive Mechanism |
| :--- | ---: | ---: | ---: | :--- |
| **`CodeLintProject` (`project/{python,rust}/`, 11 files)** | `4` | `2` (`check_rust_cross_component_edge`, `is_rust_path_allowed_by_exports`) | `2` (`50.0%`) | `routes_to_python_component` and `python_sibling_violation` encode Python `"__init__"` and `".**"` semantics over `&str` primitives; moving them into `foundation` would violate cross-language purity. |
| **All 25 Components (`src/`, 209 files)** | `59` | `2` | **`57` (`96.6%`)** | Flags policy, rule, and renderer layers whose architectural purpose is to compute policy or text over lower AST/declaration types without defining local types: `CodeLintRules` (`16`), `CodeLintAst` (`12`), `RuleCatalog` (`7`), `CodeLintPolicy` (`6`), `CodeLintSemantic` (`5`), `TestUtils` (`5`), `CommandLintRules` (`2`). |

**Analysis & Interpretation**: A syntactic "zero local types, one lower dependency's types" lint rule contradicts [ADR 012](../decisions/012_module_placement_by_abstraction.md) P7 (*facts in `ast`, policy in `policy` or the rule*) and functional pipeline stages (`render => [model]`). By contrast, inspecting [foundation/facts.rs](../../../src/code_lint/project/foundation/facts.rs) and [graph/builder.rs](../../../src/code_lint/graph/builder.rs) revealed why `omnicoach graph` failed to guide placement inside `CodeLintProject`: `ProjectFacts::build` dropped `ComponentSpec::submodules` and hardcoded all intra-component edges to `ReflexionStatus::Inferred`. Surfacing recursive `Submodule` containers and `Convergent`/`Absent`/`Divergent` Reflexion statuses in `omnicoach graph` exposes `foundation` $\leftarrow$ `{python, rust}` layering during navigation with zero false positives.

---

### 1.3 Recursive Child Structs & Intra-Module Token-Dice Clone Thresholds

**Context**: Evaluates precision, recall, and destructive-compliance risk of structural child-struct subset checks and 4-gram token-skeleton Dice similarity across `209` `src/` files.

| Candidate Check | Recall on Pre-Cleanup Specimen | Precision on Clean `main` | Structural Failure Mode |
| :--- | :---: | :---: | :--- |
| **Recursive Child Struct Subset (`Child` $\subset$ `Parent`)** | `100%` (`DefinedSubmoduleEntry`) | **`0%`** (`SubmoduleSpec`, `ArchitectureSubmoduleConfig`) | Induces **destructive compliance**: pressures agents to merge child structs into parent structs (`ComponentSpec`) that carry parent-only invariants (`roots`, `owns_external`, `public_exports`), making invalid states representable. |
| **4-Gram Token-Skeleton Dice (`>= 0.80`, `>= 45` tokens)** | **`50%`** — misses `validate_submodule_dag` (`Dice = 0.798`) | Flags 13-line symmetrical helpers (`token_before_double_colon ~ token_after_double_colon`, `Dice = 1.000`, `94` tokens) | Punctuation inflates token counts (`45` tokens $\approx 7$ lines); no threshold separates `validate_submodule_dag` (`0.798`) from directional `before`/`after` helpers (`1.000`); breaks `rule_test!` `{code}\n{code}` repetition; trivially gamed by inserting a local binding. |

**Analysis & Interpretation**: Both syntactic heuristics fail `omnicoach`'s precision and anti-gaming bar.    2. Concrete codebase duplications uncovered during the audit (`check_rust_cross_component_edge` / `append_declared_submodule_isolation_rules` in [rust/architecture.rs](../../../src/code_lint/project/rust/architecture.rs) and `find_nested_functions` / `find_nested_classes` in [ast/python/tree.rs](../../../src/code_lint/ast/python/tree.rs)) are unified directly in code, while recursive vs. schema-distinct modeling is addressed through domain-agnostic design guidance ([03_design.md](03_design.md)).

---

## 2. References

- [Knip: Entry Files & Workspace Analysis](https://knip.dev/overview/entry-files) — Multi-target workspace entry-point modeling (`src`, tests, benchmarks, workspace members, and explicit public exports) for closed-world dead-export analysis.
- [Software Reflexion Models: Bridging the Gap between Source and High-Level Models (Murphy, Notkin & Sullivan, ACM SIGSOFT FSE 1995)](https://doi.org/10.1145/222124.222136) — Formalizes `Convergent`, `Divergent`, and `Absent` architectural edges used in `CodeLintGraph` and generalized here to recursive intra-component submodule DAGs.
- [Object-Oriented Metrics in Practice (Lanza & Marinescu, Springer 2006)](https://doi.org/10.1007/3-540-39538-5) & [Identification of Move Method Refactoring Opportunities (Tsantalis & Chatzigeorgiou, IEEE TSE 2009)](https://doi.org/10.1109/TSE.2009.1) — Establishes foreign-data locality predicates (`ATFD`, `LAA`, `FDP = 1`) and explains why untyped functional and policy layers require architectural role separation (`ADR 012` P7).
- [Clone Detection Using Abstract Syntax Trees (Baxter et al., ICSM 1998)](https://doi.org/10.1109/ICSM.1998.738528) & [Comparison and Evaluation of Code Clone Detection Techniques and Tools (Roy, Cordy & Koschke, *Science of Computer Programming* 2009)](https://doi.org/10.1016/j.scico.2009.02.007) — Taxonomy of Type-1–Type-3 clones and AST subtree vs. token-shingle trade-offs on compact symmetrical functions.
- [MIPROv2 (Opsahl-Ong et al., EMNLP 2024)](https://arxiv.org/abs/2406.11695), [TextGrad (Yuksekgonul et al., *Nature* 2025)](https://arxiv.org/abs/2406.07496), [Specification Gaming (Krakovna et al., DeepMind)](https://deepmind.google/discover/blog/specification-gaming-the-flip-side-of-ai-ingenuity/), & [Adding Error Bars to Evals (Miller, 2024)](https://arxiv.org/abs/2411.00640) — Grounds the three-way disjoint split, domain-agnostic skill prompts, adversarial negative-control boundary traps, and multi-replicate paired evaluation.
