# Omni Toolkit Roadmap

This document serves as the single source of truth for architectural milestones, performance investigations, and planned ecosystem integrations for Omni.

Items here represent design areas and technical directions to evaluate rather than fixed implementation mandates.

---

## Random
- Review the names of the rules, of the configuration, etc, to make them totally aligned on what the are, explicit and self explanatory, and coherent together.
- Also review the violation message, so they correctly explain what is the issue and why it is one rather than just explaining what the code does, and that the suggestion correctly point to correct solutions, so the user (or agent) can fix it autonomously. They should push to a single direction, which is the pit of success, even if it seems pedantic. Use `ruff` documentation as a reference and improve on it.
  - Write a **Violation Message Style Guide** and review all violation messages (`summary`, `rationale`, `suggestion`) so they concisely state the issue (`summary`), explain why it is harmful rather than just restating what the code does (`rationale`), and point to a single canonical "pit of success" solution so a user or agent can fix it autonomously (`suggestion`). Use `ruff` documentation as a reference and improve on it.
  - Reviewed violation message that we can use as reference
    - [prefer_dedent_for_multiline_strings.rs](/usr/local/google/home/paquerot/Documents/dev_projects/custom_lints/src/code_lint/rules/prefer_dedent_for_multiline_strings.rs)
- Review our architecure, component, abstraction, modules, etc names as well to align and have the explicit and self explanatory.
- **Deriving Module Organization from the Architectural Graph**:
  - *Context*: Today, `tests/architecture.rs` discovers module-to-component mappings from colocated `architecture_component!(...)` declarations in `src/` and checks them against `ARCHITECTURE_GRAPH`. Some components span multiple sibling modules (`FoundationPrimitives` $\to$ `diagnostic`, `diff`; `CodeSemanticEngines` $\to$ `bindings`, `calls`, `comments`), while `pub mod` trees in `src/lib.rs`, `src/code_lint.rs`, and `src/command_lint.rs` are maintained separately.
  - *Investigation*:
    - Evaluate restructuring `src/` into a 1-to-1 isomorphism between `ArchitectureComponent` variants and module namespaces (e.g., grouping `diagnostic` and `diff` under `src/primitives/`, and `bindings`, `calls`, `comments` under `src/code_lint/semantics/`).
    - Investigate whether the `pub mod` / `pub(crate) mod` declarations in `src/` can be generated directly from the architectural graph macro so module organization and visibility boundaries derive from the graph definition.
    - Find a representation where not every file must declare `architecture_component!(...)` (currently required per file, see `decisions/006_architectural_dag_and_conformance.md` §2.2). If module namespaces map 1-to-1 to components, or the module tree is generated from the graph, a file's component follows from its path and per-file declarations become redundant.
- **Subtree-Level Isolation for `(no_internal_dependencies)`**:
  - *Current*: `tests/architecture.rs` isolates the leaf **files** of a component from each other. Every rule, semantic engine, and binary is a single file today, so leaves and units coincide.
  - *Target*: Isolate the direct child **subtrees** of each component root instead, so a unit split into `foo.rs` + `foo/sub.rs` is treated as one unit (internal imports allowed, imports from sibling units forbidden).
  - *Trigger*: The first multi-file rule, semantic engine, or binary.
- **Architecture Conformance Watch List** (hypothetical gaps, no occurrence today):
  - These gaps are recorded because they would **bypass the DAG checks silently** rather than fail loudly. Neither pattern exists in `src/` today.
  - *Inline `super::` paths*: `test_no_relative_imports_in_production_code` only inspects `use` declarations, and `rust_arkitect` does not resolve inline `super::sibling::item()` expression or type paths, so such a dependency escapes both checks. *Trigger*: The first inline `super::` path in production code, or any evidence of it in review.
  - *Code in root barrel files*: `src/lib.rs`, `src/code_lint.rs`, and `src/command_lint.rs` declare no `ArchitectureComponent`, so no dependency rule has them as subject. They contain only `mod` declarations today; a function or `use` added there would be unconstrained. A candidate check is asserting that these files contain only `mod` items (plus `#[macro_export]` macros in `src/lib.rs`). *Trigger*: The first non-`mod` item added to a root barrel file.
## Rule Engine & Declarative Rules

- **Unified Single-Pass AST Visitor Dispatch**:
  - *Context & Problem*: Today, each registered rule implements `check_file` independently and executes its own AST search or traversal over the file. At 16 rules on ~8k lines, rule passes take ~180ms total (~11ms/rule). However, this scales linearly as $O(\text{files} \times \text{rules})$: at 100+ rules, traversing the syntax tree 100 times per file becomes a multi-second bottleneck.
  - *Investigation & Design Questions*:
    - Investigate how high-throughput linters dispatch rules. (e.g. Ruff's `Checker` uses a single AST walk with match arms and $O(1)$ bitset checks; Biome groups subscriptions by `SyntaxKind`; Clippy fuses passes into combined callbacks).
    - Can we establish a rule interest declaration (e.g. target node kinds) early in the trait lifecycle without breaking existing rule independence?
    - How do we handle rules that need multi-stage context (like `no_env_in_functions` traversing upward or `max_test_assertions` counting inner blocks) within a unified traversal?
  - *Trigger*: When the code rule registry approaches ~30–40 rules, or when rule evaluation time exceeds parse time.
- **Dynamic Selector Deserialization**:
  - *Current*: `Selector` deserialization directly scans static `CODE_RULES` and `COMMAND_RULES` registries.
  - *Target*: Decouple selector resolution from static arrays to allow dynamically registered and external declarative rules loaded from configuration files (`.omnilint.toml`).
- **Rule Naming Canonicalization (`heck`)**:
  - *Target*: Support case-insensitive and format-tolerant rule selection (matching `SingleLetterVariableName`, `single-letter-variable-name`, and `single_letter_variable_name` interchangeably).
  - *Trigger*: When adding declarative AST rule files or multi-rule alias configurations.
- **Rule Autofix Engine (`diffy` / `similar`)**:
  - *Target*: Extend `CodeRule` and `CommandRule` with optional auto-fix transformations. Support `--fix` and `--fix --dry-run` with in-memory unified diff previews before writing changes to disk.
  - *Trigger*: When implementing the first batch of auto-fixable rules (e.g., replacing `logging.error` with `logging.exception`).
- **Decorator & Attribute Span Awareness for `disable-next-line`**:
  - *Current*: `disable-next-line` matches diagnostics anchored on `comment_line + 1`, which misses function-level diagnostics anchored on `def` / `fn` when `@decorator` or `#[attribute]` lines appear in between.
  - *Target*: Propagate decorated/attributed span boundaries to the suppression resolver so `disable-next-line` placed above a decorator/attribute block suppresses diagnostics on the decorated declaration.
- **Generic Container Base-Type Matching (`no-identical-positional-types`)**:
  - *Current*: Positional parameter types are compared by exact formatted annotation string (`dict[str, int]` != `dict[str, float]`).
  - *Target*: Optionally normalize or group generic collection/mapping containers (`dict[...]`, `Mapping[...]`, `list[...]`, `Sequence[...]`) so multiple positional mappings or sequences are flagged even when their inner type arguments differ.
- **Escaping Nested Scopes (`no-env-in-functions`)**:
  - *Current*: The boundary exemption (`main`, `from_env`, ...) is inherited by every scope declared inside it, which is correct for nested functions and closures but also exempts a class declared inside a boundary whose methods later escape (returned, registered as a callback).
  - *Target*: Treat a `class` / `impl` declared inside a boundary as a barrier that resets the exemption, once a real-world occurrence justifies the added language-specific complexity.
- **Framework-Level Multiline Explanation Awareness (`EnforcementMode::RequireExplanation`)**:
  - *Context & Problem*: Centralized explanation checking in `lint_file` currently evaluates `index.has_adjacent_explanation(diagnostic.location.line)`. Because `diagnostic.location.line` points only to the start line of a matched call, multiline parenthesized expressions (or calls with trailing inline comments on closing parenthesis `)`) trigger false positives unless rules manually reimplement AST-aware comment scanning.
  - *Target*: Move AST node/statement header span awareness into `CommentIndex::has_explanation_for_node` in the framework layer so multiline commented expressions are handled uniformly across all rules without per-rule comment inspection logic.

---

## 2. Performance & Concurrency Architecture

- **Concurrent File-Level Analysis**:
  - *Context & Problem*: Analysis is currently single-threaded. On multi-core development workstations, scanning 31 files (~8,200 lines) sequentially takes ~400ms in release mode. The work is embarrassingly parallel across files.
  - *Investigation & Design Questions*:
    - Evaluate using `ignore::WalkParallel` (already present in the dependency tree) vs. collecting files and scheduling via `rayon`.
    - Ensure output determinism: `print_diagnostics` already groups findings into a `BTreeMap` by location context and sorts by byte span for text output, but JSON formatting must also preserve deterministic ordering across runs.
    - Measure overhead on small repositories to ensure thread-pool initialization does not degrade latency for micro-runs or pre-commit hooks.
  - *Target*: Bring full-workspace cold-cache execution down to ~50–80ms on multi-core systems.
- **Parse & Pipeline Floor Profiling**:
  - *Context*: Disabling all rules via tag exclusion shows that the shared per-file pipeline (walking files, reading from disk, tree-sitter parsing via `ast-grep`, and comment suppression scanning) accounts for ~228ms (56% of total runtime on ~8k lines).
  - *Investigation*:
    - Filter files before I/O: currently `lint_single_file` reads files to a string before checking `detect_language` (reading snapshots and ignored extensions unnecessarily).
    - Investigate the parse cost breakdown: how much of the ~228ms is Tree-sitter parser initialization / tree building vs. `SuppressionTracker::from_file` traversing comment nodes?
    - Determine whether suppression comments can be scanned more cheaply or parsed concurrently with AST visitation.
- **Subprocess Batching & Caching (`EnvContext`)**:
  - *Current*: Command rules spawn individual `jj` or `git` CLI calls per evaluation.
  - *Target*: Introduce a shared `EnvContext` struct that pre-fetches and caches repository state (e.g., batching queries into a single `jj log --json` or `git status` invocation) to ensure sub-10ms execution across multiple rules.
- **VCS Error Propagation**:
  - *Current*: VCS client query errors in command rules are swallowed to avoid blocking users on query failures.
  - *Target*: Propagate structured errors or display user warnings when the underlying VCS client fails unexpectedly, distinguishing clean working copies from failed CLI calls.
- **Architecture Test Parse Caching (`tests/architecture.rs`)**:
  - *Context*: `tests/architecture.rs` is the slowest test binary (~3.9s) because each test re-reads and re-parses every file in `src/`; only `discover_module_components()` is cached via `OnceLock`.
  - *Constraint*: `rust_arkitect::RustFile` wraps a `syn::File`, which is `!Sync` and cannot be stored in a `static OnceLock`. Caching the source text or test-stripped source is possible; caching the parsed tree is not without re-parsing per thread.
  - *Trigger*: When `tests/architecture.rs` becomes the dominant cost of `cargo test`.

---

## 3. Performance Measurement, Tracing & Tooling

- **Execution Timing & Observability (`--timings`)**:
  - *Context*: Understanding which rules or pipeline stages dominate execution on a user's machine is essential for performance triage.
  - *Investigation & Design Questions*:
    - Compare a lightweight, zero-dependency `--timings` flag (using `std::time::Instant` around rule executions to produce a sorted table, following oxlint's `--debug timings` or ESLint's `TIMING=1`) against heavyweight runtime tracing.
    - SOTA review shows that full `tracing-subscriber` pipelines pull in substantial dependencies (`sharded-slab`, `regex-automata`, etc.) and are best suited for server/LSP contexts rather than fast batch CLI invocations.
    - Explore what level of timing granularity is useful (per-rule vs. pipeline phase) without penalizing normal runs.
- **Benchmarking & Regression Guard Strategy**:
  - *Context*: Simple shell-level timing (`date +%s%N`) exhibits $\pm 17\%$ noise on sub-second runs, causing false regressions. At the same time, threshold-based wall-clock assertions on shared CI runners (e.g. GitHub Actions) suffer from high variance (15–30%) and lead to flaky CI.
  - *Investigation & Design Questions*:
    - Evaluate local developer micro-benchmarking harnesses: `divan` (lightweight, zero additional heavy dependencies) vs. `criterion` (industry standard, but heavier dependency footprint).
    - Explore CI regression gating models: evaluate simulated instruction-count tracking (e.g., CodSpeed or Valgrind cachegrind) vs. keeping performance gates advisory/local to prevent CI alert fatigue.
    - Define a standardized macro-benchmark corpus (e.g., fixed snapshot of source files) executed via `hyperfine` for reproducible end-to-end timing.
- **Profiling Workflow & Cargo Configuration**:
  - *Target*: Document standard profiling recipes for Linux (`samply`, `perf`) and macOS (`cargo-instruments`). Establish a dedicated `[profile.profiling]` Cargo profile (`inherits = "release"`, `debug = "line-tables-only"`, `strip = "none"`) that provides symbolicated stack traces without bloating production binaries.

---

## 4. Reporting & Diagnostics

- **Feature-Gated Rich Terminal Diagnostics (`miette`)**:
  - *Current*: Fast, zero-dependency printer in `src/diagnostic.rs` outputting standard compiler-style format (`path:line:col: [CODE] message`) and JSON.
  - *Target*: Add an optional Cargo feature (`features = ["miette"]`) that enables rich, syntax-highlighted source snippets with colored squiggly underlines and clickable rule documentation URLs, while keeping the default pre-commit hook binary lightweight and fast.
- **Polished Diagnostic Summaries (`pluralizer` / Native Helper)**:
  - *Target*: Clean grammatical inflection ("1 violation" vs "3 violations") in terminal summary footers, JSON reports, and future JUnit/SARIF export formats.
- **Single-Pass Placeholder Tokenizer / Interpolator (`LanguageText`)**:
  - *Current*: `LanguageText::interpolate` performs sequential string replacement (`result.replace("{key}", val)`).
  - *Target*: Replace sequential search-and-replace with a single-pass scanner/tokenizer (or regex) to avoid potential secondary replacement issues when interpolated values contain curly braces (`{}`) matching other parameter names.

---

## 5. Tags, Discovery & Documentation

Prior analysis, candidate designs, and open questions: [docs/dev/tag_system_analysis.md](docs/dev/tag_system_analysis.md). Nothing in that document is decided; it is the starting point for the design work below, not a specification of it.

- **Tag Taxonomy Design**:
  - *Current*: The tag set grew ad-hoc, and at 17 rules there is not enough evidence to judge it. `Cli` has zero members, so `select = ["cli"]` silently matches nothing; `Workflow`, `Vcs`, and `JJ` resolve to the same singleton set; `Safety` covers one typing rule while documenting operational command restrictions.
  - *Target*: Decide what a tag is for (selector, documentation, or both), then design the taxonomy that follows: whether axes are the right organising idea, what admission criteria a tag must meet, and how dispositions are defined. Apply the outcome to the concrete tag set and record it as an ADR in `decisions/`.
  - *Trigger*: The first configuration need a tag cannot express, or `CODE_RULES` exceeding ~40 rules, whichever comes first.
- **Tag Hierarchy Design**:
  - *Current*: `has_tag` is flat set membership, so a rule must declare every ancestor tag it wants to be selectable by, and a specific tag can silently duplicate a general one.
  - *Target*: Decide whether tags should nest at all, and if so choose a mechanism and the rule for when a parent link is legitimate. A `Tag::parent` function with a chain-walking `has_tag` is one candidate; declaring the parent on each rule plus a test assertion is another.
  - *Trigger*: The first genuine parent/child pair with independent members — for example a Git rule joining the jj rule under `Vcs`.
- **Rule Discovery**:
  - *Current*: No way to ask the binary what rules exist. `Tag::description` and `Tag::as_str` are unused, and `test_tag_description` asserts a doc comment against a copy of itself.
  - *Target*: Design how users discover rules — plausibly a `rules` subcommand listing names, languages, tags, target scope, and configuration keys. Settling this also settles whether tags are documentation, which the taxonomy design depends on.
  - *Trigger*: The README rule list exceeding ~30 entries, or the first user outside this repository.
- **Generated Rule Documentation**:
  - *Current*: `README.md` hand-maintains a flat list of every rule, with no tags or languages, and nothing detects drift from `CODE_RULES`.
  - *Target*: Decide whether rule documentation should be generated from the registry, and if so how drift is prevented.
  - *Trigger*: With rule discovery, which would supply the rendering.
