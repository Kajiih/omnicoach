# ADR 009: Rule Declaration and Rule Options

## 1. Problem Statement

Rule configuration grew in layers that did not know about each other:

1. **Three declarations per rule.** A rule exposed its detector struct, an inherent `CLASSIFICATION`, an inherent `DOC`, and was then listed in a registry entry repeating all three. The configuration keys it accepted were a fourth list, `RuleDoc::configuration: &[ConfigShape]`, mapped to key names by hand in `RuleCatalog`. Nothing tied the four together: a rule could read a key it never documented, and did.
2. **Config was stringly typed and read lazily.** `Config.rules` was a `HashMap<String, serde_json::Value>` re-parsed by every `effective_*` helper on every file, keyed by `self.name().0`. Parse errors were swallowed (`.ok().unwrap_or_default()`), so a typo such as `pyhton` or `max = 3` on a rule expecting `max_assertions` silently restored the default. Per-language tables used serde `flatten`, which rules out `deny_unknown_fields`.
3. **Enforcement mode was special-cased.** The runner applied `mode` to every code rule, yet only `no-uncommented-suppress` documented it, and the key did not say what it was a mode of.
4. **Defaults were repeated in prose.** `RuleDoc` text said "4 by default" or listed the banned items, so the text and the code could drift.

## 2. Decision

### 2.1. One declaration per rule

Each rule file exposes exactly one item:

```rust
pub const RULE: Rule<dyn CodeDetector> = Rule {
    detector: &MaxTestAssertions,       // private struct
    classification: Classification { ... },
    doc: RuleDoc { ... },
    options: RuleOptions { options: &[OptionSpec::Count(&MAX_ASSERTIONS)], ..RuleOptions::CODE_RULE },
};
```

`Rule<D>` lives in a new component, `RuleDeclaration => [CoreVocabulary, RuleTaxonomy, RuleDocumentation]`. `RuleTaxonomy` is pure classification again. Registries are flat lists of these consts (`&[max_test_assertions::RULE, ...]`), so a rule cannot be registered without all four parts, and nothing is repeated at the registration site. Detector structs are private; the only public path to a rule is its `RULE`.

### 2.2. Options are declared as const handles

An option is a `const` next to `RULE`, holding its key, its one-sentence doc and its per-language default:

```rust
const MAX_ASSERTIONS: CountOption = CountOption {
    key: "max_assertions",
    doc: "Maximum assertions allowed in one test function.",
    default: LanguageDefaults::new(4, &[]),
};
```

Two kinds exist, in `core`:

- `CountOption { key, doc, default: LanguageDefaults<usize> }`. Keys name what is counted (`max_assertions`, `min_positional_parameters`), never a bare `max` or `min`.
- `ListOption { kind: ListKind, doc, default: FilterListDefaults }`. `ListKind::Deny` fixes the keys `banned`, `extend_banned`, `allowed`; `ListKind::Allow` fixes `allowed`, `extend_allowed`, `banned`. Because the kind fixes the keys, a rule declares at most one list; a registry test enforces it.

`RuleOptions { enforcement_mode: Option<LanguageDefaults<EnforcementMode>>, options: &[OptionSpec] }` is the rule's whole surface under `[rules.<name>]` and `[rules.<name>.<language>]`. `RuleOptions::CODE_RULE` is a code rule with no options of its own (`enforcement_mode` defaults to `ban`); `RuleOptions::NONE` rejects every key, including `enforcement_mode`, and is used by suppression audits and command rules, whose runners never honour a mode.

The option types live in `core` rather than in `RuleDeclaration` because the detector contract (`CodeDetector::check_file`, in `CodeRuleContracts`) takes the resolved options, and that contract must not depend on taxonomy or documentation.

### 2.3. Enforcement mode is an ordinary option

The key is `enforcement_mode`. Every code rule declares a default (`ban`, except `no-uncommented-suppress` which defaults to `require-explanation`), and `--explain` documents it for every code rule. There is no rule-specific handling in the runner beyond reading `options.enforcement_mode()`.

### 2.4. Validation happens once, at load

`rule_selection::parse_config` reads `[rules]` as a raw TOML table and, for each entry, looks the rule up in the registry (`ConfigError::UnknownRule`, with a did-you-mean) and calls `RuleOverrides::parse(name, &rule.options, rule.languages, value)`. The walker rejects, with the full key path:

- an undeclared key (`rules.max-test-assertions.max`: unknown key; did you mean `max_assertions`?). The suggestion uses edit distance first, then whole-word containment, so a renamed key (`mode` → `enforcement_mode`) is suggested too;
- a value of the wrong type (`expected a non-negative integer, found "5"`);
- a table for a language the rule does not analyze;
- `enforcement_mode` on a rule that declares none.

The result is `Config.rule_overrides: HashMap<RuleName, RuleOverrides>`, typed values and nothing else. No string-keyed lookup survives past load.

### 2.5. Rules read through `ResolvedOptions`

`CodeDetector::check_file(&self, path, file, options: &ResolvedOptions<'_>)`. The runner builds the view once per rule and file from the language, `RULE.options` and the rule's overrides. Reads are by handle: `options.count(&MAX_ASSERTIONS)`, `options.list(&BANNED)`, `options.enforcement_mode()`. Precedence is language table, then rule table, then the declared per-language default; list additions and removals apply from every layer. `CommandDetector::check_command` no longer takes a `Config`.

### 2.6. `--explain` renders the declaration

`RuleCatalog` renders the `## Configuration` section from `RuleOptions`: one bullet per key with its type, per-language defaults and doc, under the rule's `[rules.<name>]` heading. Rule prose no longer states defaults. Rules whose `RuleOptions` is `NONE` get no section.

## 3. Consequences

- Runtime, validation and `--explain` read the same const, so keys, types and defaults cannot drift between them.
- Per rule: one line (`options: RuleOptions::CODE_RULE`) for a rule without options; a 5-line handle plus a 4-line `options` block for a count; a 9-line handle plus the block for a list.
- A rule can still read a handle it did not list in `RULE.options`. That compiles; `ResolvedOptions` catches it with a `debug_assert` that every rule's own tests exercise, and a release build would fall back to the handle's default. Closing this gap at compile time (typing `Rule` by its options declaration) is the planned follow-up.
- `RuleDoc::configuration` and `ConfigShape` (ADR 008 §2.2), the `effective_*` helpers, `ThresholdConfig`, `DenyListConfig`, `AllowListConfig`, `EnforcementConfig`, `DynamicRuleConfig` and `Config.rules` are removed.
- Breaking for users: `mode` → `enforcement_mode`; `max` / `min` → explicit names; `enforcement_mode` on an audit or command rule is now an error. Each case fails loudly with a suggestion.
- Rejecting unknown top-level keys and a JSON Schema generated from the declarations stay on the roadmap.
