//! Rule options: each rule declares its options once, as `const` handles; the config loader
//! validates every `[rules.<name>]` table against that declaration once, at load; and the
//! rule reads resolved values through [`ResolvedOptions`].

use std::collections::{HashMap, HashSet};

use ast_grep_language::SupportLang;
use strum::IntoEnumIterator as _;

use super::{
    EnforcementMode, FilterListDefaults, LanguageDefaults, SUPPORTED_LANGUAGES, closest_match,
    support_lang_name,
};

/// A non-negative integer option, such as the maximum number of assertions in one test.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CountOption {
    /// The TOML key, naming what is counted (`max_assertions`, not `max`).
    pub key: &'static str,
    /// One sentence saying what the value bounds.
    pub doc: &'static str,
    /// The default value, per language.
    pub default: LanguageDefaults<usize>,
}

/// Whether a list option holds the items a rule flags or the items it accepts. The kind
/// fixes the option's three keys, so a rule declares at most one list option.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListKind {
    /// Items the rule flags: `banned`, `extend_banned` and `allowed`.
    Deny,
    /// Items the rule accepts: `allowed`, `extend_allowed` and `banned`.
    Allow,
}

impl ListKind {
    /// The key whose items replace the defaults.
    #[must_use]
    pub const fn replace_key(self) -> &'static str {
        match self {
            Self::Deny => "banned",
            Self::Allow => "allowed",
        }
    }

    /// The key whose items are added to the defaults.
    #[must_use]
    pub const fn extend_key(self) -> &'static str {
        match self {
            Self::Deny => "extend_banned",
            Self::Allow => "extend_allowed",
        }
    }

    /// The key whose items are removed from the defaults.
    #[must_use]
    pub const fn remove_key(self) -> &'static str {
        match self {
            Self::Deny => "allowed",
            Self::Allow => "banned",
        }
    }
}

/// A list-of-strings option, such as the abbreviations a rule bans.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ListOption {
    /// Whether the items are flagged or accepted; fixes the option's keys.
    pub kind: ListKind,
    /// One sentence saying what the items are.
    pub doc: &'static str,
    /// The default items, per language.
    pub default: FilterListDefaults,
}

/// One declared option of a rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OptionSpec {
    /// A count, under one key.
    Count(&'static CountOption),
    /// A list, under the three keys of its kind.
    List(&'static ListOption),
}

impl OptionSpec {
    /// The TOML keys the option accepts.
    #[must_use]
    pub fn keys(self) -> Vec<&'static str> {
        match self {
            Self::Count(option) => vec![option.key],
            Self::List(option) => vec![
                option.kind.replace_key(),
                option.kind.extend_key(),
                option.kind.remove_key(),
            ],
        }
    }
}

/// Everything a rule accepts under `[rules.<name>]` and `[rules.<name>.<language>]`.
/// A key that is not declared here is rejected at load.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RuleOptions {
    /// The default `enforcement_mode`, or `None` if the rule rejects the key because its
    /// runner always reports every finding.
    pub enforcement_mode: Option<LanguageDefaults<EnforcementMode>>,
    /// The rule's own options.
    pub options: &'static [OptionSpec],
}

impl RuleOptions {
    /// A code rule with no options of its own: only `enforcement_mode`, defaulting to `ban`.
    pub const CODE_RULE: Self = Self {
        enforcement_mode: Some(LanguageDefaults::new(EnforcementMode::Ban, &[])),
        options: &[],
    };

    /// A rule that accepts no options, not even `enforcement_mode`.
    pub const NONE: Self = Self {
        enforcement_mode: None,
        options: &[],
    };

    /// Every key accepted in a rule or language table, in declaration order.
    #[must_use]
    pub fn keys(&self) -> Vec<&'static str> {
        self.enforcement_mode
            .map(|_| EnforcementMode::KEY)
            .into_iter()
            .chain(self.options.iter().flat_map(|option| option.keys()))
            .collect()
    }
}

/// The items one table puts in place of, adds to or removes from a list's defaults.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct ListOverride {
    replace: Option<HashSet<String>>,
    extend: HashSet<String>,
    remove: HashSet<String>,
}

/// The validated values of one `[rules.<name>]` table or one of its language tables.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct OptionValues {
    enforcement_mode: Option<EnforcementMode>,
    /// Counts keyed by their declared [`CountOption::key`].
    counts: HashMap<&'static str, usize>,
    list: ListOverride,
}

/// A rule's validated configuration: its `[rules.<name>]` values and its language tables.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RuleOverrides {
    global: OptionValues,
    per_language: Vec<(SupportLang, OptionValues)>,
}

/// What is wrong with a `[rules]` entry.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum OptionProblem {
    /// A key the rule does not declare.
    #[error("unknown key; {}", match (suggestion, expected.as_slice()) {
        (Some(known), _) => format!("did you mean `{known}`?"),
        (None, []) => "this rule takes no options".to_owned(),
        (None, keys) => format!("expected one of `{}`", keys.join("`, `")),
    })]
    UnknownKey {
        /// The closest accepted key, if it looks like a typo or a renamed key.
        suggestion: Option<&'static str>,
        /// Every accepted key.
        expected: Vec<&'static str>,
    },
    /// A value of the wrong type.
    #[error("expected {expected}, found {found}")]
    WrongType {
        /// What the key accepts.
        expected: String,
        /// The value as written, or its type.
        found: String,
    },
    /// A table for a language the rule does not analyze.
    #[error("this rule does not analyze this language; it analyzes {analyzed}")]
    UnsupportedLanguage {
        /// The languages the rule analyzes, comma-separated.
        analyzed: String,
    },
    /// `enforcement_mode` on a rule whose runner always reports every finding.
    #[error("this rule has no enforcement mode; it always reports every finding")]
    EnforcementModeRejected,
}

/// A rejected `[rules]` entry: the key path as written, and what is wrong with it.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("`{key_path}`: {problem}")]
pub struct RuleOptionsError {
    /// The dotted path to the entry, e.g. `rules.max-test-assertions.rust.max_assertions`.
    pub key_path: String,
    /// What is wrong with it.
    pub problem: OptionProblem,
}

impl RuleOverrides {
    /// Validates the `[rules.<rule_name>]` value against the rule's declaration.
    ///
    /// # Errors
    ///
    /// Returns [`RuleOptionsError`] on an undeclared key, a value of the wrong type, a table
    /// for a language the rule does not analyze, or `enforcement_mode` on a rule that
    /// rejects it.
    pub fn parse(
        rule_name: &str,
        declaration: &RuleOptions,
        supported_languages: &[SupportLang],
        value: &toml::Value,
    ) -> Result<Self, RuleOptionsError> {
        let rule_path = format!("rules.{rule_name}");
        let mut overrides = Self::default();
        for (key, value) in expect_table(value, &rule_path)? {
            let key_path = format!("{rule_path}.{key}");
            match language_named(key) {
                Some(language) if supported_languages.contains(&language) => {
                    let mut values = OptionValues::default();
                    for (key, value) in expect_table(value, &key_path)? {
                        values.set(declaration, &[], &key_path, key, value)?;
                    }
                    overrides.per_language.push((language, values));
                }
                Some(_) => {
                    let analyzed: Vec<_> = supported_languages
                        .iter()
                        .map(|&language| support_lang_name(language))
                        .collect();
                    return Err(RuleOptionsError {
                        key_path,
                        problem: OptionProblem::UnsupportedLanguage {
                            analyzed: analyzed.join(", "),
                        },
                    });
                }
                None => {
                    overrides.global.set(
                        declaration,
                        supported_languages,
                        &rule_path,
                        key,
                        value,
                    )?;
                }
            }
        }
        Ok(overrides)
    }

    fn for_language(&self, language: SupportLang) -> Option<&OptionValues> {
        self.per_language
            .iter()
            .find_map(|(candidate, values)| (*candidate == language).then_some(values))
    }
}

impl OptionValues {
    /// Validates the entry `key = value` of the table at `table_path` and stores it.
    /// `languages` are the language tables also accepted in that table.
    fn set(
        &mut self,
        declaration: &RuleOptions,
        languages: &[SupportLang],
        table_path: &str,
        key: &str,
        value: &toml::Value,
    ) -> Result<(), RuleOptionsError> {
        let key_path = format!("{table_path}.{key}");
        let error = |problem| RuleOptionsError {
            key_path: key_path.clone(),
            problem,
        };
        if key == EnforcementMode::KEY {
            if declaration.enforcement_mode.is_none() {
                return Err(error(OptionProblem::EnforcementModeRejected));
            }
            self.enforcement_mode = Some(parse_enforcement_mode(value).map_err(error)?);
            return Ok(());
        }
        for &option in declaration.options {
            match option {
                OptionSpec::Count(count) if key == count.key => {
                    self.counts
                        .insert(count.key, parse_count(value).map_err(error)?);
                    return Ok(());
                }
                OptionSpec::List(list) if key == list.kind.replace_key() => {
                    self.list.replace = Some(parse_items(value).map_err(error)?);
                    return Ok(());
                }
                OptionSpec::List(list) if key == list.kind.extend_key() => {
                    self.list.extend = parse_items(value).map_err(error)?;
                    return Ok(());
                }
                OptionSpec::List(list) if key == list.kind.remove_key() => {
                    self.list.remove = parse_items(value).map_err(error)?;
                    return Ok(());
                }
                OptionSpec::Count(_) | OptionSpec::List(_) => {}
            }
        }
        let mut expected = declaration.keys();
        expected.extend(
            languages
                .iter()
                .map(|&language| support_lang_name(language)),
        );
        Err(error(OptionProblem::UnknownKey {
            suggestion: suggest_key(key, &expected),
            expected,
        }))
    }
}

/// The accepted key closest to `key`: a likely typo, or else a key that contains `key` as a
/// word, which is what a key renamed to an explicit name looks like (`max` →
/// `max_assertions`, `mode` → `enforcement_mode`).
fn suggest_key(key: &str, accepted: &[&'static str]) -> Option<&'static str> {
    closest_match(key, accepted.iter().copied()).or_else(|| {
        accepted
            .iter()
            .copied()
            .find(|candidate| candidate.split('_').any(|word| word == key))
    })
}

fn language_named(key: &str) -> Option<SupportLang> {
    SUPPORTED_LANGUAGES
        .iter()
        .copied()
        .find(|&language| support_lang_name(language) == key)
}

fn expect_table<'value>(
    value: &'value toml::Value,
    key_path: &str,
) -> Result<&'value toml::Table, RuleOptionsError> {
    value.as_table().ok_or_else(|| RuleOptionsError {
        key_path: key_path.to_owned(),
        problem: wrong_type("a table", value),
    })
}

fn wrong_type(expected: &str, value: &toml::Value) -> OptionProblem {
    let found = match value {
        toml::Value::String(text) => format!("\"{text}\""),
        toml::Value::Integer(number) => number.to_string(),
        other => other.type_str().to_owned(),
    };
    OptionProblem::WrongType {
        expected: expected.to_owned(),
        found,
    }
}

fn parse_enforcement_mode(value: &toml::Value) -> Result<EnforcementMode, OptionProblem> {
    EnforcementMode::iter()
        .find(|&mode| value.as_str() == Some(mode.into()))
        .ok_or_else(|| {
            let labels: Vec<&str> = EnforcementMode::iter().map(Into::into).collect();
            wrong_type(&format!("`{}`", labels.join("` or `")), value)
        })
}

fn parse_count(value: &toml::Value) -> Result<usize, OptionProblem> {
    value
        .as_integer()
        .and_then(|number| usize::try_from(number).ok())
        .ok_or_else(|| wrong_type("a non-negative integer", value))
}

fn parse_items(value: &toml::Value) -> Result<HashSet<String>, OptionProblem> {
    value
        .as_array()
        .and_then(|items| {
            items
                .iter()
                .map(|item| item.as_str().map(str::to_owned))
                .collect()
        })
        .ok_or_else(|| wrong_type("an array of strings", value))
}

/// A rule's options resolved for one file's language: what `check_file` reads.
///
/// Each value comes from the language table, else the rule table, else the declared default
/// for the language.
#[derive(Debug, Clone, Copy)]
pub struct ResolvedOptions<'config> {
    language: SupportLang,
    declaration: &'config RuleOptions,
    overrides: Option<&'config RuleOverrides>,
}

impl<'config> ResolvedOptions<'config> {
    /// The options of the rule declaring `declaration`, configured with `overrides`, for a
    /// file in `language`.
    #[must_use]
    pub const fn new(
        language: SupportLang,
        declaration: &'config RuleOptions,
        overrides: Option<&'config RuleOverrides>,
    ) -> Self {
        Self {
            language,
            declaration,
            overrides,
        }
    }

    /// The tables that apply, most specific first: the language table, then the rule table.
    fn layers(&self) -> impl Iterator<Item = &'config OptionValues> {
        let language = self.language;
        self.overrides
            .and_then(move |overrides| overrides.for_language(language))
            .into_iter()
            .chain(self.overrides.map(|overrides| &overrides.global))
    }

    /// The enforcement mode; `ban` for a rule that declares none.
    #[must_use]
    pub fn enforcement_mode(&self) -> EnforcementMode {
        self.layers()
            .find_map(|values| values.enforcement_mode)
            .unwrap_or_else(|| {
                self.declaration
                    .enforcement_mode
                    .map_or(EnforcementMode::Ban, |default| {
                        default.resolve_default_for_lang(self.language)
                    })
            })
    }

    /// The value of `option`, which the rule must declare.
    #[must_use]
    pub fn count(&self, option: &CountOption) -> usize {
        debug_assert!(
            self.declaration
                .options
                .iter()
                .any(|declared| matches!(declared, OptionSpec::Count(count) if *count == option)),
            "`{}` is read but not declared in the rule's options",
            option.key
        );
        self.layers()
            .find_map(|values| values.counts.get(option.key).copied())
            .unwrap_or_else(|| option.default.resolve_default_for_lang(self.language))
    }

    /// The items of `option`, which the rule must declare: the replacing items (language
    /// table, else rule table, else defaults), plus every added item, minus every removed one.
    #[must_use]
    pub fn list(&self, option: &ListOption) -> HashSet<String> {
        debug_assert!(
            self.declaration
                .options
                .iter()
                .any(|declared| matches!(declared, OptionSpec::List(list) if *list == option)),
            "a {:?} list is read but not declared in the rule's options",
            option.kind
        );
        let mut items = self
            .layers()
            .find_map(|values| values.list.replace.clone())
            .unwrap_or_else(|| option.default.resolve_default_for_lang(self.language));
        for values in self.layers() {
            items.extend(values.list.extend.iter().cloned());
        }
        for values in self.layers() {
            for item in &values.list.remove {
                items.remove(item);
            }
        }
        items
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    const LIMIT: CountOption = CountOption {
        key: "max_items",
        doc: "Maximum items.",
        default: LanguageDefaults::new(4, &[(SupportLang::Rust, 6)]),
    };

    const DENY: ListOption = ListOption {
        kind: ListKind::Deny,
        doc: "Banned words.",
        default: FilterListDefaults {
            base: &["default_one", "common_ok"],
            extend: &[],
            exempt: &[],
        },
    };

    const ALLOW: ListOption = ListOption {
        kind: ListKind::Allow,
        doc: "Allowed words.",
        default: FilterListDefaults {
            base: &["default_base", "revoked", "rust_revoked"],
            extend: &[(SupportLang::Rust, &["rust_extra"])],
            exempt: &[],
        },
    };

    const WITH_COUNT: RuleOptions = RuleOptions {
        enforcement_mode: Some(LanguageDefaults::new(
            EnforcementMode::Ban,
            &[(SupportLang::Rust, EnforcementMode::RequireExplanation)],
        )),
        options: &[OptionSpec::Count(&LIMIT)],
    };
    const DENYING_RULE: RuleOptions = RuleOptions {
        options: &[OptionSpec::List(&DENY)],
        ..RuleOptions::CODE_RULE
    };
    const ALLOWING_RULE: RuleOptions = RuleOptions {
        options: &[OptionSpec::List(&ALLOW)],
        ..RuleOptions::CODE_RULE
    };

    const BOTH_LANGUAGES: &[SupportLang] = &[SupportLang::Python, SupportLang::Rust];

    fn parse(
        declaration: &RuleOptions,
        languages: &[SupportLang],
        toml_content: &str,
    ) -> Result<RuleOverrides, RuleOptionsError> {
        let value = toml::Value::Table(toml::from_str(toml_content).unwrap());
        RuleOverrides::parse("some-rule", declaration, languages, &value)
    }

    fn resolve<'config>(
        declaration: &'config RuleOptions,
        overrides: &'config RuleOverrides,
        language: SupportLang,
    ) -> ResolvedOptions<'config> {
        ResolvedOptions::new(language, declaration, Some(overrides))
    }

    fn words(items: &[&str]) -> HashSet<String> {
        items.iter().map(|&item| item.to_owned()).collect()
    }

    #[rstest]
    #[case::python_default("", SupportLang::Python, 4)]
    #[case::rust_default("", SupportLang::Rust, 6)]
    #[case::rule_table("max_items = 5", SupportLang::Rust, 5)]
    #[case::other_language_table("max_items = 5\n[rust]\nmax_items = 10", SupportLang::Python, 5)]
    #[case::language_table("max_items = 5\n[rust]\nmax_items = 10", SupportLang::Rust, 10)]
    fn count_prefers_language_then_rule_table_then_default(
        #[case] toml_content: &str,
        #[case] language: SupportLang,
        #[case] expected: usize,
    ) {
        let overrides = parse(&WITH_COUNT, BOTH_LANGUAGES, toml_content).unwrap();
        assert_eq!(
            resolve(&WITH_COUNT, &overrides, language).count(&LIMIT),
            expected
        );
    }

    #[rstest]
    #[case::python_default("", SupportLang::Python, EnforcementMode::Ban)]
    #[case::rust_default("", SupportLang::Rust, EnforcementMode::RequireExplanation)]
    #[case::rule_table("enforcement_mode = \"ban\"", SupportLang::Rust, EnforcementMode::Ban)]
    #[case::language_table(
        "enforcement_mode = \"ban\"\n[python]\nenforcement_mode = \"require-explanation\"",
        SupportLang::Python,
        EnforcementMode::RequireExplanation
    )]
    fn enforcement_mode_prefers_language_then_rule_table_then_default(
        #[case] toml_content: &str,
        #[case] language: SupportLang,
        #[case] expected: EnforcementMode,
    ) {
        let overrides = parse(&WITH_COUNT, BOTH_LANGUAGES, toml_content).unwrap();
        assert_eq!(
            resolve(&WITH_COUNT, &overrides, language).enforcement_mode(),
            expected
        );
    }

    #[test]
    fn deny_list_replaces_then_extends_then_removes() {
        let overrides = parse(
            &DENYING_RULE,
            BOTH_LANGUAGES,
            indoc::indoc! {r#"
                allowed = ["common_ok"]
                extend_banned = ["global_bad"]
                enforcement_mode = "require-explanation"

                [rust]
                allowed = ["rust_ok"]
                extend_banned = ["rust_bad"]

                [python]
                banned = ["py_only_bad"]
            "#},
        )
        .unwrap();

        let rust = resolve(&DENYING_RULE, &overrides, SupportLang::Rust).list(&DENY);
        let python = resolve(&DENYING_RULE, &overrides, SupportLang::Python).list(&DENY);
        assert_eq!(
            (rust, python),
            (
                words(&["default_one", "global_bad", "rust_bad"]),
                words(&["py_only_bad", "global_bad"])
            )
        );
    }

    #[test]
    fn allow_list_replaces_then_extends_then_removes() {
        let overrides = parse(
            &ALLOWING_RULE,
            BOTH_LANGUAGES,
            indoc::indoc! {r#"
                banned = ["revoked"]
                extend_allowed = ["global_allowed"]

                [rust]
                banned = ["rust_revoked"]
                extend_allowed = ["rust_allowed"]

                [python]
                allowed = ["py_only_allowed"]
            "#},
        )
        .unwrap();

        let rust = resolve(&ALLOWING_RULE, &overrides, SupportLang::Rust).list(&ALLOW);
        let python = resolve(&ALLOWING_RULE, &overrides, SupportLang::Python).list(&ALLOW);
        assert_eq!(
            (rust, python),
            (
                words(&[
                    "default_base",
                    "rust_extra",
                    "global_allowed",
                    "rust_allowed"
                ]),
                words(&["py_only_allowed", "global_allowed"])
            )
        );
    }

    #[test]
    fn missing_overrides_resolve_to_defaults() {
        let options = ResolvedOptions::new(SupportLang::Rust, &WITH_COUNT, None);
        assert_eq!(
            (options.count(&LIMIT), options.enforcement_mode()),
            (6, EnforcementMode::RequireExplanation)
        );
    }

    #[rstest]
    #[case::typo(
        &WITH_COUNT,
        BOTH_LANGUAGES,
        "max_itemz = 5",
        "`rules.some-rule.max_itemz`: unknown key; did you mean `max_items`?"
    )]
    #[case::renamed_key(
        &WITH_COUNT,
        BOTH_LANGUAGES,
        "[rust]\nmode = \"ban\"",
        "`rules.some-rule.rust.mode`: unknown key; did you mean `enforcement_mode`?"
    )]
    #[case::unrelated_key(
        &DENYING_RULE,
        &[SupportLang::Python],
        "colour = 1",
        "`rules.some-rule.colour`: unknown key; expected one of `enforcement_mode`, `banned`, \
         `extend_banned`, `allowed`, `python`"
    )]
    #[case::no_options(
        &RuleOptions::NONE,
        &[],
        "max = 1",
        "`rules.some-rule.max`: unknown key; this rule takes no options"
    )]
    #[case::count_wrong_type(
        &WITH_COUNT,
        BOTH_LANGUAGES,
        "max_items = \"five\"",
        "`rules.some-rule.max_items`: expected a non-negative integer, found \"five\""
    )]
    #[case::negative_count(
        &WITH_COUNT,
        BOTH_LANGUAGES,
        "max_items = -1",
        "`rules.some-rule.max_items`: expected a non-negative integer, found -1"
    )]
    #[case::list_wrong_type(
        &DENYING_RULE,
        BOTH_LANGUAGES,
        "extend_banned = [\"ok\", 3]",
        "`rules.some-rule.extend_banned`: expected an array of strings, found array"
    )]
    #[case::enforcement_mode_wrong_value(
        &WITH_COUNT,
        BOTH_LANGUAGES,
        "enforcement_mode = \"bann\"",
        "`rules.some-rule.enforcement_mode`: expected `ban` or `require-explanation`, \
         found \"bann\""
    )]
    #[case::language_not_a_table(
        &WITH_COUNT,
        BOTH_LANGUAGES,
        "rust = 1",
        "`rules.some-rule.rust`: expected a table, found 1"
    )]
    #[case::unsupported_language(
        &WITH_COUNT,
        &[SupportLang::Python],
        "[rust]\nmax_items = 1",
        "`rules.some-rule.rust`: this rule does not analyze this language; it analyzes python"
    )]
    #[case::enforcement_mode_rejected(
        &RuleOptions::NONE,
        BOTH_LANGUAGES,
        "enforcement_mode = \"ban\"",
        "`rules.some-rule.enforcement_mode`: this rule has no enforcement mode; it always \
         reports every finding"
    )]
    fn invalid_options_are_rejected_with_their_key_path(
        #[case] declaration: &RuleOptions,
        #[case] languages: &[SupportLang],
        #[case] toml_content: &str,
        #[case] message: &str,
    ) {
        let error = parse(declaration, languages, toml_content).unwrap_err();
        assert_eq!(error.to_string(), message);
    }

    #[test]
    fn rule_value_must_be_a_table() {
        let error = RuleOverrides::parse(
            "some-rule",
            &WITH_COUNT,
            BOTH_LANGUAGES,
            &toml::Value::Boolean(true),
        )
        .unwrap_err();
        assert_eq!(
            error.to_string(),
            "`rules.some-rule`: expected a table, found boolean"
        );
    }
}
