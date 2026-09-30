//! Centralized registry integrity and uniqueness validation tests.

// Workaround for rust-lang/rust-clippy#13981 so clippy.toml `allow-*-in-tests` applies to the whole file.
#![cfg(test)]

use omni::code_lint::rules::CODE_RULES;
use omni::code_lint::suppression::SUPPRESSION_AUDITS;
use omni::command_lint::rules::COMMAND_RULES;
use omni::core::{Tag, is_kebab_case};
use omni::rule_taxonomy::ClassifiedRule;
use rstest::rstest;
use std::collections::HashSet;
use strum::IntoEnumIterator;

fn validate_rule(rule: &(impl omni::core::Rule + ?Sized), names: &mut HashSet<&'static str>) {
    let name = rule.name().0;

    assert!(
        names.insert(name),
        "Duplicate rule name found in registry: {name}"
    );
    assert!(
        is_kebab_case(name),
        "Rule name '{name}' does not match standard kebab-case pattern"
    );
    assert!(
        !rule.tags().is_empty(),
        "Rule {name} must declare at least one domain tag"
    );

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

#[rstest]
#[case::code_rules(CODE_RULES)]
#[case::suppression_audits(SUPPRESSION_AUDITS)]
#[case::command_rules(COMMAND_RULES)]
fn test_registry_integrity<R>(#[case] rules: &[ClassifiedRule<R>])
where
    R: omni::core::Rule + ?Sized,
{
    let mut names = HashSet::new();
    for registered in rules {
        validate_rule(registered.rule, &mut names);
    }
}

#[test]
fn test_global_registry_uniqueness() {
    let mut names = HashSet::new();

    let code_names = CODE_RULES.iter().map(|registered| registered.rule.name().0);
    let audit_names = SUPPRESSION_AUDITS
        .iter()
        .map(|registered| registered.rule.name().0);
    let command_names = COMMAND_RULES
        .iter()
        .map(|registered| registered.rule.name().0);
    for name in code_names.chain(audit_names).chain(command_names) {
        assert!(
            names.insert(name),
            "Global rule name collision across registries: {name}"
        );
    }
}

#[test]
fn test_code_rules_declare_supported_languages() {
    let code_languages = CODE_RULES.iter().map(|registered| {
        (
            registered.rule.name(),
            registered.rule.supported_languages(),
        )
    });
    let audit_languages = SUPPRESSION_AUDITS.iter().map(|registered| {
        (
            registered.rule.name(),
            registered.rule.supported_languages(),
        )
    });
    for (name, languages) in code_languages.chain(audit_languages) {
        assert!(
            !languages.is_empty(),
            "Code rule {name} must declare at least one supported language"
        );
    }
}

#[rstest]
#[case::code_rules(CODE_RULES)]
#[case::suppression_audits(SUPPRESSION_AUDITS)]
#[case::command_rules(COMMAND_RULES)]
fn test_language_tags_are_derived_not_declared<R>(#[case] rules: &[ClassifiedRule<R>])
where
    R: omni::core::Rule + ?Sized,
{
    for rule in rules.iter().map(|registered| registered.rule) {
        for tag in rule.tags() {
            assert!(
                tag.to_support_lang().is_none(),
                "Rule {} declares language tag {tag:?}; language tags are derived from supported_languages()",
                rule.name().0
            );
        }
        for lang in rule.supported_languages() {
            let lang_tag = Tag::iter()
                .find(|tag| tag.to_support_lang() == Some(*lang))
                .expect("supported language must have a matching Tag variant");
            assert!(
                rule.has_tag(lang_tag),
                "Rule {} does not resolve derived language tag {lang_tag:?}",
                rule.name().0
            );
        }
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

/// Diagnostic ordering is owned by the reporting layer, so a rule sorting its own output is dead work.
#[test]
fn test_rule_sources_do_not_sort_diagnostics() {
    let suppression_path = std::path::PathBuf::from(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/src/code_lint/suppression.rs"
    ));
    let suppression_source =
        std::fs::read_to_string(&suppression_path).expect("suppression source must be readable");
    let suppression_entry = (suppression_path, suppression_source);

    for (path, source) in RULE_SOURCES.iter().chain([&suppression_entry]) {
        assert!(
            !source.contains(".sort"),
            "{} sorts its output; diagnostic ordering belongs to the reporting layer",
            path.display()
        );
    }
}

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
