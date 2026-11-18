# Phase 4: Double-Blind Multi-Split Subagent Benchmark

## TL;DR

- **100% Architectural & Adversarial Boundary Alignment Across Splits**: Across `16` isolated subagent trials spanning a Python multi-dialect query planner (**Development**) and an unhinted zero-dependency Rust storage-manifest engine (**Holdout**), every trial passed all behavioral tests and `omnicoach code` checks with **`100%` negative-control boundary preservation**—preserving locked public SDK entrypoints, strict root-versus-nested wire schemas, and adapter-local SQL/RFC 6901 escaping without extending unreferenced legacy exports or duplicating foundation lookups across sibling adapters.
- **Structural Graph Upgrades Cut Token Cost and Lint Retries Without Prompt Changes**: Compared to **Baseline Graph + Baseline Skills**, **Upgraded Graph Only** reduced combined input tokens by **`-20%`** (**`-29%`** on Holdout), reduced Development output tokens by **`-14%`**, lowered mean wall-clock duration by **`-10%`**, and tripled first-pass `omnicoach code` cleanliness from **`25%` to `75%`** (`100%` on Holdout), while surfacing module constants (`PUBLIC_STAGE_CATALOG`, `MANIFEST_REGISTRY`) and `<- (0 in-scope callers)` markers immediately during initial graph inspection.
- **Domain-Agnostic Skill Guardrails Drove `slice` Auditing and Sum-Type Invariants**: Adding the ~110-token design and navigation guardrails raised pre-edit `omnicoach graph slice` caller auditing from **`0%` to `100%`**, raised Holdout sum-type modeling of mutually exclusive leaf/composite invariants (`enum GroupKind` with `NonZeroUsize`) from **`0%` to `100%`**, and—combined with the upgraded graph—reduced `read_file` calls by **`-17%`** over the baseline.
- **Live Trajectory Bug Caught and Fixed (`--depth public` Owner Contraction)**: Trajectory inspection of **Upgraded Graph + Skill Guardrails** revealed why its Development runs spent **`+16%`** more output tokens than the baseline (`+35%` over **Upgraded Graph Only**): `omnicoach graph --depth public` omitted cross-module `ReferenceEdge`s originating from private functions, falsely marking public functions called by private cross-module helpers with `<- (0 in-scope callers)` and triggering extra verification turns. Contracting private callers onto their `Roots(h)` public owners in [src/code_lint/graph/builder.rs](../../../src/code_lint/graph/builder.rs) and streamlining the skill prompt to a single batched `omnicoach graph slice <symbol>...` check eliminates both sources of friction.

---

## 1. Double-Blind Experimental Protocol

Following the evaluation design in [03_design.md](03_design.md), the benchmark crossed two `omnicoach` binary surfaces (**Baseline Graph** vs. **Upgraded Graph**) with two guidance configurations (**Baseline Skills** vs. **Skill Guardrails**) across two disjoint multi-layer feature tasks (`4` cohorts $\times$ `2` splits $\times$ `2` independent replicates = `16` isolated subagent trials):

1. **Disjoint Three-Way Split**:
   - **Quarantined Calibration Split**: `omnicoach`'s own recursive submodule DAG implementation (`42a41cfa`), used strictly for deterministic unit and conformance tests in [tests/architecture_conformance.rs](../../../tests/architecture_conformance.rs).
   - **Development Split (Python Multi-Dialect Query Planner)**: Eight-module repository with `.omnicoach.toml` `[architecture]` submodule DAGs (`core.{config, ir, plan}` $\leftarrow$ isolated siblings `postgres_dialect` and `clickhouse_dialect` $\leftarrow$ `facade`), tasking agents with supporting arbitrarily nested composite pipeline stages, sibling-scoped topological cycle validation, and `"/"`-delimited stage lookup.
   - **Unhinted Holdout Split (Rust Storage Manifest Engine)**: Zero-dependency Rust 2024 crate with `define_architecture!` and `modules: { ... }` (`ManifestCore::{schema, dag, store}` $\leftarrow$ isolated siblings `JsonCodec` and `TomlCodec` $\leftarrow$ `EngineApi`) and zero source comments hinting at dead exports or boundary traps, tasking agents with supporting nested compaction groups, RFC 6901 JSON Pointer lookup, and TOML dotted-path lookup.
2. **Hermetic Blinding & Balanced Positive/Negative Oracles**:
   - Each trial ran in an isolated `/tmp` directory with an anonymized `./bin/omnicoach` binary path, an empty `XDG_CONFIG_HOME`, no VCS history, and an identical neutral task prompt.
   - Each trial was evaluated against three **positive architectural traps** (unreferenced catalog/registry export pairs, recursive IR + single topological validator, and foundation-layer path lookup across sibling adapters) paired with three **adversarial negative controls** (locked zero-internal-caller SDK entrypoints `compile_pipeline_bundle` and `verify_archive_header`, strict root-only wire fields `connection_dsn` / `storage_dir` rejected on nested children, and adapter-local PostgreSQL/ClickHouse identifier quoting and RFC 6901 `~1`/`~0` unescaping).

---

## 2. Empirical Results Across Cohorts and Splits

**Context**: Evaluates architectural alignment, sum-type domain modeling, first-pass lint cleanliness, tool-call behavior, and exact API token telemetry extracted from `transcript.jsonl` across all `16` subagent trials (`2` replicates per cell).

| Cohort | Architectural & Boundary Alignment (Dev / Holdout) | Holdout Sum-Type Invariant (`enum` + `NonZeroUsize`) | Pre-Edit `graph slice` Adoption | First-Pass Lint Clean (Combined) | Mean `read_file` Calls | Mean Input Tokens (Dev / Holdout) | Mean Output Tokens (Dev / Holdout) | Mean Wall Time (Combined) |
| :--- | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: |
| **Baseline Graph + Baseline Skills** | `100%` / `100%` | `0%` | `0%` | `25%` | `19.2` | `124,437` / `200,162` | `61,198` / `68,744` | `1,147 s` |
| **Upgraded Graph Only** | `100%` / `100%` | `0%` | `0%` | **`75%`** | `19.0` | **`117,820` / `141,908`** | **`52,543`** / `70,225` | **`1,029 s`** |
| **Baseline Graph + Skill Guardrails** | `100%` / `100%` | **`100%`** | **`100%`** | `50%` | `17.5` | `177,825` / `143,096` | `60,478` / `66,728` | `1,076 s` |
| **Upgraded Graph + Skill Guardrails** | `100%` / `100%` | **`100%`** | **`100%`** | `25%` | **`16.0`** | `144,808` / `173,000` | `70,806` / **`65,586`** | `1,162 s` |

**Analysis & Interpretation**:

- **`Upgraded Graph Only` Improves Efficiency and First-Pass Lint Cleanliness Zero-Shot**: Without changing a single word of prompt or skill text, upgrading `omnicoach graph` reduced combined input tokens by **`-20%`** (`129,864` vs. `162,299`; **`-29%`** on Unhinted Holdout), cut Development output tokens by **`-14%`** (`52,543` vs. `61,198`), reduced combined wall-clock runtime by **`-10%`** (`1,029 s` vs. `1,147 s`), and raised first-pass `omnicoach code` cleanliness from **`25%` to `75%`** (`100%` on Holdout). In Baseline Graph runs (`Baseline Graph + Skill Guardrails` Holdout replicate 2), running `omnicoach graph slice MANIFEST_REGISTRY` returned `0 matches` because `const` items were unindexed; in Upgraded Graph runs, both `MANIFEST_REGISTRY` and `PUBLIC_STAGE_CATALOG` appeared immediately with `<- (0 in-scope callers)` alongside nested `### Submodule` containers and `[xmod,convergent]` Reflexion edges.
- **`Skill Guardrails` Eliminate Unchecked Export Assumptions and Flat Half-State Structs**: Across both splits, **Skill Guardrails** raised explicit `omnicoach graph slice` caller auditing from **`0%` to `100%`**, cut `read_file` invocations by **`-12%`** across both graph arms (**`-17%`** in **Upgraded Graph + Skill Guardrails** vs. the baseline, `19.2` $\to$ `16.0`), and preserved **`100%`** of adversarial negative controls (zero trials collapsed `PipelineFileConfig` into `StageConfig` or `RootManifestConfig` into `CompactionGroupConfig`, and zero trials deleted public SDK facade functions). On Unhinted Holdout, `100%` of **Skill Guardrails** trials modeled the leaf-versus-composite mutual exclusion on `CompactionGroupConfig` as an explicit Rust sum type (`GroupKind::Leaf { max_sstables: NonZeroUsize }` vs. `GroupKind::Composite { groups }`) compared to **`0%`** of baseline-skill trials (which left `max_sstables: usize` and `groups: Vec<...>` simultaneously representable on the struct).
- **Small-Repository Working-Memory Ceiling vs. Large-Repository Graph Reliance**: All `16` trials achieved `100%` architectural alignment across the six positive traps and negative controls. Trajectory logs explain why even **Baseline Graph + Baseline Skills** avoided extending dead exports here: an 8-file (`~8 KB`) fixture fits into a single parallel `read_file` turn, allowing the model to hold every file in working memory simultaneously. In 200-file repositories such as `omnicoach` (`~1.5 MB` across `209` `src/` files), reading the entire codebase is infeasible, making `omnicoach graph` constant indexing, `(0 in-scope callers)`, and submodule Reflexion edges essential to prevent the blind spots documented in [01_understand.md](01_understand.md).

---

## 3. Live Trajectory Discovery: Fixing `--depth public` Private-Caller Projection

**Context**: Compares `omnicoach graph` behavior across **Upgraded Graph Only** and **Upgraded Graph + Skill Guardrails** to explain why Development output tokens rose by `+35%` (`70,806` vs. `52,543`, or `+16%` over the `61,198` baseline).

| Mechanism Observed in `transcript.jsonl` | Effect in Prototype Benchmark | Root-Cause Resolution Adopted |
| :--- | :--- | :--- |
| **`PublicSurface` (`--depth public`) omitted cross-module `ReferenceEdge`s from private callers** | When `plan::_compile_stage_scope` (private, owned by public `plan::build_compiled_plan`) called public `ir::make_stage_node`, `omnicoach graph --depth public` dropped the cross-module edge and rendered `//   <- (0 in-scope callers)` on live `make_stage_node`, prompting extra `slice` verification steps. | Contract intra-module and cross-module `ReferenceEdge` sources through `decl.owners` (`Roots(h)`) in `ProjectDepth::PublicSurface` ([src/code_lint/graph/builder.rs](../../../src/code_lint/graph/builder.rs)), matching `project_public_surface_ordering_edges` so `make_stage_node` displays `<- plan::build_compiled_plan[xmod]`. |
| **Skill prompt mandated both `omnicoach graph <dir> --depth public` and `omnicoach graph slice <symbol>...`** | Doubled `omnicoach graph` CLI calls per trial (`2.5` $\to$ `5.2`) even when `omnicoach graph slice <symbol>...` already answered cross-module caller liveness in one bounded call. | Streamline `omnicoach` skill guidance to recommend batching candidate exports in `omnicoach graph slice <symbol>...` before extending them, reserving `omnicoach graph <dir>` for initial architecture discovery. |

**Analysis & Interpretation**: Contracting `PublicSurface` reference edges through `Roots(h)` aligns declaration reference edges with `PublicSurface`'s existing ordering-edge quotient model: a public symbol in module $B$ is unused across the public surface only when neither public items nor private helpers reachable from public entrypoints reference it.

---

## 4. Adopted Changes & Next Steps

1. **Adopted Repository Changes ([03_design.md](03_design.md))**:
   - Promoted recursive submodule containers, `const`/`static` indexing, `(0 in-scope callers)` rendering, and `PublicSurface` owner-contracted reference edges in `CodeLintGraph` ([src/code_lint/graph.rs](../../../src/code_lint/graph.rs)).
   - Promoted workspace-wide `pub` export liveness conformance in [tests/architecture_conformance.rs](../../../tests/architecture_conformance.rs) alongside the `CodeLintProject` and `CodeLintAst` deduplications.
2. **Skill Promotion & Future Large-Corpus Benchmark Design**:
   - Promote the streamlined caller-slice check and the foundation-ownership / recursive-vs-wire-schema design principles ([03_design.md](03_design.md) §2.3) to the `omnicoach`, `software-design`, and `phased-engineering-workflow` skills.
   - When evaluating repository-scale navigation tools on structural dead-code or layer-placement traps, embed tasks inside a $\ge 50$-file (`> 200 KB`) repository so single-turn full-repo `read_file` sweeps cannot substitute for graph queries.
   - Build AST evaluation assertions on structural properties (import targets, field sets, call graphs) rather than literal string snippets that interact with `[repeated-literal]` constant extraction or path/view newtype naming.

---

## 5. References

- [Software Reflexion Models: Bridging the Gap between Source and High-Level Models (Murphy, Notkin & Sullivan, ACM SIGSOFT FSE 1995)](https://doi.org/10.1145/222124.222136) — Formalizes `Convergent`, `Absent`, and `Divergent` architectural container edges.
- [MIPROv2 (Opsahl-Ong et al., EMNLP 2024)](https://arxiv.org/abs/2406.11695) & [TextGrad (Yuksekgonul et al., *Nature* 2025)](https://arxiv.org/abs/2406.07496) — Disjoint `Calibration` / `Development` / `Holdout` evaluation and over-prescriptive prompt pruning.
- [Specification Gaming: The Flip Side of AI Ingenuity (Krakovna et al., DeepMind)](https://deepmind.google/discover/blog/specification-gaming-the-flip-side-of-ai-ingenuity/) & [Adding Error Bars to Evals (Miller, 2024)](https://arxiv.org/abs/2411.00640) — Adversarial negative-control boundary evaluation and multi-replicate paired reporting.
