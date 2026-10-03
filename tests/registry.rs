//! Centralized registry integrity and uniqueness validation tests.

// Workaround for rust-lang/rust-clippy#13981 so clippy.toml `allow-*-in-tests` applies to the whole file.
#![cfg(test)]

use omni::code_lint::rules::CODE_RULES;
use omni::code_lint::suppression::SUPPRESSION_AUDITS;
use omni::command_lint::rules::COMMAND_RULES;
use omni::diagnostic::LanguageText;
use omni::rule_declaration::{
    Declaration, DeclaredOptions, DeclaredRule, EnforcementMode, OptionSpec, SUPPORTED_LANGUAGES,
    is_kebab_case, support_lang_name,
};
use rstest::rstest;
use std::collections::HashSet;

fn validate_rule(rule: &DeclaredRule) {
    let name = rule.name.0;

    let template = rule.template;
    for field in [template.summary, template.rationale, template.suggestion] {
        for text in std::iter::once(field.base).chain(
            field
                .overrides
                .iter()
                .map(|(_, override_text)| *override_text),
        ) {
            let trimmed = text.trim();
            assert!(
                !trimmed.is_empty(),
                "Rule {name} has an empty template field"
            );
            let is_bare_placeholder = trimmed.starts_with('{')
                && trimmed.ends_with('}')
                && !trimmed[1..trimmed.len() - 1].contains(['{', '}']);
            assert!(
                !is_bare_placeholder,
                "Rule {name} template field '{trimmed}' must not be a bare placeholder pass-through"
            );
        }
        let mut seen_langs = HashSet::new();
        for (lang, _) in field.overrides {
            assert!(
                seen_langs.insert(lang),
                "Rule {name} has duplicate template override for {lang:?}"
            );
            assert!(
                rule.languages.contains(lang),
                "Rule {name} declares template override for {lang:?}, which is not in its languages"
            );
        }
    }
}

fn validate_option_languages<T>(
    name: &str,
    rule_languages: &[ast_grep_language::SupportLang],
    label: &str,
    entries: &[(ast_grep_language::SupportLang, T)],
) {
    if rule_languages.len() <= 1 {
        assert!(
            entries.is_empty(),
            "Single-language rule {name} must declare its `{label}` default in `base`, not per-language"
        );
    }
    let mut seen_langs = HashSet::new();
    for (lang, _) in entries {
        assert!(
            seen_langs.insert(lang),
            "Rule {name} has duplicate `{label}` override for {lang:?}"
        );
        assert!(
            rule_languages.contains(lang),
            "Rule {name} declares `{label}` override for {lang:?}, which is not in its languages"
        );
    }
}

fn validate_options(rule: &DeclaredRule) {
    let name = rule.name.0;
    let options = &rule.options;
    if rule.languages.is_empty() {
        assert!(
            options.keys().is_empty(),
            "Rule {name} has no languages and must not declare options"
        );
    }
    let lists = options
        .options
        .iter()
        .filter(|option| matches!(option, OptionSpec::List(_)))
        .count();
    assert!(
        lists <= 1,
        "Rule {name} declares {lists} list options; at most one fits the list keys"
    );
    let reserved: Vec<&str> = std::iter::once(EnforcementMode::KEY)
        .chain(
            SUPPORTED_LANGUAGES
                .iter()
                .map(|&language| support_lang_name(language)),
        )
        .collect();
    let mut keys = HashSet::new();
    for key in options.options.iter().flat_map(|option| option.keys()) {
        assert!(
            !reserved.contains(&key),
            "Rule {name} declares the reserved key `{key}`"
        );
        assert!(
            keys.insert(key),
            "Rule {name} declares the key `{key}` twice"
        );
    }
    if let Some(default) = options.enforcement_mode {
        validate_option_languages(
            name,
            rule.languages,
            EnforcementMode::KEY,
            default.overrides,
        );
    }
    for option in &options.options {
        match option {
            OptionSpec::Count(count) => {
                validate_option_languages(name, rule.languages, count.key, count.default.overrides);
            }
            OptionSpec::List(list) => {
                for (key, entries) in [
                    (list.kind.extend_key(), list.default.extend),
                    (list.kind.remove_key(), list.default.remove),
                ] {
                    validate_option_languages(name, rule.languages, key, entries);
                    assert!(
                        entries.iter().all(|(_, items)| !items.is_empty()),
                        "Rule {name} has an empty `{key}` slice in `FilterListDefaults`"
                    );
                }
            }
        }
    }
}

#[rstest]
#[case::code_rules(CODE_RULES.iter().map(|rule| rule.declaration()).collect())]
#[case::suppression_audits(SUPPRESSION_AUDITS.iter().map(Declaration::declared).collect())]
#[case::command_rules(COMMAND_RULES.iter().map(|rule| rule.declaration.declared()).collect())]
fn test_registry_integrity(#[case] rules: Vec<DeclaredRule>) {
    for rule in &rules {
        validate_rule(rule);
        validate_options(rule);
    }
}

#[test]
fn test_code_rules_declare_supported_languages() {
    let code_rules = CODE_RULES.iter().map(|rule| rule.declaration());
    let audits = SUPPRESSION_AUDITS.iter().map(Declaration::declared);
    for rule in code_rules.chain(audits) {
        assert!(
            !rule.languages.is_empty(),
            "Code rule {} must declare at least one supported language",
            rule.name
        );
    }
}

/// Code rule examples are executed by `rule_test!`, so each declared language gets exactly one.
#[test]
fn test_code_rules_document_one_example_per_language() {
    for rule in CODE_RULES.iter().map(|rule| rule.declaration()) {
        let example_languages: Vec<_> = rule
            .doc
            .examples
            .iter()
            .map(|example| example.language)
            .collect();
        assert_eq!(
            example_languages.len(),
            rule.languages.len(),
            "Rule {} must document exactly one example per declared language",
            rule.name
        );
        assert!(
            rule.languages
                .iter()
                .all(|language| example_languages.contains(language)),
            "Rule {} must document an example for each declared language, got {example_languages:?}",
            rule.name
        );
    }
}

/// Nothing executes the examples of suppression audits and command rules, so they declare none.
#[test]
fn test_rules_without_an_example_harness_document_no_examples() {
    let audits = SUPPRESSION_AUDITS.iter().map(Declaration::declared);
    let command_rules = COMMAND_RULES.iter().map(|rule| rule.declaration.declared());
    for rule in audits.chain(command_rules) {
        assert!(
            rule.doc.examples.is_empty(),
            "Rule {} has no harness running its examples and must document none",
            rule.name
        );
    }
}

fn rule_sources(relative_dir: &str) -> Vec<(std::path::PathBuf, String)> {
    let rules_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(relative_dir);
    let entries = std::fs::read_dir(rules_dir).expect("rule directory must be readable");
    let mut sources: Vec<_> = entries
        .map(|entry| entry.expect("rule directory entry must be readable").path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "rs"))
        .map(|path| {
            let source = std::fs::read_to_string(&path).expect("rule source must be readable");
            (path, source)
        })
        .collect();
    sources.sort_by(|(left, _), (right, _)| left.cmp(right));
    sources
}

static RULE_SOURCES: std::sync::LazyLock<Vec<(std::path::PathBuf, String)>> =
    std::sync::LazyLock::new(|| rule_sources("src/code_lint/rules"));

fn rule_test_convention_violation(source: &str) -> Option<&'static str> {
    if !source.contains("rule_test!(") {
        return Some("must use crate::test_utils::rule_test!(...) for its test suite");
    }
    if source.contains("mod tests") {
        return Some(
            "must not define a bespoke mod tests block; rule_test!(...) generates it automatically",
        );
    }
    if source.contains("assert_code_rule_snapshot") {
        return Some("must not use assert_code_rule_snapshot; use rule_test!(...) instead");
    }
    let pre_macro = source.split("rule_test!(").next().unwrap_or(source);
    // Multiline raw strings hold doc example snippets, whose `#[test]` is not a bespoke test.
    let mut in_raw_string = false;
    if pre_macro.lines().any(|line| {
        let trimmed = line.trim();
        if in_raw_string {
            in_raw_string = !(trimmed.starts_with("\"}") || trimmed.starts_with("\"#}"));
            return false;
        }
        in_raw_string = trimmed.ends_with("r\"") || trimmed.ends_with("r#\"");
        trimmed == "#[test]" || trimmed == "#[rstest]" || trimmed.starts_with("#[rstest(")
    }) {
        return Some("must not define bespoke #[test] functions outside rule_test!(...)");
    }
    None
}

/// Enforces that all rule test modules use `rule_test!` and never define bespoke tests.
#[test]
fn test_rule_files_use_rule_test() {
    for (path, source) in &*RULE_SOURCES {
        let filename = path
            .file_name()
            .and_then(|name| name.to_str())
            .expect("valid filename");

        if let Some(reason) = rule_test_convention_violation(source) {
            panic!("{filename} {reason}");
        }
    }
}

// The tests below enforce `docs/dev/naming_and_message_style_guide.md` §1, §2 and §3.

/// Every rule the three registries declare.
fn all_declared_rules() -> Vec<DeclaredRule> {
    CODE_RULES
        .iter()
        .map(|rule| rule.declaration())
        .chain(SUPPRESSION_AUDITS.iter().map(Declaration::declared))
        .chain(COMMAND_RULES.iter().map(|rule| rule.declaration.declared()))
        .collect()
}

/// Name openings and ending that state a policy or a threshold instead of the flagged pattern.
const POLARITY_PREFIXES: [&str; 6] = ["no-", "prefer-", "enforce-", "banned-", "max-", "min-"];
const POLARITY_SUFFIX: &str = "-enforced";
const MAX_RULE_NAME_WORDS: usize = 4;

fn states_a_policy(name: &str) -> bool {
    POLARITY_PREFIXES
        .iter()
        .any(|prefix| name.starts_with(prefix))
        || name.ends_with(POLARITY_SUFFIX)
}

#[test]
fn test_rule_names_follow_the_naming_grammar() {
    let mut seen = HashSet::new();
    for rule in all_declared_rules() {
        let name = rule.name.0;
        assert!(is_kebab_case(name), "Rule name `{name}` is not kebab-case");
        assert!(
            !states_a_policy(name),
            "Rule name `{name}` states a policy; name the flagged pattern instead"
        );
        assert!(
            name.split('-').count() <= MAX_RULE_NAME_WORDS,
            "Rule name `{name}` has more than {MAX_RULE_NAME_WORDS} words"
        );
        assert!(seen.insert(name), "Rule name `{name}` is declared twice");
    }
}

/// The rule names a source file declares, in order.
fn declared_rule_names(source: &str) -> Vec<&str> {
    source
        .split("RuleName(\"")
        .skip(1)
        .filter_map(|rest| rest.split('"').next())
        .collect()
}

/// A rule file is the `snake_case` of its rule, exposed as `RULE`; a file holding several rules
/// exposes each as the `SCREAMING_SNAKE_CASE` of its name.
#[test]
fn test_rule_files_and_consts_are_named_after_their_rule() {
    let command_sources = rule_sources("src/command_lint/rules");
    for (path, source) in RULE_SOURCES.iter().chain(&command_sources) {
        let stem = path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .expect("valid file stem");
        let names = declared_rule_names(source);
        assert!(
            names.iter().any(|name| name.replace('-', "_") == stem),
            "{stem}.rs is not the snake_case of a rule it declares: {names:?}"
        );
        for name in &names {
            let expected_const = if names.len() == 1 {
                "RULE".to_string()
            } else {
                name.replace('-', "_").to_uppercase()
            };
            assert!(
                source.contains(&format!("pub const {expected_const}:")),
                "{stem}.rs must expose `{name}` as `pub const {expected_const}`"
            );
        }
    }
}

/// The verbs a suggestion may open with (guide §2.4).
const SUGGESTION_VERBS: [&str; 23] = [
    "Wrap",
    "Replace",
    "Rename",
    "Split",
    "Add",
    "Remove",
    "Move",
    "Pass",
    "Wait",
    "Destructure",
    "Unpack",
    "Insert",
    "Spawn",
    "Create",
    "Load",
    "Narrow",
    "Assert",
    "Inject",
    "Specify",
    "Verify",
    "Extract",
    "Synchronize",
    "Access",
];
/// Verbs that describe the fix; a summary states the fact and a rationale explains the harm.
const FIX_VERBS: [&str; 5] = ["use", "replace", "add", "rename", "remove"];
/// Words that judge instead of describing (guide §2.1).
const JUDGEMENT_WORDS: [&str; 5] = ["banned", "forbidden", "discouraged", "illegal", "must"];
/// Abbreviations spelled out as "such as" or "for example" (guide §2.1).
const ABBREVIATIONS: [&str; 2] = ["e.g.", "i.e."];
/// The placeholders every template draws from (guide §3); a rule may also use the `snake_case`
/// of its own count option keys.
const PLACEHOLDERS: [&str; 16] = [
    "callee",
    "function",
    "class",
    "name",
    "suffix",
    "token",
    "stem",
    "expression",
    "replacement",
    "receiver",
    "positions",
    "construct",
    "duplicates",
    "rule",
    "revision",
    "count",
];

/// The base text and every language override of a template field.
fn texts(field: &LanguageText) -> impl Iterator<Item = &'static str> {
    std::iter::once(field.base).chain(field.overrides.iter().map(|(_, text)| *text))
}

/// `text` without its backtick spans: the prose the style rules apply to.
fn prose(text: &str) -> String {
    text.split('`').step_by(2).collect()
}

/// The lowercase words of `text`.
fn words(text: &str) -> impl Iterator<Item = String> {
    text.split(|character: char| !character.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .map(str::to_lowercase)
}

/// The first word of `text`, skipping any leading punctuation.
fn first_word(text: &str) -> &str {
    text.trim_start_matches(|character: char| !character.is_alphabetic())
        .split(|character: char| !character.is_alphabetic())
        .next()
        .unwrap_or_default()
}

/// True if `prose` quotes with `'` or `"`; a possessive such as `block's` is not a quote.
fn has_quotes(prose: &str) -> bool {
    let mut previous = ' ';
    prose.chars().any(|character| {
        let quoting = character == '"' || (character == '\'' && !previous.is_alphanumeric());
        previous = character;
        quoting
    })
}

/// The `{word}` placeholders of `text`; braces around anything else, such as `{r"..."}`, are
/// literal text.
fn placeholders(text: &str) -> impl Iterator<Item = &str> {
    text.split('{')
        .skip(1)
        .filter_map(|rest| rest.split('}').next())
        .filter(|inner| {
            !inner.is_empty()
                && inner
                    .chars()
                    .all(|character| character.is_ascii_lowercase() || character == '_')
        })
}

/// The `snake_case` form of a rule's count option keys, usable as threshold placeholders.
fn count_placeholders(options: &DeclaredOptions) -> Vec<String> {
    options
        .options
        .iter()
        .filter_map(|option| match option {
            OptionSpec::Count(count) => Some(count.key.replace('-', "_")),
            OptionSpec::List(_) => None,
        })
        .collect()
}

/// Guide §2.1: every field is a sentence in backtick-quoted prose with no judgement word.
#[test]
fn test_template_fields_share_the_common_form() {
    for rule in all_declared_rules() {
        let name = rule.name.0;
        let template = rule.template;
        for field in [template.summary, template.rationale, template.suggestion] {
            for text in texts(&field) {
                let prose = prose(text);
                assert!(
                    text.ends_with('.'),
                    "Rule {name}: `{text}` does not end with a period"
                );
                assert!(
                    ABBREVIATIONS
                        .iter()
                        .all(|abbreviation| !text.contains(abbreviation)),
                    "Rule {name}: `{text}` abbreviates; write \"such as\" or \"for example\""
                );
                assert!(
                    !has_quotes(&prose),
                    "Rule {name}: `{text}` quotes with ' or \"; use backticks"
                );
                assert!(
                    words(&prose).all(|word| !JUDGEMENT_WORDS.contains(&word.as_str())),
                    "Rule {name}: `{text}` contains a judgement word"
                );
            }
        }
    }
}

/// Guide §2.2: a summary is one sentence stating the fact, with no fix verb.
#[test]
fn test_summaries_state_only_the_fact() {
    for rule in all_declared_rules() {
        let name = rule.name.0;
        for text in texts(&rule.template.summary) {
            let prose = prose(text);
            assert!(
                text.starts_with(|character: char| character.is_uppercase() || character == '`'),
                "Rule {name}: summary `{text}` does not start with an uppercase letter or a backtick"
            );
            assert!(
                !prose.contains(". "),
                "Rule {name}: summary `{text}` has more than one sentence"
            );
            assert!(
                words(&prose).all(|word| !FIX_VERBS.contains(&word.as_str())),
                "Rule {name}: summary `{text}` contains a fix verb"
            );
        }
    }
}

/// Guide §2.3: a rationale explains the harm without commanding a fix.
#[test]
fn test_rationales_explain_without_commanding() {
    for rule in all_declared_rules() {
        let name = rule.name.0;
        for text in texts(&rule.template.rationale) {
            assert!(
                !FIX_VERBS.contains(&first_word(text).to_lowercase().as_str()),
                "Rule {name}: rationale `{text}` starts with a fix verb"
            );
            assert!(
                words(&prose(text)).all(|word| word != "should"),
                "Rule {name}: rationale `{text}` is normative"
            );
        }
    }
}

/// Guide §2.4: a suggestion opens with a verb from the shared list.
#[test]
fn test_suggestions_open_with_a_listed_verb() {
    for rule in all_declared_rules() {
        let name = rule.name.0;
        for text in texts(&rule.template.suggestion) {
            assert!(
                SUGGESTION_VERBS.contains(&first_word(text)),
                "Rule {name}: suggestion `{text}` does not start with a verb from guide §2.4"
            );
        }
    }
}

/// Guide §3: placeholders come from the shared vocabulary or the rule's own count keys.
#[test]
fn test_placeholders_use_the_shared_vocabulary() {
    for rule in all_declared_rules() {
        let name = rule.name.0;
        let template = rule.template;
        let allowed = count_placeholders(&rule.options);
        for field in [template.summary, template.rationale, template.suggestion] {
            for placeholder in texts(&field).flat_map(placeholders) {
                assert!(
                    PLACEHOLDERS.contains(&placeholder)
                        || allowed.iter().any(|key| key == placeholder),
                    "Rule {name}: placeholder `{{{placeholder}}}` is not in the guide §3 vocabulary"
                );
            }
        }
    }
}

/// True if `text` is a single sentence closed by a period.
fn is_one_sentence(text: &str) -> bool {
    text.ends_with('.') && !text.contains(". ")
}

/// Guide §2.5: a doc summary is one sentence opening with `Flags` or `Requires`.
#[test]
fn test_doc_summaries_open_with_flags_or_requires() {
    for rule in all_declared_rules() {
        let name = rule.name.0;
        let summary = rule.doc.summary;
        assert!(
            summary.starts_with("Flags ") || summary.starts_with("Requires "),
            "Rule {name}: doc summary `{summary}` does not open with `Flags` or `Requires`"
        );
        assert!(
            is_one_sentence(summary),
            "Rule {name}: doc summary `{summary}` is not one sentence"
        );
    }
}
