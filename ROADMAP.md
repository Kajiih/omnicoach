# Omni Toolkit Roadmap

This document serves as the single source of truth for architectural milestones, performance optimizations, and planned ecosystem integrations for Omni.

---

## 1. Rule Engine & Declarative Rules

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

---

## 2. Performance & VCS Optimizations

- **Partitioned Call Expression Matching (`find_banned_calls`)**:
  - *Current*: `calls::find_banned_calls` executes `root.find_all(...)` sequentially for each pattern entry, repeatedly traversing the entire syntax tree (measured at ~1.4s per pattern on an 830-line file in debug mode; 70s on the 8k-line repo in release).
  - *Target*: Partition the deny list into literal callee names and structural `ast-grep` patterns. Literal names are evaluated in a single-pass AST traversal collecting `call_expression` (Rust) and `call` (Python) nodes and checked against a `HashSet` in $O(1)$; entries with metavariables (e.g. `$LOOP($$$LOOP_ARGS).create_task`) or custom pattern syntax retain structural `find_all` matching. Eliminates redundant traversals for the vast majority of entries while preserving full rule expressiveness.
- **Subprocess Batching & Caching (`EnvContext`)**:
  - *Current*: Command rules spawn individual `jj` or `git` CLI calls per evaluation.
  - *Target*: Introduce a shared `EnvContext` struct that pre-fetches and caches repository state (e.g., batching queries into a single `jj log --json` or `git status` invocation) to ensure sub-10ms execution across multiple rules.
- **VCS Error Propagation**:
  - *Current*: VCS client query errors in command rules are swallowed to avoid blocking users on query failures.
  - *Target*: Propagate structured errors or display user warnings when the underlying VCS client fails unexpectedly, distinguishing clean working copies from failed CLI calls.

---

## 3. Reporting & Diagnostics

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

## 4. Testing & Verification

- **Automated Rule Test Verification**:
  - *Current*: Tests are validated via runtime registry loops.
  - *Target*: Compile-time or CI assertion ensuring that colocated test suites exist for every registered rule, preventing rules from being added without corresponding snapshot and unit test coverage.

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
