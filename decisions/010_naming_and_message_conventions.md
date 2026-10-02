# ADR 010: Naming and Message Conventions

## 1. Problem Statement

Names and messages grew rule by rule, with no written convention:

1. **Six rule-name grammars.** Across 26 rules: `no-` (14), `prefer-` (3), `enforce-` (1), `-enforced` (1), `max-` (1) and bare nouns naming the bad pattern (6). Some named the pattern (`no-typing-cast`), some the policy (`flat-scope-enforced`), some the fix (`prefer-tuple-unpacking`). `no-uncommented-suppress` read as a ban while its default mode was `require-explanation`.
2. **Three summary shapes.** Full sentences ("Multiline string literal is not wrapped in a dedent helper."), verb-less noun phrases ("Type cast call `typing.cast()`."), and policy statements ("Blanket suppression directives without rule names are banned."). One rule quoted code with `'…'`, the rest with backticks.
3. **Placeholder drift.** `{call}` and `{callee}` for the same thing; `{func}` and `{func_name}`; `{max}` for the option `max_assertions`; `{min_args}` for `min_positional_parameters`.
4. **Option keys whose meaning flipped.** For a deny list, `allowed` meant "remove from the default"; for an allow list, `banned` meant the same. Keys were snake_case in a TOML file whose rule names, tags, enum values and CLI flags were all kebab-case.
5. **Component names unrelated to modules.** `CodeSyntaxAdapters` was `code_lint::ast`, `CodeSemanticEngines` was `code_lint::semantic`, with a suffix vocabulary of Adapters, Engines, Engine, Contracts, Harness, Binaries and Primitives.

## 2. Research

We compared Ruff, Clippy, the rustc diagnostics guide, ESLint, Biome, Pylint, Semgrep, staticcheck, ShellCheck and the Google error-message guidance. They converge on: a name must read naturally in its suppression context; the main message is short, stands alone, and never contains the fix; code is always delimited; config keys follow the host format's idiom; list options have an "extend, don't replace" mechanism.

They diverge on two points, and we had to pick a side:

| Topic | Rust family (Ruff, Clippy, rustc) | JS family (ESLint, Biome) |
| :--- | :--- | :--- |
| Polarity prefix in names | Forbidden. The name is the bad thing, because it is read inside `allow(...)`. | Required (`no-`/`prefer-`, or Biome's `no`/`use`), because names are read in a config map where polarity is invisible. |
| Message casing | lowercase, no period, prefixed by `error:` | Sentence case, full sentence, period |

## 3. Decision

The full rules live in `docs/dev/naming_and_message_style_guide.md`. The choices and their reasons:

1. **Rule names follow the Rust family: the flagged pattern, no polarity prefix.** Our suppression and config contexts are `omni:ignore [name]` and `ignore = ["name"]`, which read exactly like `allow(name)`. `ignore no-sleep-in-tests` is a double negative; `ignore sleep-in-tests` is not. Names are concepts, not library identifiers, because most rules match a configurable list of calls.
2. **Messages follow the sentence family.** Our output prints the summary as its own line followed by `Rationale:` and `Suggestion:` lines that are already full sentences; a lowercase, period-less first line would be the odd one out. The summary is a declarative sentence stating the observed fact, and never the fix or the harm.
3. **One placeholder vocabulary**, with thresholds named after their option key.
4. **Keys are kebab-case** like every other TOML linter, and like everything else in our config. List options have two operations only, `<noun>` (replace) and `extend-<noun>` (add). The third operation was unused and was the source of the flipped meaning; if it returns, it is `remove-<noun>`, parsed into a `remove` field on `ListOverride` and applied after `extend`.
5. **A component is named after its module path.** `code_lint::ast` is `CodeLintAst`. The three root modules become three components (`Architecture`, `Diagnostic`, `Diff`) rather than one named exception.
6. **No backward compatibility.** Old names and keys are not accepted; the "renamed key" hints in `rule_declaration::options` are removed. The project has no external users yet.
7. **Every mechanical rule is a test.** `tests/registry.rs` checks names, templates and placeholders; `tests/architecture_conformance.rs` checks component names; the options and config tests check key shapes. Two of these checks are closed lists (suggestion verbs, placeholders). They are there so that the same thing is always called the same way, not to cap the vocabulary: a new rule whose clearest wording needs a new entry adds it, in the guide and the test together, with the reason in the commit. Clarity of a rule is never traded for fitting an existing list.

## 4. Consequences

* Every `.omnilint.toml` and every `omni:ignore [...]` directive written before this ADR is invalid and must be updated.
* `enforce-frozen-slots-dataclass` is split into `mutable-dataclass` and `unslotted-dataclass`: the two conditions have different exceptions (slots break multiple inheritance), so a user needs to disable them separately.
* Adding a rule now means passing the registry tests, which reject a name with a prefix, a summary without a verb, or a placeholder outside the vocabulary. The guide is the reference; the tests are the enforcement.
