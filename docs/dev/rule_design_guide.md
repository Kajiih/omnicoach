# Rule Design Guide & Best Practices

This guide outlines principles for designing high-signal, actionable lint rules in Omni. It focuses on the design philosophy of rules—not engine implementation details.

---

## 1. Rule Granularity: 1 Rule = 1 Antipattern
A lint rule is not a broad category (e.g. avoid rules like `async-hygiene` or `test-cleanliness`). A rule must target **one specific antipattern**.

* **Why Granularity Matters**: Configuration, CI blocking, and suppressions operate at the rule level. If two distinct antipatterns share a rule name, teams cannot suppress or configure one without affecting the other.
* **The Split Test**: If two violations differ in **why** they are bad or **how** to fix them, they are separate rules.
  * *Example*: Wall-clock `sleep(5)` causes flaky tests and slow CI (`no-sleep-in-tests`). Zero-duration `sleep(0)` is an attempt to yield to the scheduler (`no-zero-sleep-in-tests`). Different rationales and remedies mean different rules.
* **When to Parameterize**: If the concept, rationale, and fix are identical across syntax variations (e.g., compound boolean assertions vs. tuple equality packing), keep them under one rule and parameterize the message.

---

## 2. Actionability: Why & How, Not Just What
A diagnostic that only says "X is banned" causes frustration. Developers need to understand the harm and the idiomatic path forward.

Every rule diagnostic is split into three strictly orthogonal fields—**never repeat information across them**:
1. **What (`summary`)**: State the factual syntactic or semantic condition observed on the flagged construct (e.g., `"Variable name `{name}` is a single-letter."` or `"Multiline string literal is not wrapped in a dedent helper."`). Do **not** embed the rationale (e.g., *"obscures intent"*, *"bypasses type checking"*) or filler judgment (*"is discouraged"*, *"is prohibited"*) in `summary`.
2. **Why (`rationale`)**: Explain the concrete failure mode or maintenance hazard (flakiness, hidden tracebacks, corrupted runtime values, broken searchability). Do **not** restate what construct was matched or explain how to fix it.
3. **How (`suggestion`)**: Prescribe a **single canonical pit-of-success replacement** per language so a developer or AI agent can fix the violation autonomously (e.g., `inspect.cleandoc(...)` in Python, `indoc::indoc!` in Rust). Do **not** repeat `"instead of <bad construct>"` or re-state why the original code was harmful.

---

## 3. Anticipate Perverse Incentives
Developers under pressure will take the path of least resistance to satisfy a linter. A poorly designed rule often incentivizes worse code.

* **Assertion Limits**: Limiting assertions per test can tempt developers to pack multiple conditions into boolean tuples (`assert (a, b) == (1, 2)`) or wrap assertions in obscure closures solely to bypass the count. Anticipate this by pairing limits with rules against assertion packing, and encourage domain assertions or snapshot testing instead.
* **Blanket Avoidance**: Make sure rules encourage the correct abstraction, not superficial workarounds that defeat static analysis.

---

## 4. Universal Concepts, Idiomatic Guidance
The architectural antipattern is usually language-agnostic; the solution is almost always language-specific.

* **Unify the concept**: Antipatterns like unstructured concurrency, arbitrary test delays, or Hungarian notation exist across languages.
* **Specialize the guidance**: Tailor suggestions to native idioms and libraries (e.g., recommend `@pytest.mark.parametrize` in Python vs. `#[rstest]` in Rust). Avoid generic advice when language-standard tools exist.

---

## 5. Extensibility Over Dogma
Static rules should establish sane defaults while respecting domain vocabulary.

* **Allowlists & Denylists**: Name-checking rules (abbreviations, type suffixes) must support extension and exemption. Certain terms may be primitive keywords in one language or valid domain acronyms in a specific codebase (e.g., `str` is a primitive in Rust, not an abbreviation).
* **Thresholds**: Numeric bounds (e.g., maximum assertions per test) must be configurable so teams can adjust the strictness without disabling the rule entirely.

---

## 6. Rule Testing Standards (`rule_test!`)
All rule unit tests in `src/code_lint/rules/*.rs` must use `crate::test_utils::rule_test!`.

* **Never Snapshot Static Prose in Unit Tests**: `insta::assert_snapshot!` and `assert_code_rule_snapshot` are reserved for end-to-end CLI output tests (`tests/cli.rs`). Unit tests must never assert on rendered static `TEMPLATE` text (`summary`, `rationale`, `suggestion`), as `tests/registry.rs` validates template integrity globally.
* **Declarative Suites with `rule_test!`**: Every rule file invokes `#[cfg(test)] crate::test_utils::rule_test!(RuleName, { Language => { pass: [...], fail: [...] } })` at the bottom of the file. Bespoke `#[test]` or `#[rstest]` functions are strictly forbidden in rule files and enforced by CI.
* **Language Completeness**: The generated `language_completeness` test verifies at test execution time that every `SupportLang` declared in `rule.supported_languages()` is covered in `rule_test!`.
* **Mandatory AST Span Verification**:
  * Omit `=> r#"..."#` when the entire test snippet is the flagged AST node: `case_name => "typing.cast(int, x)"`.
  * Add `=> r#"..."#` when the flagged construct is an inner AST slice inside setup syntax: `case_name => r#"fn build() { let bad = "..."; }"# => r#""...""#`.
  * Leading block indentation is normalized automatically across lines `2..N`, so expected inner snippets can always be written with clean `indoc!`-dedented multiline strings.
  * Each `fail` case is also run with its code repeated twice in one file and must report both occurrences, so a rule that stops after its first match (`find` instead of `find_all`, early `return`, stray `break`) fails.
  * A `fail` case asserts exactly one diagnostic; multi-node cases are not supported. Known cases that would need them if revisited: flagged constructs nested inside flagged constructs (a `def` inside a nested `def` in `flat-scope-enforced`), rules reporting every occurrence inside one node (the legacy linter `QuoteWrappedPlaceholderRule`, `DocstringOptionalArgRule`, `BannedTypeAnnotationsRule`), and rules aggregating per file or scope, which would need an opt-out from the repeated-occurrence check (the legacy linter `IndexingInsteadOfUnpackingRule`).
* **Minimum Required Cases**:
  1. **Core Antipattern (`fail`)**: The primary construct the rule flags.
  2. **Canonical Fix & Syntactic Exemptions (`pass`)**: The recommended pit-of-success replacement (e.g. `inspect.cleandoc`, `indoc::indoc!`, docstrings) to prove the suggested fix passes the rule.
  3. **Do Not Re-Test Framework Config Plumbing**: `FilterListDefaults` and `LanguageDefaults` threshold resolution are tested centrally in `src/core.rs`. Individual rule tests must not re-test framework configuration parsing.
* **One Behavior per Case**: Each `pass`/`fail` case exercises exactly one code path (one banned pattern, one exemption, one AST construct) and is named after it, so a failing case name pinpoints the regression.

---

## 7. Rule Layering: Rules Express Policy, Not Plumbing
Rule files are the highest-level policy code in the codebase: they state *what* is banned and *why*, and delegate *how* to lower components in the architectural DAG (`src/architecture.rs`, documented in `decisions/006_architectural_dag_and_conformance.md`).

| Component | Modules | Role |
| :--- | :--- | :--- |
| `FoundationPrimitives` | `architecture`, `diagnostic`, `diff` | Zero-dependency primitives: DAG, locations, spans, diffs |
| `CoreVocabulary` | `core` | Domain vocabulary, config, tags, selectors |
| `CodeSyntaxAdapters` / `CommandVcsAdapters` | `code_lint::ast`, `command_lint::vcs` | Syntax and VCS adapters; `code_lint::ast` encapsulates `ast_grep_core` |
| `CodeSemanticEngines` | `code_lint::semantic::{bindings,calls,comments}` | Cross-language semantic engines |
| `CodeRuleContracts` / `CommandRuleContracts` | `code_lint::rule`, `command_lint::rule` | Rule contracts (`CodeRule`, `CommandRule`) |
| `CodeSuppressionEngine` | `code_lint::suppression` | Inline comment suppression tracker and directive policies |
| `CodeLintRules` / `CommandLintRules` | `code_lint::rules::*`, `command_lint::rules::*` | Concrete lint rules and static registries (`CODE_RULES`, `COMMAND_RULES`) |
| `CodeLintRunner` / `CommandLintRunner` | `code_lint::runner`, `command_lint::runner` | Multi-file and command orchestration runners |

* **Rules See Opaque Nodes**: `CodeRule::check_file` receives a `ParsedFile`. Rules query it through named AST/semantic helpers (`ast::python::extract_classes`, `ast::collect_call_candidates`, `self.check_banned_calls(...)`) and anchor diagnostics on `AstNode`, which exposes text, span, and location but no tree navigation or grammar kinds. A rule needing a new structural fact adds a named helper to `code_lint::ast` rather than walking the tree itself.
* **Universal Sibling Isolation**: Rule files inherit `CodeLintRules` / `CommandLintRules` from their parent component root (`src/code_lint/rules.rs`, `src/command_lint/rules.rs`) and never import sideways from sibling rules; shared logic moves down to `code_lint::semantic` or `code_lint::ast`.
* **Single Public Path & Intra-Component Relative Paths**: Every item keeps a single canonical path. Cross-component imports use `crate::`; relative paths (`super::`, `self::`) are allowed within the same component subtree. Visible re-exports (`pub use` / `pub(crate) use`) are allowed only when a parent module re-exports items from a private direct child submodule (`mod child; pub use self::child::Item;`). A `macro_rules!` macro declares its module path with `pub(crate) use name;` next to its definition; `#[macro_export]` is used only in `src/lib.rs`.
* **Enforced, Not Documented**: `tests/architecture_conformance.rs` (powered by `summarize_rust_file` in `code_lint::ast::rust`) enforces DAG reachability, universal sibling subtree isolation, single-path/facade rules, intra-component relative path bounds, and `ast_grep_core` encapsulation.

