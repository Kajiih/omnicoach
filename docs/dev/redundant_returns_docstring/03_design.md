# 03 — Design: `returns-section-stutter`

> [!NOTE]
> Status: **PENDING** (lead validation). Batch D (docstrings), worker phase 3 of 7. Previous: [01_understand.md](01_understand.md), [02_references.md](02_references.md).

**Verdict: implement, narrowed and improved.** One rule, one template, Python only. No external tool covers it ([02_references.md](02_references.md) §2). The port fixes the legacy linter's noun false positive (G1), its typed-line, multi-section and `Return:` header misses (G2–G4), and its location (G5).

## 1. Decisions

| # | Decision | Why |
| :-- | :--- | :--- |
| D1 | **Antipattern = verb-led opening.** Flag a `Returns:` / `Yields:` section whose opening word is `Return`, `Returns`, `Yield` or `Yields`. Broader restatements (`This function returns …`, `The return value is …`) are **not** flagged. | The verb form is frequent (~24k monorepo files, [01_understand.md](01_understand.md) §3), unambiguous and fixed by deleting one word. Restatements are rarer, open-ended (no closed phrase list), and `The return value of callback …` can be a correct noun phrase. Listed in `known_problems`. |
| D2 | **Both section kinds, all four words.** `Yields:` / `Returns each row.` is flagged like `Returns:` / `Returns the row.` | Google treats `Returns:` and `Yields:` as one section (§3.8.3 "*Returns:* (or *Yields:* for generators)"). The wrong verb is as redundant as the right one, and the fix is the same. |
| D3 | **Every `Returns:` / `Yields:` section** of the docstring is checked, one diagnostic each (fixes G3). Headers are those of the shared parser: `Returns`, `Return`, `Yields`, `Yield`, case-insensitive, alone on their line (fixes G4). | The legacy linter's `break` was an accident of its loop, not a design. |
| D4 | **Only the opening word** of the section is checked: the first word of the first non-blank body line (after D5's type prefix). | Later sentences may legitimately start with a `Return…` word (Google's own `Returned keys are always bytes.`, `… Returns None on timeout.`). Keeps the rule exact on what it claims. |
| D5 | **Typed first line.** A leading Napoleon type prefix `<type>:` is skipped before taking the opening word, so `bool: Returns True if ready.` is flagged (fixes G2). A type prefix is the text before the first `:` of the line when it contains no whitespace outside brackets and the `:` is followed by whitespace or the end of the line (`dict[str, int]:` qualifies, `Note that:` does not). | Napoleon and pydoclint both read this form; without it the most explicit docstrings escape the rule. |
| D6 | **Case-insensitive, whole word.** `returns the alloc owner mdb.` is flagged; `Returned keys …` and `ReturnCode …` are not. | Matches the legacy linter; lowercase occurrences exist in monorepo. |
| D7 | **Noun reading exempt (E1).** A bare `Return` / `Yield` followed by `code`, `codes`, `value`, `values`, `type`, `types` or `status` is a noun phrase and is not flagged. `Returns` / `Yields` are always read as verbs. The list is fixed (see Q1). | Removes the legacy linter's measured false positives (`Return code based on …`, `return value passed through …`, [01_understand.md](01_understand.md) §3). `Returns`/`Yields` as plural nouns opening a Returns section (`Yields of each crop.`) is rare enough for `known_problems`. |
| D8 | **`Raises:`, `Args:` and other sections are out of scope.** | `Raises ValueError if …` is a malformed `Name: description` entry; its fix (`ValueError: If …`) and reason (pydoclint `DOC503` cannot match it) differ, so it would be another rule. In `Args:`, `callback: Returns the new value.` describes a callable parameter and is correct. |
| D9 | **The summary line is out of scope.** | Google endorses a `Returns …` summary for functions (§3.8.3 lines 2108–2111). |
| D10 | **The `@property` rule is a separate candidate, not part of this rule** (see §7). | Different construct (the summary line of a property, not a section), different reason (a property documents an attribute, Google lines 2081–2084), different trigger list (`Gets`, `Retrieves`, `Fetches` also apply). Fails the split test of [rule_design_guide.md](../rule_design_guide.md) §1. It is also not a legacy check, so it is outside this batch's scope. |
| D11 | **No broader "docstring section stutter" rule.** | The other stutters do not share a fix: `Args:` `name: The name.` needs a description of meaning, `Raises:` `Raises X` needs reformatting (D8). One template could not prescribe one canonical fix. |
| D12 | **Scope: every Python function with a docstring**: methods, nested and `async` functions, properties, `@override`, `@abstractmethod`, stubs, test files. Class and module docstrings are not read. | Wording is wrong wherever it appears. Google defines no `Returns:` for classes or modules (pydoclint `DOC302` already flags a class-level `Returns:`). |
| D13 | **Location: the opening body line** of the section (`PythonDocstring::line_node(line.index)`), falling back to the whole docstring as the helper does (fixes G5). | The editor marks the line to edit. |
| D14 | **Options: none besides the enforcement mode**, default `ban` (`RuleOptions::code_rule(())`). | Nothing to tune (Q1 covers the noun list). An explanation comment next to a docstring line adds nothing a reword would not. |

## 2. Exemptions (one mutation-tested `pass` case each)

| # | Exemption | `pass` case (sketch) | Mutation that must make it fail |
| :-- | :--- | :--- | :--- |
| E1 | Bare `Return`/`Yield` + noun from D7 | `Returns:` / `Return code of the process.` | Drop the noun-list check. |
| E2 | Word boundary: a longer word starting with the stem | `Returns:` / `Returned keys are bytes.` | Match the stem as a prefix. |
| E3 | Only the first body line | `Returns:` / `A dict of rows.` / `Returns an empty dict when nothing matches.` | Check every body line. |
| E4 | Only the first word of that line | `Returns:` / `True if ready; returns False otherwise.` | Search the whole first line for the words. |
| E5 | Summary line | `"""Returns the row from Bigtable as a tuple of strings."""` | Scan the whole docstring instead of sections. |
| E6 | Other sections | `Args:` / `callback: Returns the new value.` | Check every section kind. |

Fail cases to cover besides the basic one: `Yields:` with `Yields`, `Yields:` with `Returns` (D2), lowercase `returns` (D6), typed line `bool: Returns True if ready.` (D5), a second section in the same docstring (D3, two spans), a `Return:` header (D3), a method and a nested function (D12). The `{code}\n{code}` harness duplication is harmless: findings are per function and independent of other definitions.

## 3. Name

`returns-section-stutter`: names the flagged pattern, three words, no polarity, no library identifier ([naming_and_message_style_guide.md](../naming_and_message_style_guide.md) §1). "Stutter" is established vocabulary for repeating the enclosing name ([02_references.md](02_references.md) §4); "returns section" follows Google, where `Yields:` is a variant of the Returns section. Rejected: `redundant-returns-docstring` (judgement word, and Pylint's `redundant-returns-doc` W9008 means something else), `verb-led-returns-section` (accurate but less recognizable; acceptable fallback, Q5).

## 4. Message template

```rust
const TEMPLATE: ViolationTemplate = violation_template! {
    summary: "`{section}:` section of `{function}` starts with `{word}`.",
    rationale: "The section header already says that a value is returned, so the leading verb repeats it and the text describes the call again instead of the value the caller receives.",
    suggestion: "Remove the leading `{word}` so the section starts with a noun phrase that describes the value, such as `True if the device is ready.` or `A dict mapping keys to rows.`",
};
```

- `{section}` is the header as written (`Returns`, `Yields`, `Return`); `{word}` the opening word as written (`Returns`, `returns`). Both placeholders are **new** (Q2): the vocabulary has none for a docstring section or a prose word, and `{token}` is defined as part of an identifier.
- One language, so no per-language variants.

## 5. Documentation (`RuleDoc`) draft

- **summary**: "Flags docstring `Returns:` and `Yields:` sections that start with the verb `Returns` or `Yields`."
- **what_it_does**: Flags a `Returns:` or `Yields:` section of a function docstring whose first word is `Return`, `Returns`, `Yield` or `Yields`, in any case, including after a type prefix such as `bool:`. Only the opening word counts; the summary line and other sections are not checked, so a docstring that starts with `Returns …` and has no section is fine. `Return code …`, `Return value …` and similar noun phrases are not flagged.
- **why_is_this_bad**: Rendered docs read `Returns: Returns True if …`. Google style asks the section to describe the semantics of the returned value; a leading verb turns it into a second summary of the call.
- **known_problems**: Restatements that do not start with the verb (`This function returns …`, `The return value is …`) are not flagged. A noun reading outside the fixed list is flagged (`Return address of the peer.`). A header followed by text on the same line (`Returns: The count.`) is not parsed as a section and is not checked.
- **references**: Google Python Style Guide §3.8.3 (Functions and Methods).
- **examples**: Python, `Returns:` / `Returns True if the device is ready.` → `Returns:` / `True if the device is ready.`

## 6. Classification

| Facet | Value | Test ([tag_guide.md](../tag_guide.md) §2) |
| :--- | :--- | :--- |
| Topic | New `docstrings` topic, shared by the batch (Q3) | The rule inspects docstring content; no existing topic is about documentation. |
| Precision | `heuristic` | The word list is a proxy for "the verb": `Returns`/`Yields` used as plural nouns, and noun compounds outside the D7 list, are flagged although correct. |
| Consensus | `opinionated` | Ordinary correct-enough code: many codebases, including monorepo at scale, write `Returns: Returns …` and accept it. |
| Impacted quality | `maintainability` | No runtime mechanism; the cost is documentation readability. |

## 7. Related candidate (not implemented here): `@property` docstrings that start with a verb

For [ROADMAP.md](../../../ROADMAP.md), to be added by the lead if accepted (Q4):

- **Pattern**: the summary line of a `@property` / `functools.cached_property` getter starts with `Returns`, `Return`, `Gets`, `Get`, `Retrieves` or `Fetches` (`"""Returns the Bigtable path."""`).
- **Fix**: an attribute-style noun phrase (`"""The Bigtable path."""`).
- **Backing**: Google §3.8.3 lines 2081–2084, quoted in [02_references.md](02_references.md) §1. No external rule: Ruff `D401` explicitly lets properties use "a non-imperative summary line" but flags nothing.
- **Interaction**: a property with a `Returns:` section that starts with `Returns` would get both findings on different lines; pydoclint `DOC201` already notes that properties need no `Returns:` section.

## 8. Helper API

No change needed to `src/code_lint/ast/python/docstrings.rs`. The rule uses:

- `extract_function_signatures(file)` → `PythonFunctionSignature::{name, docstring()}`;
- `PythonDocstring::sections()` → `DocstringSection { kind, header, body }` filtered on `DocstringSectionKind::Returns | Yields`;
- `body.first()` → `DocstringLine { index, text }`;
- `PythonDocstring::line_node(index)` for the span.

The D5 type-prefix split is a rule-local private function unless another Batch D rule needs the `Returns` type; then it belongs on `DocstringSection` in `docstrings.rs` (lead's call, Q6).

## 9. Rust analogue (ROADMAP suggestion only)

None worth a rule. Rustdoc has no `Returns` section convention (the recognized headings are `# Examples`, `# Errors`, `# Panics`, `# Safety`), and the standard library's own convention for getters is a `Returns …` summary (`Vec::len`: "Returns the number of elements in the vector"), so neither this rule nor the property candidate transfers. Suggested ROADMAP note: *Not pursued for Rust: no `Returns` section convention in rustdoc.*

## 10. Open questions

- **Q1 (user).** Keep the D7 noun list fixed, or expose it as an `allowed` list option (`extend-allowed = ["address"]`)? Recommendation: fixed; [rule_design_guide.md](../rule_design_guide.md) §5 targets domain vocabulary, and this is English grammar.
- **Q2 (lead).** Add placeholders `{section}` (a docstring section header as written) and `{word}` (a prose word as written) to the style guide §3 and the vocabulary test. Other Batch D rules matching prose (the `Optional` check) may need `{word}` too.
- **Q3 (lead).** Create the `docstrings` topic for the batch (label, description, scope note).
- **Q4 (user).** Add the `@property` candidate (§7) to ROADMAP, or drop it?
- **Q5 (user).** Name: `returns-section-stutter` (recommended) or `verb-led-returns-section`?
- **Q6 (lead).** Type-prefix split: rule-local or shared in `docstrings.rs`?
