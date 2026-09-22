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

Every rule diagnostic must answer three questions:
1. **What (`summary`)**: State the defect concisely (e.g., `"Multiline string literal is not wrapped in a dedent helper."`), rather than merely restating the syntax or naming the rule.
2. **Why (`rationale`)**: Explain the tangible failure modes (flakiness, hidden exceptions, corrupted runtime values, broken visual hierarchy). Never cite "team convention" or "linter policy" as the rationale.
3. **How (`suggestion`)**: Point to a **single canonical pit-of-success solution** per language so a developer or AI agent can fix the violation autonomously without guesswork (e.g., recommend `inspect.cleandoc(...)` in Python rather than listing multiple competing helpers with different edge cases).

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
All rule unit tests in `src/code_lint/rules/*.rs` must use `crate::rule_test!`.

* **Never Snapshot Static Prose in Unit Tests**: `insta::assert_snapshot!` and `assert_code_rule_snapshot` are reserved for end-to-end CLI output tests (`tests/cli.rs`). Unit tests must never assert on rendered static `TEMPLATE` text (`summary`, `rationale`, `suggestion`), as `src/rules.rs` validates template integrity globally.
* **Declarative Suites with `rule_test!`**: Every rule file invokes `#[cfg(test)] crate::rule_test!(RuleName, { Language => { pass: [...], fail: [...] } })` at the bottom of the file. Bespoke `#[test]` or `#[rstest]` functions are strictly forbidden in rule files and enforced by CI.
* **Language Completeness**: The generated `language_completeness` test verifies at test execution time that every `SupportLang` declared in `rule.supported_languages()` is covered in `rule_test!`.
* **Mandatory AST Span Verification**:
  * Omit `=> [...]` when the entire test snippet is the flagged AST node: `case_name => "typing.cast(int, x)"`.
  * Add `=> [r#"..."#]` when the flagged construct is an inner AST slice inside setup syntax: `case_name => r#"fn build() { let bad = "..."; }"# => [r#""...""#]`.
  * Leading block indentation is normalized automatically across lines `2..N`, so expected inner snippets can always be written with clean `indoc!`-dedented multiline strings.
* **Minimum Required Cases**:
  1. **Core Antipattern (`fail`)**: The primary construct the rule flags.
  2. **Canonical Fix & Syntactic Exemptions (`pass`)**: The recommended pit-of-success replacement (e.g. `inspect.cleandoc`, `indoc::indoc!`, docstrings) to prove the suggested fix passes the rule.
  3. **Do Not Re-Test Framework Config Plumbing**: `FilterListDefaults` and `ThresholdDefaults` resolution are tested centrally in `src/core.rs`. Individual rule tests must not re-test framework configuration parsing.
