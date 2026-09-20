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

- **Single-Pass Call Expression Matching (`find_banned_calls`)**:
  - *Current*: `calls::find_banned_calls` executes `root.find_all(...)` sequentially for each pattern entry, repeatedly traversing the entire syntax tree (causing ~1.3s overhead per pattern on 800+ line files in debug mode).
  - *Target*: Perform a single-pass AST traversal collecting `call_expression` (Rust) and `call` (Python) nodes, resolving callee text and checking against a `HashSet` in $O(1)$ to eliminate redundant full-tree traversals and achieve sub-millisecond execution.
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
