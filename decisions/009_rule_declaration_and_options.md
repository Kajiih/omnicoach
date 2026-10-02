# ADR 009: Rule Declaration and Rule Options

## 1. Problem Statement

Rule configuration grew in layers that did not know about each other:

1. **Three declarations per rule.** A rule exposed its detector struct, an inherent `CLASSIFICATION`, an inherent `DOC`, and was then listed in a registry entry repeating all three. The configuration keys it accepted were a fourth list, `RuleDoc::configuration: &[ConfigShape]`, mapped to key names by hand in `RuleCatalog`. Nothing tied the four together: a rule could read a key it never documented, and did.
2. **Config was stringly typed and read lazily.** `Config.rules` was a `HashMap<String, serde_json::Value>` re-parsed by every `effective_*` helper on every file, keyed by `self.name().0`. Parse errors were swallowed (`.ok().unwrap_or_default()`), so a typo such as `pyhton` or `max = 3` on a rule expecting `max_assertions` silently restored the default. Per-language tables used serde `flatten`, which rules out `deny_unknown_fields`.
3. **Enforcement mode was special-cased.** The runner applied `mode` to every code rule, yet only `no-uncommented-suppress` documented it, and the key did not say what it was a mode of.
4. **Defaults were repeated in prose.** `RuleDoc` text said "4 by default" or listed the banned items, so the text and the code could drift.

## 2. Decision

### 2.1. One declaration per rule

There are no detector traits (`Detector`, `CodeDetector`, `CommandDetector`) or zero-sized detector structs. Each rule is a single `const` struct literal holding its metadata, its typed options declaration, and its check function pointer:

```rust
pub const RULE: CodeRule<CountOption> = CodeRule {
    declaration: Declaration {
        name: RuleName("max-test-assertions"),
        template: &TEMPLATE,
        languages: &[SupportLang::Python, SupportLang::Rust],
        options: RuleOptions::code_rule(MAX_ASSERTIONS),
        classification: Classification { ... },
        doc: RuleDoc { ... },
    },
    target: RuleTarget::TestsOnly,
    check: check_file,
};
```

`Declaration<Options>`, its type-erased view `DeclaredRule`, and its constituent building blocks—options (`rule_declaration::options`), taxonomy (`rule_declaration::taxonomy`), and documentation (`rule_declaration::documentation`)—live together in a single component `RuleDeclaration => [FoundationPrimitives]`, re-exported from `crate::rule_declaration`. Project-level configuration (`Config`, `ContextConfig`, `compile_glob`) lives in a dedicated `Config => [FoundationPrimitives, RuleDeclaration]` component (`src/config.rs`), so rule contracts (`CodeRuleContracts`, `CommandRuleContracts`) and rule implementations (`CodeLintRules`, `CommandLintRules`) depend on `RuleDeclaration` and never reach `Config`.

`CodeRule<Options>` (in `code_lint::contract`), `CommandRule` (in `command_lint::contract`), and the four suppression audits (`Declaration` consts in `code_lint::suppression`) share `Declaration` for their name, template, languages, options, classification, and doc. Because `CodeRule<Options>` is parameterized by `Options`, `code_lint::contract` also defines a type-erased `AnyCodeRule` trait blanket-implemented once for every `CodeRule<Options>`. The registries (`CODE_RULES: &[&dyn AnyCodeRule]`, `SUPPRESSION_AUDITS: &[Declaration]`, `COMMAND_RULES: &[CommandRule]`) are flat slices of these consts, so a rule cannot be registered without every part and nothing is repeated at the registration site.

### 2.2. Options are declared as const values implementing `OptionsDeclaration`

An option is a `const` next to `RULE`, holding its key, its one-sentence doc and its per-language default:

```rust
const MAX_ASSERTIONS: CountOption = CountOption {
    key: "max_assertions",
    doc: "Maximum assertions allowed in one test function.",
    default: LanguageDefaults::new(4, &[]),
};
```

Two option structs exist in `rule_declaration`, and together with `()` and `(First, Second)` they implement `OptionsDeclaration`:

- `()` — no rule-specific options (`Resolved = ()`, `Param<'a> = ()`).
- `CountOption { key, doc, default: LanguageDefaults<usize> }` (`Resolved = usize`, `Param<'a> = usize`). Keys name what is counted (`max_assertions`, `min_positional_parameters`), never a bare `max` or `min`.
- `ListOption { kind: ListKind, doc, default: FilterListDefaults }` (`Resolved = HashSet<String>`, `Param<'a> = &'a HashSet<String>`). `ListKind::Deny` fixes the keys `banned`, `extend_banned`, `allowed`; `ListKind::Allow` fixes `allowed`, `extend_allowed`, `banned`. Because the kind fixes the keys, a rule declares at most one list; a registry test enforces it.
- `(First, Second)` — a pair of option declarations (`Resolved = (First::Resolved, Second::Resolved)`, `Param<'a> = (First::Param<'a>, Second::Param<'a>)`).

`RuleOptions<Options> { enforcement_mode: Option<LanguageDefaults<EnforcementMode>>, options: Options }` is the rule's whole surface under `[rules.<name>]` and `[rules.<name>.<language>]`. `RuleOptions::code_rule(options)` wraps `options` with `enforcement_mode` defaulting to `ban`; `RuleOptions::none()` rejects every key, including `enforcement_mode`, and is used by suppression audits and command rules, whose runners never honour a mode. `RuleOptions::declared(&self) -> DeclaredOptions` converts the typed `Options` into a `Vec<OptionSpec>` for config validation and `--explain`.

### 2.3. Enforcement mode is an ordinary option

The key is `enforcement_mode`. Every code rule declares a default (`ban`, except `no-uncommented-suppress` which defaults to `require-explanation`), and `--explain` documents it for every code rule. There is no rule-specific handling in the runner: `CodeRule::check_file` resolves the mode for the file and drops explained findings in `require-explanation`.

### 2.4. Validation happens once, at load

`rule_selection::parse_config` reads `[rules]` as a raw TOML table and, for each entry, looks the rule up in the registry (`ConfigError::UnknownRule`, with a did-you-mean) and calls `RuleOverrides::parse(name, &rule.options, rule.languages, value)`. The walker rejects, with the full key path:

- an undeclared key (`rules.max-test-assertions.max`: unknown key; did you mean `max_assertions`?). The suggestion uses edit distance first, then whole-word containment, so a renamed key (`mode` → `enforcement_mode`) is suggested too;
- a value of the wrong type (`expected a non-negative integer, found "5"`);
- a table for a language the rule does not analyze;
- `enforcement_mode` on a rule that declares none.

The result is `Config.rule_overrides: HashMap<RuleName, RuleOverrides>`, typed values and nothing else. No string-keyed lookup survives past load.

### 2.5. Compile-time linking between declared options and `check`

`CodeRule<Options>::check` has the type `for<'a> fn(&Self, &Path, &ParsedFile, Options::Param<'a>) -> Vec<Diagnostic>`. `CodeRule::check_file` resolves `self.declaration.options.options.resolve(file.lang(), overrides)` once per file and passes `Options::as_param(&options)` directly to `self.check`:

- a rule with `Options = ()` receives `(): ()` by value;
- a rule with `Options = CountOption` receives `usize` by value;
- a rule with `Options = (CountOption, CountOption)` receives `(usize, usize)` by value;
- a rule with `Options = ListOption` receives `&HashSet<String>` by reference.

Precedence is language table, then rule table, then the declared per-language default; list additions and removals apply from every layer. Because the check function receives only `Options::Param<'a>`, a rule cannot read an option it did not declare, and a signature mismatch fails to compile with `E0308` at `check: check_file`.

### 2.6. `--explain` renders the declaration

`RuleCatalog` renders the `## Configuration` section from `DeclaredOptions`: one bullet per key with its type, per-language defaults and doc, under the rule's `[rules.<name>]` heading. Rule prose no longer states defaults. Rules whose `RuleOptions` is `none()` get no section.

## 3. Consequences

- Runtime, validation and `--explain` read the same const, so keys, types and defaults cannot drift between them.
- A rule cannot read an undeclared option: `check` receives only the resolved value of the `Options` type declared in `RULE`.
- Zero-sized detector structs, `Detector`, `CodeDetector` and `CommandDetector` traits, and per-rule `impl` blocks are gone; every rule is a single `const` struct literal plus a private `check_file` / `check_command` function.
- Breaking for users: `mode` → `enforcement_mode`; `max` / `min` → explicit names; `enforcement_mode` on an audit or command rule is now an error; unknown top-level or `[context]` keys are rejected.
- A JSON Schema generated from the declarations stays on the roadmap.
