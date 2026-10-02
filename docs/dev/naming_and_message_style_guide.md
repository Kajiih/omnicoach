# Naming and Message Style Guide

This guide fixes how rules, configuration keys, placeholders and architecture components are named, and how the three parts of a violation message are worded. The *design* of a rule (granularity, actionability, extensibility) is covered by `rule_design_guide.md`; this document is only about words.

Every hard rule below is enforced by a test (see §7), so the guide and the code cannot drift apart.

Two of those rules are closed lists: the suggestion verbs (§2.4) and the placeholder vocabulary (§3). They exist to keep the vocabulary coherent, not to freeze it. When the clearest wording for a new rule needs a verb or a placeholder that is not in the list, add it: extend the list in this guide and in the test, in the same change, and say in the commit message why no existing entry fit. Never pick a less precise word to stay inside the list.

The conventions follow Ruff and Clippy where the two agree, and the Google error-message guidance for tone. The reasons for each choice are in `decisions/010_naming_and_message_conventions.md`.

---

## 1. Rule names

A rule name is read in three places: `omni:ignore [name] -- reason`, `ignore = ["name"]` in `.omnilint.toml`, and `--explain name`. The first two read as *"ignore the bad thing"*, so the name **is** the bad thing.

| Rule | Do | Don't |
| :--- | :--- | :--- |
| Name the flagged pattern, never the fix or the policy. | `sleep-in-tests`, `nested-function`, `mutable-dataclass` | `prefer-events-in-tests`, `flat-scope-enforced`, `enforce-frozen-dataclass` |
| No polarity prefix or suffix. The suppression context already says "ignore"; `no-` makes a double negative. | `type-cast` | `no-typing-cast`, `banned-abbreviations`, `max-test-assertions` |
| Name the concept, not a library identifier. Most rules match a configurable list of calls, so an identifier in the name is wrong as soon as the list is extended. Use an identifier only when the rule targets exactly that API and no generic concept exists. | `suppressed-exception`, `sleep-in-tests`, `edit-of-described-commit` (the rule *is* about `jj edit`) | `contextlib-suppress`, `time-sleep-in-tests` |
| Singular noun phrase for one construct; plural only when the rule reports a class of items at once. | `single-letter-name`, `abbreviated-name` | `single-letter-variable-names` |
| kebab-case, at most four words. | `zero-sleep-in-tests` | `no-zero-duration-sleep-in-test-functions` |
| The scope qualifier `-in-tests` appears only when the rule is `TestsOnly` **and** the construct is fine outside tests. | `mock-in-tests` | `assertion-packing-in-tests` (packing is never fine) |

The enforcement mode (`ban` / `require-explanation`) is never part of the name: the same rule flags the same pattern in both modes, the mode only decides whether an adjacent comment excuses it.

The Rust `const` exposing a rule is always `RULE` (one rule per file) or the SCREAMING_SNAKE_CASE of the rule name (several rules per file). The file is the snake_case of the rule name.

---

## 2. Violation messages

A diagnostic prints three lines, and `--explain` prints the same three under *Message*:

```text
src/app.py:12:5: [sleep-in-tests] Test calls `time.sleep()`.
  Rationale: Sleeping for a fixed duration slows the suite and fails under load.
  Suggestion: Wait on an `asyncio.Event` or a queue, or advance an injected clock.
```

The three fields are orthogonal (`rule_design_guide.md` §2). This section fixes their form.

### 2.1 Common form

* Sentence case, full sentences, each ending with a period. One sentence for `summary`; one or two for `rationale` and `suggestion`.
* Present tense, active voice, no first person, no "please", no "sorry".
* Code, identifiers, calls, keys, paths and flags are always in backticks. Never single or double quotes.
* No judgement words: "banned", "forbidden", "discouraged", "illegal", "must". The rule's existence is the judgement.
* No "e.g." / "i.e."; write "such as" or "for example".
* Numbers, thresholds and defaults are placeholders, never literals, so the message stays true when the option changes.

### 2.2 `summary`: the fact

* A declarative sentence whose subject is the flagged construct and whose verb states what is observed. No verb-less noun phrases.
* States only what was matched. No cause, no consequence, no fix verb (`Use`, `Replace`, `Add`, `Rename`, `Remove`).
* Must make sense alone, in an editor gutter, without the other two lines.

| Do | Don't | Why |
| :--- | :--- | :--- |
| Test calls `time.sleep()`. | Wall-clock or async sleep call `time.sleep()` in test. | Noun phrase, no verb. |
| Function `load` is defined inside another function. | Nested function `load` hurts testability. | Rationale leaked into the summary. |
| `point` is read by index at positions 0 and 1. | `point` should be unpacked. | Fix leaked into the summary. |
| Suppression directive names no rule. | Blanket suppression directives without rule names are banned. | Policy statement, judgement word. |

### 2.3 `rationale`: the harm

* Names the concrete failure mode or maintenance cost. One or two sentences.
* Does not restate the construct and does not say how to fix it.
* Never normative ("must", "should"): it explains, it does not command.

### 2.4 `suggestion`: the fix

* Imperative mood, starts with a verb from the current list: `Wrap`, `Replace`, `Rename`, `Split`, `Add`, `Remove`, `Move`, `Pass`, `Wait`, `Destructure`, `Unpack`, `Insert`, `Spawn`, `Create`, `Load`, `Narrow`, `Assert`, `Inject`, `Specify`, `Verify`, `Extract`, `Synchronize`, `Access`. The list grows when a fix needs a verb that is not in it (see the introduction).
* One canonical replacement per language, idiomatic to that language. The base text is language-neutral; per-language variants replace it entirely.
* Does not repeat "instead of `<bad construct>`" and does not re-explain the harm.

### 2.5 Doc summary (`RuleDoc::summary`)

One sentence, third person, starting with `Flags` for pattern rules and `Requires` for rules whose fix is "add something": "Flags fixed-duration sleeps in tests.", "Requires a comment on every `suppress(...)` block."

---

## 3. Placeholders

Templates use a fixed vocabulary so the same thing has the same name in every message:

| Placeholder | Meaning | Rendered as |
| :--- | :--- | :--- |
| `{callee}` | The function, method or macro that was called | `` `{callee}()` `` or `` `{callee}!` `` |
| `{function}` | A function that is *defined* at the flagged site | `` `{function}` `` |
| `{class}` | A class, struct or enum defined at the flagged site | `` `{class}` `` |
| `{name}` | An identifier being named (variable, parameter, field) | `` `{name}` `` |
| `{suffix}`, `{token}` | The offending part of an identifier | `` `{suffix}` `` |
| `{rule}` | A rule name inside a suppression directive | `` `{rule}` `` |
| `{revision}` | A VCS revision | `` `{revision}` `` |
| `{count}` | The observed number | bare |
| `{max_assertions}`, `{min_positional_parameters}`, … | A threshold; always the snake_case of the option key | bare |

The vocabulary grows the same way as the verb list: when a message needs a thing none of these names, add a row here and an entry in the vocabulary test, in the same change. Do not reuse an existing placeholder for a different meaning to avoid adding one.

---

## 4. Option and configuration keys

`.omnilint.toml` is TOML, so keys follow the TOML ecosystem (Ruff, Clippy, Pylint): **kebab-case**. Values that are enums are kebab-case words.

| Kind | Form | Examples |
| :--- | :--- | :--- |
| Top level | kebab-case | `select`, `ignore`, `per-file-ignores`, `rules`, `context` |
| Context | kebab-case | `test-patterns` |
| Enforcement mode | key `enforcement-mode`, values `ban` / `require-explanation` | |
| Count | `max-<noun>` or `min-<noun>`, the noun being what is counted | `max-assertions`, `min-positional-parameters`, `min-positions`, `max-placeholders` |
| Deny list | `banned` replaces the default; `extend-banned` adds to it | `[rules.abbreviated-name] extend-banned = ["tmp"]` |
| Allow list | `allowed` replaces the default; `extend-allowed` adds to it | `[rules.bare-multiline-string.rust] extend-allowed = ["rule_test"]` |

There is no "remove from the default" key: to drop one default item, replace the list. If that becomes a recurring need, it will be a `remove-banned` / `remove-allowed` key mapped to the `remove` field `ListOverride` already has.

Inside the code the three list operations are called **replace**, **extend**, **remove**, everywhere (`ListOverride`, `--explain` rendering, docs). The Rust consts holding a rule's default list are `BANNED` or `ALLOWED`.

---

## 5. Architecture components

A component name is the PascalCase of its module path, nothing more: `code_lint::ast` → `CodeLintAst`, `command_lint::vcs` → `CommandLintVcs`, `test_utils` → `TestUtils`, `bin` → `Bin`. One module, one component; the three root primitives are `Architecture`, `Diagnostic` and `Diff`.

The description of a component says what the module does, not what the name already says.

---

## 6. Taxonomy, CLI and files

* Topic labels, facet values and synonyms are kebab-case nouns describing a subject, never a verb or a policy (`concurrency`, not `async-hygiene`).
* CLI flags and subcommands are kebab-case; binaries are kebab-case (`omni-code-lint`).
* Rust modules and files are snake_case.

---

## 7. What is enforced where

| Rule | Test |
| :--- | :--- |
| Rule names: kebab-case, no polarity prefix or suffix, at most four words, unique | `tests/registry.rs` |
| `RULE` / SCREAMING_SNAKE const and snake_case file match the rule name | `tests/registry.rs` |
| Summary: uppercase start, single sentence ending with `.`, no `'`/`"` quoting, no fix verb, no judgement word | `tests/registry.rs` |
| Rationale: ends with `.`, no "must" / "should", no fix verb at sentence start | `tests/registry.rs` |
| Suggestion: ends with `.`, starts with a verb from §2.4 | `tests/registry.rs` |
| Placeholders drawn from §3 | `tests/registry.rs` |
| Option and config keys kebab-case; count keys `max-` / `min-`; list keys `banned` / `extend-banned` / `allowed` / `extend-allowed` | `rule_declaration::options` tests, `config` tests |
| Component name is the PascalCase of the declaring module path | `tests/architecture_conformance.rs` |
