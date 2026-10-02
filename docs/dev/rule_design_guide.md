# Rule Design Guide & Best Practices

This guide outlines principles for designing high-signal, actionable lint rules in Omni. It focuses on the design philosophy of rules—not engine implementation details.

---

## 1. Rule Granularity: 1 Rule = 1 Antipattern
A lint rule is not a broad category (e.g. avoid rules like `async-hygiene` or `test-cleanliness`). A rule must target **one specific antipattern**.

* **Why Granularity Matters**: Configuration, CI blocking, and suppressions operate at the rule level. If two distinct antipatterns share a rule name, teams cannot suppress or configure one without affecting the other.
* **The Split Test**: If two violations differ in **why** they are bad or **how** to fix them, they are separate rules.
  * *Example*: Wall-clock `sleep(5)` causes flaky tests and slow CI (`sleep-in-tests`). Zero-duration `sleep(0)` is an attempt to yield to the scheduler (`zero-sleep-in-tests`). Different rationales and remedies mean different rules.
* **When to Parameterize**: If the concept, rationale, and fix are identical across syntax variations (e.g., compound boolean assertions vs. tuple equality packing), keep them under one rule and parameterize the message.

---

## 2. Actionability: Why & How, Not Just What
A diagnostic that only says "X is banned" causes frustration. Developers need to understand the harm and the idiomatic path forward.

Every rule diagnostic is split into three strictly orthogonal fields—**never repeat information across them**:
1. **What (`summary`)**: State the factual syntactic or semantic condition observed on the flagged construct (e.g., `"Name `{name}` is a single letter."` or `"Multiline string literal is not wrapped in a dedent helper."`). Do **not** embed the rationale (e.g., *"obscures intent"*, *"bypasses type checking"*) or filler judgment (*"is discouraged"*, *"is prohibited"*) in `summary`.
2. **Why (`rationale`)**: Explain the concrete failure mode or maintenance hazard (flakiness, hidden tracebacks, corrupted runtime values, broken searchability). Do **not** restate what construct was matched or explain how to fix it.
3. **How (`suggestion`)**: Prescribe a **single canonical pit-of-success replacement** per language so a developer or AI agent can fix the violation autonomously (e.g., `inspect.cleandoc(...)` in Python, `indoc::indoc!` in Rust). Do **not** repeat `"instead of <bad construct>"` or re-state why the original code was harmful.

The wording of each field (sentence form, tense, quoting, placeholders) and the naming of rules, keys and components are fixed by `naming_and_message_style_guide.md`.

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

* **Allowlists & Denylists**: Name-checking rules (abbreviations, type suffixes) must let a project replace or extend the default list. Certain terms may be primitive keywords in one language or valid domain acronyms in a specific codebase (e.g., `str` is a primitive in Rust, not an abbreviation).
* **Thresholds**: Numeric bounds (e.g., maximum assertions per test) must be configurable so teams can adjust the strictness without disabling the rule entirely.

---

## 6. Where the Mechanics Live

This guide is about *why*. The contracts live next to the code, so they cannot drift from it:

| Topic | Reference |
| :--- | :--- |
| Rule contract, options typing, opaque AST nodes | `CodeRule` (`src/code_lint/contract.rs`), `RuleOptions` (`src/rule_declaration/options.rs`) |
| Message template fields | `ViolationTemplate` (`src/diagnostic.rs`) |
| User-facing doc and executed examples | `RuleDoc`, `Example` (`src/rule_declaration/documentation.rs`) |
| Testing contract and case-writing standards | `rule_test!` (`src/test_utils.rs`) |
| Components, layering, sibling isolation, single public path | `src/architecture.rs` module doc |
| Classification | [tag_guide.md](tag_guide.md) |
| Names and message wording | [naming_and_message_style_guide.md](naming_and_message_style_guide.md) |
