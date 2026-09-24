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

* **Never Snapshot Static Prose in Unit Tests**: `insta::assert_snapshot!` and `assert_code_rule_snapshot` are reserved for end-to-end CLI output tests (`tests/cli.rs`). Unit tests must never assert on rendered static `TEMPLATE` text (`summary`, `rationale`, `suggestion`), as `src/rules.rs` validates template integrity globally.
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
  3. **Do Not Re-Test Framework Config Plumbing**: `FilterListDefaults` and `ThresholdDefaults` resolution are tested centrally in `src/core.rs`. Individual rule tests must not re-test framework configuration parsing.
* **One Behavior per Case**: Each `pass`/`fail` case exercises exactly one code path (one banned pattern, one exemption, one AST construct) and is named after it, so a failing case name pinpoints the regression.

---

## 7. Rule Layering: Rules Express Policy, Not Plumbing
Rule files are the highest-level code in the codebase: they state *what* is banned and *why*, and delegate *how* to lower layers. Each layer depends only on layers below it.

| Layer | Modules | Role |
| :--- | :--- | :--- |
| L1 | `diagnostic`, `diff` | Primitives: locations, spans, output |
| L2 | `core`, `code_lint::ast`, `command_lint::vcs` | Domain vocabulary; `code_lint::ast` is the sole owner of `ast_grep_core` |
| L3 | `code_lint::{bindings,calls,comments}` | Cross-language semantic engines |
| L4 | `code_lint::rule`, `command_lint::rule`, `test_utils` | Rule contracts (`CodeRule`, `CommandRule`) |
| L5 | `code_lint::rules::*`, `code_lint::suppression`, `command_lint::rules::*` | Declarative rule policy |
| L6 | `rules`, `code_lint::runner` | Registry and orchestration |
| L7 | `lib.rs`, module roots, `bin/*` | Entry points and re-exports |

* **Rules See Opaque Nodes**: `CodeRule::check_file` receives a `ParsedFile`. Rules query it through named L2/L3 helpers (`ast::python::extract_classes`, `ast::collect_call_candidates`, `self.check_banned_calls(...)`) and anchor diagnostics on `AstNode`, which exposes text, span and location but no tree navigation or grammar kinds. A rule needing a new structural fact adds a named helper to `code_lint::ast` rather than walking the tree itself.
* **Rules Are Independent**: A rule never imports another rule or the L6 registry; shared logic moves down to L2/L3.
* **One Path per Item**: Import items from their defining module (`crate::code_lint::rule::CodeRule`, `crate::diagnostic::RuleName`). No re-exports: a facade would hide the defining layer from the architecture tests. A `macro_rules!` macro, which cannot carry a visibility, declares its single path with `pub(crate) use name;` next to its definition; `#[macro_export]` is not used.
* **Enforced, Not Documented**: `tests/architecture.rs` (`rust_arkitect`) fails on upward dependencies, cross-rule imports, re-exports, and `ast_grep_core` use outside `code_lint::ast`, `command_lint::rule` (Bash parser) and the `ast_dumper` debug binary. Grammar-kind isolation follows from type privacy: raw nodes never leave `code_lint::ast`.

