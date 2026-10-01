# Tag Guide

> [!NOTE]
> Normative contributor guide for Omni's rule taxonomy and selection system ([ADR 007](../../decisions/007_rule_taxonomy_and_selection.md)). Decisions are cited as D*n* from the T1 exploration records in [rule_docs_and_tags/t1/](rule_docs_and_tags/t1/).

This guide is for contributors who **classify a rule** or **add a tag**. Every structural rule below is enforced by the compiler or a test (§6). The guide explains the *why* and gives the yes/no questions that tests cannot answer.

---

## 1. The model in one screen

A rule is described by **facets**. Each facet answers one question. Its **values** are the labels you can put in `select` / `ignore`.

| Facet (display label) | Question | Values | How it is set |
|---|---|---|---|
| **Topic** | What construct, API or domain does the rule inspect? | the topic tree (§5) | Declared, **one or more**, most specific only |
| **Precision** | Can it flag correct code? | `exact`, `heuristic` | Declared, **exactly one** |
| **Consensus** | Would reasonable people disagree with it? | `opinionated`, `unopinionated` | Declared, **exactly one** |
| **Impacted quality** | What software quality suffers when it is violated? | `reliability`, `maintainability` (from ISO/IEC 25010; §2.4) | Declared, **exactly one** |
| **Languages** | Which languages does it check? | `python`, `rust` | Derived from `supported_languages()` (**1+ for code rules, 0 for command rules**) |
| **Analyzed input** | Code or shell commands? | `code`, `command` | Derived from the rule registry (**exactly one**; suppression audits derive `code`) |
| **File scope** | Tests only, sources only? | `tests-only`, `source-only` | Derived from `RuleTarget` (**0 or 1**; none when the rule runs on all files) |

Rules of the namespace:
- **Facet labels are never selectors** (D19). `select = ["precision"]` or `select = ["impacted-quality"]` is an error that lists the values.
- **Every label is globally unique** (DI5): rule names, topic labels, facet values and synonyms share one namespace. That is why values are bare (`heuristic`, not `precision:heuristic`).
- **Derived facets cannot be declared.** The rule has no field for them.
- **Behaviour never depends on a tag** (DI4, D37). `Rule` exposes no tag getter, suppression audits have their own contract, and neither rules nor runners may depend on `RuleSelection`.

---

## 2. Classifying a rule

A rule's whole classification is one declaration next to the rule struct, in the rule's own file:

```rust
impl NoSleepInTests {
    /// The rule's declared facets (ADR 007).
    pub(crate) const CLASSIFICATION: Classification = Classification {
        topics: &[Topic::TEST_TIMING],
        precision: Precision::Exact,
        consensus: Consensus::Unopinionated,
        impacted_quality: ImpactedQuality::Reliability,
    };
}
```

The registry (`CODE_RULES`, `SUPPRESSION_AUDITS` or `COMMAND_RULES`) lists the rule as a `Rule { detector, classification, doc }`, so a rule cannot be registered without it. Forgetting a field, giving a field two values, or declaring a derived facet does not compile.

### 2.1 Topics: most specific, at least one

- List **only the most specific topics** (R8). Ancestors are implied: `TestTiming` already makes the rule a `testing` rule. Listing both fails a test that tells you which one to remove.
- A rule may carry **several topics on different branches** when it is genuinely about both subjects (`no-logging-error-in-except` is `logging` and `error-handling`).
  - Consequence (D18): with `ignore = ["error-handling"]` the rule is off even if `logging` is selected. Ignore wins when branches disagree.
- **Topic = what is inspected; Impacted quality = what breaks** (D38).
  - A topic names a **code construct, API or domain** (`durations`, `async`, `jj`, or future `sql`, `subprocess`, `crypto`).
  - A software quality (`reliability`, `maintainability`, `security`, `performance-efficiency`, `safety`) names the **consequence of a violation** and belongs in *Impacted quality* (§2.4), **never in Topic**.
  - For example, a SQL-injection rule has topic `sql` and impacted quality `security`; an N+1 query rule has topic `sql` and impacted quality `performance-efficiency`.
- **Tests-only is not a topic.** "Runs only on test files" is the derived *File scope*. `testing` means "about test-code practice".

### 2.2 Precision: `exact` or `heuristic`

> **Test:** does the rule's *design* use a syntactic proxy for the target construct or its exemption boundary, so that it can flag code that is correct? If yes → `heuristic`.

- A threshold that can only cause **missed** findings does not make a rule heuristic. Only a proxy that **adds** findings does.
- **Design proxy vs. rule bug:** an incidental implementation defect or single-file AST lack of import resolution (tracked under *Rule Defects* in `ROADMAP.md`) is a bug to fix, not a reason to classify the rule as `heuristic`.
- `exact`: `no-typing-cast` (it flags `cast` calls, which is what it claims), `single-letter-variable-name` (the claim *is* "the name is one character").
- `heuristic`: `banned-abbreviations` (segment matching flags real words like `cat`), `max-test-assertions` (a count stands in for "several behaviours"), `no-unstructured-task-creation` (it also flags tasks that are stored and awaited), `no-env-in-functions` (guesses entrypoint boundaries from a function-name allowlist), `prefer-tuple-unpacking` (integer subscripting `x[0]` is a proxy for indexing a tuple).

### 2.3 Consensus: `opinionated` or `unopinionated`

> **Test (D23):** name one ordinary situation where the flagged code is correct and appropriate. If you can → `opinionated`.

- The meaning is "reasonable people disagree" (DEF2). The test is how to check it consistently.
- A real failure mode does **not** make a rule unopinionated. `no-mocks-in-tests` prevents false-green tests, yet whole testing schools use mocks.
- `unopinionated`: `no-sleep-in-tests`, `unknown-suppression-rule`.
- `opinionated`: `no-mocks-in-tests`, `prefer-timedelta-over-seconds` (`timeout_ms` is standard at config boundaries), `enforce-frozen-slots-dataclass` (mutable dataclasses are ordinary Python).
- Expect most Omni rules to be opinionated. Omni exists to go beyond the default Ruff and Clippy sets.

### 2.4 Impacted quality (D38)

Each rule declares **one** primary quality from the ISO/IEC 25010:2023 product-quality model. The code enum (`ImpactedQuality`) contains **only values used by at least one rule**, so no selector can match zero rules; adding another ISO value when the first rule needs it is a one-line enum change.

| ISO/IEC 25010 characteristic | Status | Meaning for a lint rule |
|---|---|---|
| `reliability` | **In enum** | Prevents wrong runtime behaviour, flaky/false-green tests, broken suppressions, or unintended command state/history mutation |
| `maintainability` | **In enum** | Keeps code understandable, diagnosable and safe to change |
| `security` | Allowed when needed | Prevents an exploitable vulnerability or exposure of sensitive data |
| `performance-efficiency` | Allowed when needed | Prevents wasted time, memory or I/O |
| `functional-suitability`, `compatibility`, `interaction-capability`, `flexibility`, `safety` | Allowed when needed | Standard ISO/IEC 25010 characteristics; admit to the enum only with a concrete rule and a yes/no boundary test |

> **Test (`reliability` vs `maintainability`):** does the flagged code or command have a *mechanism that leads to wrong behaviour*: a product bug, a flaky or false-green test, a suppression that silently does something unintended, or a command that mutates/corrupts state or history in place? If yes → `reliability`.

- Readability, searchability, refactoring-brittleness and diagnosability costs do **not** count as `reliability`. Without this boundary every rule qualifies.
- `reliability`: `no-typing-cast` (hides a type error until runtime), `unused-suppression` (will hide a future real violation), `no-mocks-in-tests` (false-green tests when real collaborator diverges), `no-zero-sleep-in-tests` (relies on timer-subsystem side effects for scheduler yielding), `no-edits-on-described-commits` (silently rewrites a described commit).
- `maintainability`: `single-letter-variable-name`, `max-test-assertions` (a worse failure report, but recoverable by re-running), `no-mock-assertions` (asserting on call wiring couples the test to implementation details, making refactors brittle rather than false-green).

### 2.5 What goes in the rule's docs instead

- **External backing** (a PEP, the Rust API Guidelines, a Clippy lint that is not on by default): write it in the rule's *References* section. It is not a facet. Almost every Omni rule would say "none".
- **The failure mode**: write it in the *rationale*. It justifies the rule; it does not decide consensus.

---

## 3. Selecting rules (what users see)

- `select` absent → every rule is on. `select = []` → none.
- A topic selects its descendants: `select = ["vcs"]` includes `jj` rules.
- **Nearest wins along one branch, ignore wins across branches** (D18):
  - `select = ["jj"]`, `ignore = ["vcs"]` → jj rules on, other vcs rules off.
  - `select = ["testing"]`, `ignore = ["test-doubles"]` → `no-mock-assertions` (on both `test-assertions` and `test-doubles`) is off.
- **A rule name beats any tag** (D15): `ignore = ["testing"]`, `select = ["no-sleep-in-tests"]` keeps that one rule.
- `per_file_ignores` only removes rules, after the main selection.
- **Errors, never silence:**
  - an unknown label (with "did you mean");
  - a facet label;
  - the same selector in `select` and `ignore` (synonyms count as the same).

---

## 4. Adding or changing a topic

### 4.1 Admission (D25)

A topic is admitted when **all** of these hold. The number of rules it has is **not** a criterion: a one-rule topic is fine.

1. **It names a subject**: a construct, API or domain (`durations`, `async`, `jj`). It is not a quality every rule could claim (`style`, `safety`, `readability`), and not a facet value.
2. **It passes the all-and-some test under its parent** (R6): *all* rules that deserve the child also deserve the parent, now and for plausible future rules.
   - ✅ `jj → vcs`: every jj rule is a vcs rule.
   - ❌ `naming → style`: `prefer-timedelta-over-seconds` is a naming rule motivated by unit bugs, not taste.
   - "Related to" is not enough. Use a *see also* note instead of a parent link.
3. **It has a one-line `description` (`Topic::description`) and a `scope_note` (`Topic::scope_note`)** that says what it includes and excludes, and names neighbours with a similar word (`error-handling` includes `contextlib.suppress`; `suppression-directives` covers `omni:` comments only).
4. **Its label is kebab-case, unabbreviated where practical, and unique** across rules, topics, values and synonyms.
5. **It has at least one rule.** An empty topic is a selector that silently matches nothing.

### 4.2 Structure

- **A single-spot `const` struct** (D32): each topic is declared once as an associated `const` on `Topic` (`Topic { label, parent, description, scope_note, synonyms }`).
- **A tree** (D16, D33): one parent at most (`parent: Option<&'static Topic>`), read by callers through `ancestors()` and `path()`. Several parents (a DAG) may come later as a data change.
- **Compile-time acyclicity** (D34): because topics are `const`s referencing `Option<&'static Topic>`, any cycle in `parent` links fails to compile with `E0391` (`cycle detected when const-evaluating`).
- **Topics only**: a topic's parent is a topic, never a facet value.

### 4.3 Synonyms

- A synonym must be a **true equivalent** (`jujutsu` = `jj`). Narrower or broader words are not synonyms (`mocks` is narrower than `test-doubles`).
- Synonyms are accepted in config and resolved to the canonical label. Output always shows the canonical label.

### 4.4 Renaming or splitting

There are no aliases for old labels (A1, D24): renaming a topic breaks configs that name it, and the unknown-label error with "did you mean" is the migration path. Keep at least the old granularity (D22): a split must leave every old distinction selectable.

---

## 5. Current topic tree

| Topic | Parent | Synonyms | Description (`Topic::description`) | Scope note (`Topic::scope_note`: includes / excludes) |
|---|---|---|---|---|
| `naming` | — | | How identifiers are named. | Identifier names. Not string contents, file names, formatting or layout. |
| `abbreviated-names` | naming | | Too short or cryptic names: abbreviations and single letters. | Abbreviations and single-letter identifiers. Not type or unit suffixes. |
| `type-encoded-names` | naming | | Names encoding a type, container or unit. | Type, container or unit encoded in the name (`users_dict`, `timeout_secs`). Not names that are just short. |
| `testing` | — | | Practices specific to test code. | How test code is structured and written. Not production code, even when tested. |
| `test-assertions` | testing | | Count, shape and granularity of test assertions. | Number and shape of test assertions. Not production `assert` or what the test exercises. |
| `test-doubles` | testing | | Mocks, fakes, stubs, spies and monkeypatching. | Replacing collaborators in tests and asserting on those replacements. Not fixtures that only build data. |
| `test-timing` | testing | | Sleeps, clocks and timeouts in tests. | Sleeping or waiting in tests. Not production retries or duration representation (see `durations`). |
| `static-typing` | — | | What the static type checker can see and verify. | What static type checking can prove. Not runtime validation or dataclass mutability (see `record-types`). |
| `type-checker-bypass` | static-typing | | Code that overrides or routes around the type checker. | Casts and dynamic access (`cast`, `getattr`) that hide types from the checker. Not `omni:` directives. |
| `positional-meaning` | — | | Meaning carried by position instead of a name. | Meaning carried by argument order or tuple position instead of a name. Not named-field access. |
| `positional-indexing` | positional-meaning | | Reading sequence elements by literal index instead of unpacking. | Access by literal index (`t[0]`) where destructuring would name the parts. Not loops over indices or slicing. |
| `vcs` | — | `version-control` | Version-control operations and history. | Commands and workflows of any version-control system. Not CI or code review. |
| `jj` | vcs | `jujutsu` | Rules specific to Jujutsu. | Commands and workflows specific to Jujutsu. Not generic VCS behaviour. |
| `durations` | — | | How spans of time are represented. | Representing lengths of time and their units. Not sleeping/waiting (see `test-timing`) or wall-clock dates. |
| `record-types` | — | | Declaring named records (`dataclass`, `NamedTuple`, `struct`). | Field-bundle declarations, mutability and slots. Not enums or protocols. |
| `literals` | — | | How literal values are written in code. | Writing string and number literals (multiline strings, magic numbers). Not identifiers or formatting APIs. |
| `complexity` | — | | Structural size and nesting of code units. | Nesting depth and scope structure. Not naming or "readability" in general. |
| `global-state` | — | | Process-wide state read or written implicitly. | Reading or writing ambient process-wide state (environment variables, globals). Not file or network I/O. |
| `error-handling` | — | | Raising, catching, swallowing and reporting errors. | Exceptions and `Result`s, including `contextlib.suppress`. Not `omni:` directives (see `suppression-directives`). |
| `logging` | — | | Use of logging APIs. | Log calls and their arguments. Not `print` or metrics. |
| `async` | — | | `async`/`await`, tasks and event loops. | Coroutines, tasks and their lifetimes. Not OS threads. |
| `suppression-directives` | — | | Hygiene of `omni:` suppression comments. | `omni:` directives that silence Omni. Not `contextlib.suppress` (see `error-handling`). |

---

## 6. Enforcement map

Compiler checks are pinned by `compile_fail` doctests on `Classification` in `src/rule_taxonomy.rs`. The named tests live in `src/rule_selection/taxonomy.rs` unless stated otherwise. Design rationale: [ADR 007](../../decisions/007_rule_taxonomy_and_selection.md) (D33–D38).

| Guide rule | Mistake | Caught by |
|---|---|---|
| §2: each single-valued facet has exactly one value | M1, M2 | Compiler (`E0063` missing / `E0062` duplicate field) |
| §2.1: at least one topic | M3 | Test `every_rule_has_a_topic` |
| §2.1: most specific topics only | M4 | Test `no_rule_lists_a_topic_with_its_ancestor_or_twice` |
| §2.1: topics are topics, not facet values | M5 | Compiler (`E0308` type mismatch) |
| §1: derived facets cannot be declared | M6 | Compiler (`E0560` no such field) |
| §2: topics exist | M7 | Compiler (`E0599` no such associated `const`) |
| §4.2: a tree without cycles | M8 | Compiler (`E0391` `const` evaluation cycle on `parent: Option<&'static Topic>`, D34) |
| §1, §4.1.4: labels are globally unique, `kebab-case`, and not facet labels | M9 | Test `labels_are_globally_unique_and_not_facet_labels` |
| §2.4, §4.1.5: every topic and facet value has at least one rule | M10 | Test `every_tag_has_a_rule` + `dead_code` lint on unused `pub(crate) const` topics |
| §4.2: a topic's parent is a topic | M11 | Compiler (`E0308` type mismatch) |
| §1: behaviour never depends on a tag | M13 | Compiler (`Rule` has no tag getter; separate suppression-audit contract) + `test_runners_never_read_the_taxonomy` in `tests/architecture_conformance.rs` (D37) |
| §4.1.3: every tag has a description and every topic has a scope note | — | `missing_docs` lint (facet values) + compiler (`E0063` missing struct field on `Topic`) + test `every_topic_is_documented` (non-empty); **Review** (meaning) |
| §2.2–§2.4, §4.1.1–4.1.2: the yes/no questions | — | **Review.** A test cannot judge meaning; the worked examples are the reference. |
