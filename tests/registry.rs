//! Centralized registry integrity and uniqueness validation tests.

// Workaround for rust-lang/rust-clippy#13981 so clippy.toml `allow-*-in-tests` applies to the whole file.
#![cfg(test)]

use omni::code_lint::rules::CODE_RULES;
use omni::code_lint::suppression::SUPPRESSION_AUDITS;
use omni::command_lint::rules::COMMAND_RULES;
use omni::core::{
    EnforcementMode, OptionSpec, RuleOptions, SUPPORTED_LANGUAGES, support_lang_name,
};
use omni::rule_declaration::Rule;
use rstest::rstest;
use std::collections::HashSet;

fn validate_rule(rule: &(impl omni::core::Detector + ?Sized)) {
    let name = rule.name().0;

    let template = rule.violation_template();
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
                rule.supported_languages().contains(lang),
                "Rule {name} declares template override for {lang:?}, which is not in supported_languages()"
            );
        }
    }
}

fn validate_options(name: &str, options: &RuleOptions) {
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
}

#[rstest]
#[case::code_rules(CODE_RULES)]
#[case::suppression_audits(SUPPRESSION_AUDITS)]
#[case::command_rules(COMMAND_RULES)]
fn test_registry_integrity<R>(#[case] rules: &[Rule<R>])
where
    R: omni::core::Detector + ?Sized,
{
    for registered in rules {
        validate_rule(registered.detector);
        validate_options(registered.detector.name().0, &registered.options);
    }
}

#[test]
fn test_code_rules_declare_supported_languages() {
    let code_languages = CODE_RULES.iter().map(|registered| {
        (
            registered.detector.name(),
            registered.detector.supported_languages(),
        )
    });
    let audit_languages = SUPPRESSION_AUDITS.iter().map(|registered| {
        (
            registered.detector.name(),
            registered.detector.supported_languages(),
        )
    });
    for (name, languages) in code_languages.chain(audit_languages) {
        assert!(
            !languages.is_empty(),
            "Code rule {name} must declare at least one supported language"
        );
    }
}

static RULE_SOURCES: std::sync::LazyLock<Vec<(std::path::PathBuf, String)>> =
    std::sync::LazyLock::new(|| {
        let rules_dir = concat!(env!("CARGO_MANIFEST_DIR"), "/src/code_lint/rules");
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
    });

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
    if pre_macro.lines().any(|line| {
        let trimmed = line.trim();
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
