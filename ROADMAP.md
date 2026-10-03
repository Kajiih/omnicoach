# Omni Toolkit Roadmap

This document serves as the single source of truth for architectural milestones, performance investigations, and planned ecosystem integrations for Omni.

Items here represent design areas and technical directions to evaluate rather than fixed implementation mandates.

---

## Architecture & Conformance

Design: `decisions/006_architectural_dag_and_conformance.md`. Enforcement: `src/architecture.rs` (graph definition) and `tests/architecture_conformance.rs` (source-tree conformance).

- **Conformance CST Edge Cases (Watch List)**:
  - *Current*: `summarize_rust_file` skips `macro_definition` bodies (production macros `architecture_component!` and `rule_test!` expand either to a doc attribute or inside `#[cfg(test)]`) and assumes paths do not start with a root-anchored leading `::` (`::omni::...` or `::ast_grep_core::...`).
  - *Target*: If production `macro_rules!` macros calling cross-component helpers (`$crate::...`) are introduced outside `src/lib.rs`, or if root-anchored `::` paths appear, extend `summarize_rust_file` to scan `macro_rule` body token trees and normalize leading `::` prefixes.
- **Non-transitive DAG edges**: every edge is transitive today, so a component reaches everything its dependencies reach. A "private" edge (a dependency that dependents do not inherit) would let the graph express isolation rules that currently need bespoke conformance checks.
- **Compiler-enforced subtree visibility (`pub(in crate::...)` / `pub(super)`)**: tighten item visibility to `pub(in crate::code_lint)`, `pub(in crate::command_lint)`, or `pub(super)` wherever component boundaries align with a directory subtree, so cross-domain access fails in `rustc` (`E0603`) before conformance tests run.
- **Validated Tree-sitter node kinds and field names**:
  - *Current*: `src/code_lint/ast.rs` and the `ast/` modules compare node kinds and field names as plain strings (`node.kind() == "function_definition"`, `node.field("body")`). A typo compiles and silently never matches; only behavioural `rule_test!` cases catch it, when one exercises the branch. `ast.rs`, `ast/python.rs` and `ast/rust.rs` carry `omni:disable-file [repeated-literal]` until this is resolved (design: `docs/dev/repeated_literal/`).
  - *Investigate, then pick one* (a new dependency is acceptable):
    - **(c) Typed CST via `type-sitter`** (generated node/field types from each grammar's `node-types.json`): compile-time checking. Key question: interop with `ast-grep` 0.45, which owns parsing and wraps nodes in its own `Node` type and pins its own `tree-sitter` version; check whether the inner `tree_sitter::Node` is reachable and version-compatible, and the migration cost of about 4k lines of AST helpers.
    - **(d) Grammar-conformance test**: parse the `ast` modules with Omni's own Rust parser, collect the string literals used as node kinds and field names, and assert each exists via `Language::id_for_node_kind` / `Language::field_id_for_name` (reachable through `SupportLang::get_ts_language()`). No dependency or migration; heuristic extraction limited to the call shapes it recognises.
  - *Then*: remove the `repeated-literal` file-level disables if the chosen option makes the strings typed or validated.
- **Standalone crate extraction**: once the DAG design items above settle (or when a second project needs it), evaluate extracting the declarative architecture and conformance engine into a standalone publishable crate (zero-dependency `define_architecture!` / `architecture_component!` macros in `[dependencies]`, CST conformance runner behind a `check` feature in `[dev-dependencies]`), moving `summarize_rust_file` out of `src/code_lint/ast/rust.rs`.
- **Pre-existing Python collector defects** (found in the signature collection review, `docs/dev/signature_collection_types/06_review_and_audit.md`):
  - `parse_param_parts`: four near-copies and an unreachable `dfs()` fallback.

## Rule Engine & Declarative Rules

- **Unified Single-Pass AST Visitor Dispatch**:
  - *Context & Problem*: Today, each registered rule implements `check_file` independently and executes its own AST search or traversal over the file. At 16 rules on ~8k lines, rule passes take ~180ms total (~11ms/rule). However, this scales linearly as $O(\text{files} \times \text{rules})$: at 100+ rules, traversing the syntax tree 100 times per file becomes a multi-second bottleneck.
  - *Investigation & Design Questions*:
    - Investigate how high-throughput linters dispatch rules. (e.g. Ruff's `Checker` uses a single AST walk with match arms and $O(1)$ bitset checks; Biome groups subscriptions by `SyntaxKind`; Clippy fuses passes into combined callbacks).
    - Can we establish a rule interest declaration (e.g. target node kinds) early in the trait lifecycle without breaking existing rule independence?
    - How do we handle rules that need multi-stage context (like `environment_variable_in_function` traversing upward or `too_many_assertions` counting inner blocks) within a unified traversal?
  - *Trigger*: When the code rule registry approaches ~30–40 rules, or when rule evaluation time exceeds parse time.
- **Rule Naming Canonicalization (`heck`)**:
  - *Target*: Support case-insensitive and format-tolerant rule selection (matching `SingleLetterName`, `single-letter-name`, and `single_letter_name` interchangeably).
  - *Trigger*: When adding declarative AST rule files or multi-rule alias configurations.
- **Rule Autofix Engine (`diffy` / `similar`)**:
  - *Target*: Extend `CodeRule` and `CommandRule` with optional auto-fix transformations. Support `--fix` and `--fix --dry-run` with in-memory unified diff previews before writing changes to disk.
  - *Trigger*: When implementing the first batch of auto-fixable rules (e.g., replacing `logging.error` with `logging.exception`).
- **Multiline Decorator & Attribute Span Awareness for `omni:ignore`**:
  - *Current*: `compute_effective_target_line` in `src/code_lint/suppression.rs` advances `end_target_line` across contiguous lines starting with `@`, `#[`, `//`, or `#`, handling single-line decorators and attributes. However, multiline decorators or attributes whose continuation lines do not start with `@` or `#[` stop the line scan early.
  - *Target*: Use AST decorated/attributed node spans in the suppression resolver so `omni:ignore` placed above a multiline decorator/attribute block suppresses diagnostics on the underlying declaration.
- **Generic Container Base-Type Matching (`identical-positional-types`)**:
  - *Current*: Positional parameter types are compared by exact formatted annotation string (`dict[str, int]` != `dict[str, float]`).
  - *Target*: Optionally normalize or group generic collection/mapping containers (`dict[...]`, `Mapping[...]`, `list[...]`, `Sequence[...]`) so multiple positional mappings or sequences are flagged even when their inner type arguments differ.
- **Escaping Nested Scopes (`environment-variable-in-function`)**:
  - *Current*: The boundary exemption (`main`, `from_env`, ...) is inherited by every scope declared inside it, which is correct for nested functions and closures but also exempts a class declared inside a boundary whose methods later escape (returned, registered as a callback).
  - *Target*: Treat a `class` / `impl` declared inside a boundary as a barrier that resets the exemption, once a real-world occurrence justifies the added language-specific complexity.
- **Import-Aware Qualified Call Resolution (`src/code_lint/semantic/calls.rs`)**:
  - *Current*: Banned calls are matched syntactically by call-site name (like `ast-grep` and the legacy linter), as documented in each rule's `what_it_does`. A bare call imported from an unrelated library (`from sqlalchemy import cast`, `from httpx import patch`) is flagged, while an aliased module call (`import typing as t; t.cast(...)`) is missed. Type annotations in the collection type rules have the same gap: `t.List` and `from typing import List as L` are not resolved (pass case `known_gap_typing_module_alias_not_resolved`), and a local `class Set` is still treated as `typing.Set`.
  - *Target*: Evaluate adding a per-file scope and import symbol table (modeled on Ruff's `SemanticModel::resolve_qualified_name`) that resolves imported and aliased callees to their canonical qualified path while distinguishing module imports from local parameter/fixture receivers (`mocker.patch`, `monkeypatch.setattr`, `loop.create_task`).
- **Mode-Aware Explanation Hint**:
  - *Current*: Whether an adjacent comment excuses a finding depends on the effective enforcement mode, which users can change, so templates and docs cannot state it truthfully. The signature collection rules no longer mention the comment alternative; `suppressed-exception` still hardcodes "or add a comment" in its suggestion and "by default" in its docs.
  - *Target*: In `src/code_lint/contract.rs` (where `RequireExplanation` diagnostics are filtered), append a generated hint such as "or explain it in an adjacent comment" to the suggestion only when the effective mode is `require-explanation`. Have `--explain` document where the comment goes (line above the header, on or above the attribute), possibly via a per-rule explanation-location field, then migrate `suppressed-exception`.

## Candidate Rules

Source: Python Tip of the Week #069 "Prefer constants over wild values" (Python tips #069) and the legacy linter `IndexingInsteadOfUnpackingRule` (`scratch/legacy_reference/legacy_lints.py`). Candidates to prioritize, not commitments.

- **`repeated-index-access`** (Python, Rust — tip `#unpack`) — **implemented**, design in `docs/dev/prefer_tuple_unpacking/`. Follow-ups:
  - *Named record for sparse positional access*: reads needing more `_` placeholders than `repeated-index-access` allows (`row[0]`, `row[7]`) → `NamedTuple` / dataclass / struct (or `csv.DictReader` for CSV rows).
  - *Tuple-returning functions*: functions returning tuples of ≥3 elements → `NamedTuple` / dataclass / struct.
  - *Multi-field tuple structs* (Rust): tuple structs with ≥2 fields → named-field struct (tuple structs reserved for newtypes).
  - *`re.Match` group indexing* (Python): `m[1]`, `m[2]` → `a, b = m.groups()`.
  - *Rust slice patterns*: `arr[0]`, `arr[1]` on fixed-size arrays → `let [a, b] = arr;`.
  - *Macro arguments*: reads inside expression-like macros (`format!`, `assert_eq!`, `vec![]`) are not inspected because tree-sitter leaves them as flat `token_tree` tokens (pass case `known_gap_macro_arguments_not_inspected`). Supporting them needs a multi-token span in `AstNode` (`cmd.span.0` is several tokens, not one node) or re-parsing the arguments as expressions.
  - *Receiver rebinding*: `a = p[0]; p = nxt(); b = p[1]` groups two different values as one receiver (both languages). Needs binding awareness (split the group at each rebinding).
  - *Shadowing*: a closure parameter (`|t| t.1`) or comprehension variable reusing the receiver's name is grouped with the outer receiver (both languages).
  - *Receiver normalization*: receivers are grouped by source text, so `(t).0` vs `t.0`, `len((xs))` vs `len(xs)`, and chains split across lines are not recognized as the same receiver.
- **Signature & attribute collection type rules** (`concrete-collection-parameter`, `concrete-collection-return`, `concrete-collection-attribute`, `mutable-collection-parameter`, `mutable-collection-return`, `mutable-collection-attribute`, `specific-collection-parameter` — Python) — **implemented**, design in `docs/dev/signature_collection_types/`. Follow-ups:
  - *Type alias resolution*: `type IntList = list[int]` or `IntList: TypeAlias = list[int]` used in annotations is treated as an unknown generic/identifier until per-file type-alias resolution is added.
  - *Cross-file caller and subclass mutation tracking*: return and attribute mutation checks (`collect_locally_mutated_return_functions`, `collect_public_class_attributes`) operate within a single file (`G3`); cross-module mutations rely on the default `require-explanation` mode.
  - *String annotation resolution*: string annotations (`"list[int]"`) are not parsed (pass case `known_gap_string_annotation_not_parsed`). Import aliases are covered by "Import-Aware Qualified Call Resolution".
  - *Callee resolution for returned values*: `collect_locally_mutated_return_functions` matches callees by bare name, so mutating `cfg.get(...)` exempts every function named `get` in the file (pass case `known_gap_same_named_callee_exempts_function`).
  - *Header-wide explanation scope*: one `RequireExplanation` comment above a `def` silences every finding on that header, across rules. Framework-wide behavior; per-rule scoping would need comment-to-rule association.
  - *Unified use-site classifier*: `is_safe_readonly_parameter_reference` (mutation) and `record_reference_capability` (capability) classify the same uses separately and can diverge. Merge into one `classify_use`; the `python.rs` unit tests cover both.
  - *Body walk cost*: body walkers recurse without a depth bound and run once per parameter, and each rule rebuilds the same file-wide data. Share one per-function use index if profiling shows a cost.
- **`no-manual-enum-name-map`** (Python, Rust — tip `#protobufs`):
  - *Detection*: A dict literal where every entry is `'NAME': X.Y.NAME` (string key equals the value's last attribute). Suggest `Enum.Value(name)` (protobuf), `Enum[name]`, or `Enum.__members__`.
  - *Rust*: `match` arms mapping `"Alpha" => Kind::Alpha` → derive `strum::EnumString`.
- **`no-overprecise-float-in-tests`** (Python, Rust — tip `#keep_it_simple`):
  - *Detection*: Float literals in test code with more significant digits than a configurable threshold (e.g. >6).
  - *Overlap*: Clippy `excessive_precision` only flags digits beyond `f64` representability, not unreadable test values.
- **`repeated-literal`** (Python, Rust — tip core rule and `#no_magic`) — **implemented**, design in `docs/dev/repeated_literal/`. Follow-ups:
  - *Values inside exempt macros* (Rust): the whole exempt macro (`assert_eq!`, `format!`, …) is skipped, so `assert_eq!(x, "expected")` repeated in production code is not counted (pass case `known_gap_values_inside_exempt_macros`). Counting only value arguments needs per-macro knowledge of which arguments are format strings.
  - *Negative numbers as separate tokens*: where the sign is a bare `-` token (Rust macro `token_tree`, Python mapping, keyword and union patterns), the number is skipped rather than counted as `N` (pass cases `known_gap_negative_numbers_in_macros`, `known_gap_negative_numbers_in_mapping_and_keyword_patterns`).
  - *Tuple indices in non-exempt macros* (Rust): inside a `token_tree`, `pair.0` is a bare integer token, so repeated tuple indices in custom macros count as numbers.
  - *Unpacked and chained constants* (Python): `A, B = 1, 2` and `A = B = 1` are not recognized as constant definitions; their values count as inline uses.
  - *Typed template placeholders*: constants such as `CALLEE = "callee"` exist only to name a template placeholder once. A typed placeholder (enum or typed key) would remove them.
  - *Multi-diagnostic `rule_test!` cases*: a `fail` case expects exactly one span, so 3+ copies and a constant plus two uses are only unit-tested on the collector. A `fail` case listing several spans would cover them (also needed by `nested-function`).
  - *Tree-sitter node kinds*: `ast.rs`, `ast/python.rs` and `ast/rust.rs` opt out with `omni:disable-file`. Lift once node kinds are typed or validated (see "Validated Tree-sitter node kinds").
- **`nested-class`** (Python — companion of `nested-function`):
  - *Detection*: a `class` defined inside a function or method body. Suggest moving it to module level and passing captured values to its constructor.
  - *Context*: `nested-function` no longer flags the methods of such a class (pass case `method_of_local_class_not_flagged`), since the local class, not each method, is the design choice to review.
- **Time-unit literal arithmetic** (extension of `primitive-duration` — tip `#rationale`):
  - *Detection*: `24 * 60 * 60`, `60 * 60`, `86400`, `3600` → `timedelta` / `Duration`.
- **Import alias conventions** (`import x as y`, `use x as y`):
  - *Context*: Naming rules (`single-letter-name`, `abbreviated-name`, `type-suffixed-name`, `primitive-duration`) skip all imports, including aliased imports.
  - *Investigation*: Evaluate how much is already covered by Ruff's `flake8-import-conventions` (`ICN001` `unconventional-import-alias`, `ICN002` `banned-import-alias`) and Pylint (`PLC0414` `useless-import-alias`), and whether a dedicated multi-language import-alias rule is warranted in Omni.
- *Not pursued*: magic numbers in comparisons (covered by Ruff `PLR2004`), bare HTTP status codes (too narrow), path composition (Ruff `PTH`), test correspondence-signaling and same-value/different-meaning constants (require semantic understanding).

---

## 2. Performance & Concurrency Architecture

- **Directory Discovery Parallelism & Micro-Run Overhead**:
  - *Current*: File-level analysis runs in parallel via `rayon` (`targets.into_par_iter()`) with deterministic sorting across both plain-text and JSON output, while directory traversal in `collect_directory_candidates` runs single-threaded via `ignore::WalkBuilder::build()`.
  - *Investigation*:
    - Evaluate whether `ignore::WalkParallel` improves directory discovery on large repositories compared to single-threaded collection + `rayon`.
    - Measure `rayon` thread-pool initialization overhead on small repositories to ensure micro-runs and pre-commit hooks are not penalized.
- **Parse & Pipeline Floor Profiling**:
  - *Context*: Disabling all rules via tag exclusion shows that the shared per-file pipeline (walking files, reading from disk, tree-sitter parsing via `ast-grep`, and comment suppression scanning) accounts for ~228ms (56% of total runtime on ~8k lines). Pre-I/O `detect_language` filtering is in place, and `SuppressionTracker::from_file` skips the comment walk on files without `omni:`.
  - *Investigation*: Break down the remaining cost between Tree-sitter parser initialization and tree building, and whether either can be reduced.
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

- **Feature-Gated Rich Terminal Diagnostics (`miette`)**:
  - *Current*: Fast, zero-dependency printer in `src/diagnostic.rs` outputting standard compiler-style format (`path:line:col: [CODE] message`) and JSON.
  - *Target*: Add an optional Cargo feature (`features = ["miette"]`) that enables rich, syntax-highlighted source snippets with colored squiggly underlines and clickable rule documentation URLs, while keeping the default pre-commit hook binary lightweight and fast.
- **Polished Diagnostic Summaries (`pluralizer` / Native Helper)**:
  - *Target*: Clean grammatical inflection ("1 violation" vs "3 violations") in terminal summary footers, JSON reports, and future JUnit/SARIF export formats.

---

## 5. Tags, Discovery & Documentation

Design rationale: [ADR 007](decisions/007_rule_taxonomy_and_selection.md) (taxonomy and selection) and [ADR 008](decisions/008_rule_documentation_and_discovery.md) (rule docs and discovery). Contributor guide: [docs/dev/tag_guide.md](docs/dev/tag_guide.md).

- **Shadowed-selector config warning**: warn when a `select` or `ignore` entry changes no rule's outcome (needs config warning plumbing).
- **`--list-tags` discovery command**: print the topic tree (labels, parents, synonyms, descriptions, scope notes) from the `Topic` consts. Today users read it from `docs/dev/tag_guide.md` §5, a copy kept in sync by the `topic_tree_table_matches_the_topics` test; once the CLI prints it, the README can point there and §5 can go.
- **Examples for suppression audits and command rules**:
  - *Current*: Code rules document one executed example per language (`RuleDoc.examples`, run by `rule_test!`). Suppression audits and command rules have no harness that could run an example, so they declare `examples: &[]` (enforced empty in `tests/registry.rs`).
  - *Target*: Give them executed examples once a harness exists. Audits can reuse `Example` (code with suppression comments) and only need a harness. Command rules need another shape (a command line plus a fake `jj` state), hence their own type and harness.
  - *Placement (decided: keep in `RuleDoc`)*: Moving `examples` from `RuleDoc` to `CodeRule` would make empty examples unrepresentable for audits and command rules and drop the emptiness test, without losing any compile check. Rejected for now: it splits the user-facing doc across two places, plumbs examples through `AnyCodeRule` and `RegisteredRule`, and audits (bare `Declaration`s) would need a wrapper type before they could get examples. Revisit when command rule examples land, since they need their own typed field anyway.
- **Discovery & Documentation Follow-ups**:
  - JSON output for discovery commands (`--format json` for `--list-rules` / `--explain`), and `tags` on JSON diagnostics. Include per-language message overrides (`summary` / `rationale` / `suggestion`), not just the base text.
  - Path-aware status in `explain` (evaluating `per-file-ignores` for a given file path).
  - Generated in-repo rule catalog guarded by a golden-file drift test.
  - Styled Markdown rendering in the terminal.
  - JSON Schema for `.omnilint.toml` (editor completion). It must be registry-aware, because `rules` is a free map: per-rule keys and defaults, the threshold bounds each rule uses, language sub-tables, and every rule name and tag as a selector value. Document the `#:schema` directive (Taplo) in the README.
  - Typed overlap / sources field linking rules to equivalent Ruff / Clippy rules.
  - Decide whether plain diagnostics keep printing the full rationale and suggestion on every hit.
  - Investigate subcommands (`rules`, `explain`, a default `check`) instead of the `--list-rules` / `--explain` flags.
- **Later / Out-of-Scope Ideas**:
  - **Abstraction-driven auto-tagging**: importing or using a domain helper (e.g. a logging abstraction) automatically attaches its topic tag to the rule.
  - **Computed `recommended` view**: if curated presets are ever introduced, define `recommended` as a computed view (`exact` ∧ `unopinionated`) rather than a second hand-maintained list.
