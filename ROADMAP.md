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
- **Rule Definition Layering & Declarative Cleanliness (Separation of Policy vs. AST Plumbing)**:
  - *Context & Problem*: Rule implementations currently risk leaking low-level Tree-sitter tree navigation (`node.ancestors()`, `child(0)`), AST line arithmetic (`start_pos().line() + 1`), and comment index instantiation (`CommentIndex::from_ast`) directly into rule definitions. Rule files should be the highest-level layer in the codebase, expressing declarative domain policy rather than compiler plumbing, while avoiding speculative over-engineering.
  - *Target Architecture*:
    - **Rule Definitions (`src/code_lint/rules/*.rs`)**: Express declarative policy only (`FilterListDefaults`, target scopes, violation templates, and high-level domain filters). Standard call-banning rules should be 1-line delegations to `self.check_banned_calls(...)`.
    - **Language AST Helpers (`ast_python.rs`, `ast_rust.rs`)**: Encapsulate grammar and Tree-sitter traversal (e.g. `ast_python::is_with_context_manager(node)`), keeping direct `.kind()` and `.children()` checks out of rule files.
    - **Framework Runner (`src/code_lint.rs`, `calls.rs`)**: Owns AST traversal orchestration, call matching, comment index lifecycle, and `EnforcementMode` (`Ban` vs `RequireExplanation`).
  - *Trigger*: Audit and standardize rules (`no_uncommented_suppress`, `no_typing_cast`, `no_mock_assertions`) immediately after completing the current commit chain review.
- **Framework-Level Multiline Explanation Awareness (`EnforcementMode::RequireExplanation`)**:
  - *Context & Problem*: Centralized explanation checking in `lint_file` currently evaluates `index.has_adjacent_explanation(diagnostic.location.line)`. Because `diagnostic.location.line` points only to the start line of a matched call, multiline parenthesized expressions (or calls with trailing inline comments on closing parenthesis `)`) trigger false positives unless rules manually reimplement AST-aware comment scanning.
  - *Target*: Move AST node/statement header span awareness into `CommentIndex::has_explanation_for_node` in the framework layer so multiline commented expressions are handled uniformly across all rules without per-rule comment inspection logic.

---

## Architecture: Abstraction Layers & Dependency Direction

- **Explicit, Enforced Abstraction Levels**:
  - *Context & Problem*: The intended layering is real but implicit, and two places contradict it. (1) `code_lint.rs` re-exports `AstNode` and `SourceDoc` from `core.rs`, so `ast_python.rs` and `ast_rust.rs` import `crate::code_lint::AstNode` — an *upward* import for a symbol that lives *below* them. The dependency is harmless at runtime but it inverts the module graph and makes the language modules look coupled to the cross-language layer they must stay independent of. (2) `code_lint.rs` straddles two levels: it defines the `CodeRule` contract that `code_lint/rules/*.rs` depend on, *and* it is the registry/runner that invokes those same rules. Rust's parent/child visibility lets this compile, but `code_lint.rs` ends up both below and above `rules/`, sandwiching them. Nothing currently detects either inversion, so both can silently spread.
  - *Intended Layering* (low → high, each level may depend only on levels strictly below it):
    | Level | Contents |
    | :--- | :--- |
    | L0 | `ast_grep_core`, `ast_grep_language` (external) |
    | L1 | `core.rs` (`AstNode`, `SourceDoc`, `Rule`, `Config`, `FilterListDefaults`), `diagnostic.rs` |
    | L2 | `code_lint/ast_python.rs`, `code_lint/ast_rust.rs` — grammar vocabulary; parallel siblings that must never depend on each other |
    | L3 | `code_lint/{bindings,calls,comments,statements,suppression}.rs` — cross-language semantic engines |
    | L4 | `CodeRule` trait, `RuleTarget`, `detect_language`, `collect_test_functions` |
    | L5 | `code_lint/rules/*.rs` — declarative rule policy |
    | L6 | Lint orchestration: `lint_file`, `run_code_lint`, `collect_targets` |
    | L7 | `cli` / `main.rs` |

    -> this is the current target and can evolve if we find a better structure.
  - *Target*:
    - Drop the `pub use crate::core::{AstNode, SourceDoc}` re-export (or keep it strictly as an external-facing API alias) so no in-crate module reaches upward for an L1 symbol; `ast_*.rs` then hold *zero* references to `crate::code_lint`.
    - Split `code_lint.rs` along the L4/L6 seam — plausibly `code_lint/rule.rs` (contract) and `code_lint/runner.rs` (registry and orchestration) — so `rules/` sits above the contract and below the runner instead of inside a parent that is both.
    - Declare the level of each module explicitly (module doc header, a manifest, or both) so the intended graph is stated rather than inferred.
  - *Enforcement*: Architecture tests in CI asserting (a) no module imports from a level at or above its own, (b) no import cycles between modules, (c) `ast_python.rs` and `ast_rust.rs` import neither `crate::code_lint` nor each other, and (d) no Tree-sitter grammar kind literals appear outside `ast_*.rs`. Evaluate whether to hand-roll these as source-scanning tests, express them as dogfooded Omni rules, or adopt an existing crate before writing bespoke checks.
  - *Note*: These four items are one piece of work — the re-export fix, the `code_lint.rs` split, the cycle ban, and making the levels explicit all describe the same graph, and enforcing it is what keeps any of them from regressing.
- **Standardizing Per-Language Dispatch**:
  - *Context & Problem*: Nine call sites (5 in `bindings.rs`, 2 in `calls.rs`, 1 each in `comments.rs`, `statements.rs`, and `code_lint.rs`) repeat the identical shape `match lang { Python => ast_python::f(..), Rust => ast_rust::f(..), _ => fallback }`. The pattern silently depends on an unwritten convention: both `ast_*` modules must expose identically named functions with identical signatures. Nothing enforces that convention, and adding a third language means finding and editing all nine sites.
  - *Options to Evaluate*:
    - A declarative `macro_rules!` (e.g. `dispatch_lang!(lang, f(args), fallback)`) — collapses each site to one line, makes the naming symmetry compiler-checked, and reduces a third language to a single edit; costs macro opacity and weaker IDE navigation.
    - Per-module private helper functions — plainly readable, no magic, but leaves the convention unwritten and the nine sites intact.
    - A closed internal `enum Language { Python, Rust }` converted once from `SupportLang`, removing every `_ => fallback` arm and making exhaustiveness a compile error when a language is added.
    - A `LanguageSyntax` trait — rejected for now as a god-trait that bundles unrelated concerns (calls, comments, bindings, statements) and violates interface segregation at two languages.
    - Survey how comparable multi-grammar linters solve this before committing to a bespoke mechanism.
  - *Trigger*: Adding a third language, or the dispatch site count exceeding ~12.
- **Universal Punctuation & Trivia Token Handling in Shared Engines**:
  - *Context & Problem*: In `src/code_lint/calls.rs`, `call_argument_nodes` filters child nodes with `!matches!(child.kind().as_ref(), "(" | ")" | ",")`. While parentheses and commas happen to share identical anonymous token kinds across both Python and Rust Tree-sitter grammars, they are still Tree-sitter grammar kind string literals residing in an L3 cross-language engine module. An architecture test strictly banning grammar kind literals outside `ast_*.rs` will flag these.
  - *Investigation & Design Questions*:
    - Should syntactic punctuation/trivia tokens common to all supported C-style/ALGOL-derived grammars be granted an explicit exemption (e.g. via a centralized `is_syntax_punctuation` or `is_named` check), or does any kind literal in an L3 module represent an abstraction leak?
    - Can argument node extraction be delegated down to `ast_python` / `ast_rust` entirely (e.g. `ast_*.rs` exposing `call_argument_nodes(call_node)`), ensuring L3 operates solely on semantic `AstNode` lists without inspecting token stream trivia?
    - How do other AST frameworks (e.g. ast-grep's `is_named()` or node child filtering) differentiate semantic arguments from delimiter tokens?
  - *Trigger*: When implementing the architecture enforcement tests for grammar kind isolation.

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
    - Investigate the parse cost breakdown: how much of the ~228ms is Tree-sitter parser initialization / tree building vs. `SuppressionTracker::from_ast` traversing comment nodes?
    - Determine whether suppression comments can be scanned more cheaply or parsed concurrently with AST visitation.
- **Subprocess Batching & Caching (`EnvContext`)**:
  - *Current*: Command rules spawn individual `jj` or `git` CLI calls per evaluation.
  - *Target*: Introduce a shared `EnvContext` struct that pre-fetches and caches repository state (e.g., batching queries into a single `jj log --json` or `git status` invocation) to ensure sub-10ms execution across multiple rules.
- **VCS Error Propagation**:
  - *Current*: VCS client query errors in command rules are swallowed to avoid blocking users on query failures.
  - *Target*: Propagate structured errors or display user warnings when the underlying VCS client fails unexpectedly, distinguishing clean working copies from failed CLI calls.

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

- **Multi-Violation AST Node Aggregation / Deduplication**:
  - *Current*: Rules that can trigger multiple times on a single declaration node (e.g., `no-identical-positional-types` when a function has both duplicate `str` and duplicate `int` parameter groups) emit separate diagnostics anchored at the same `(line, column)`.
  - *Target*: Define a unified strategy for either consolidating same-node rule findings into a single diagnostic or anchoring sub-findings on offending child tokens while preserving single-directive suppression ergonomics.
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
