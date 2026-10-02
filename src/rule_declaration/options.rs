//! Rule options: each rule declares its options once, as a typed `const` value; the config
//! loader validates every `[rules.<name>]` table against that declaration once, at load; and
//! the rule receives the values resolved for each file, typed by the declaration.

use std::collections::{HashMap, HashSet};

use ast_grep_language::SupportLang;
use strum::{EnumIter, IntoEnumIterator as _, IntoStaticStr};

use super::closest_match;

/// Compile-time static descriptor for default filter lists (both allowlists and denylists).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FilterListDefaults {
    /// Base items active across all supported languages.
    pub base: &'static [&'static str],
    /// Language-specific items added to the base defaults.
    pub extend: &'static [(SupportLang, &'static [&'static str])],
    /// Language-specific items removed from the base defaults.
    pub remove: &'static [(SupportLang, &'static [&'static str])],
}

impl FilterListDefaults {
    /// Resolves the default set of strings for a specific language.
    #[must_use]
    pub fn resolve_default_for_lang(&self, lang: SupportLang) -> HashSet<String> {
        let mut set: HashSet<String> = self.base.iter().map(|&item| item.to_string()).collect();
        for &(target_lang, items) in self.extend {
            if target_lang == lang {
                set.extend(items.iter().map(|&item| item.to_string()));
            }
        }
        for &(target_lang, items) in self.remove {
            if target_lang == lang {
                for &item in items {
                    set.remove(item);
                }
            }
        }
        set
    }
}

/// Compile-time static descriptor for scalar or structured default configuration values
/// with optional per-language overrides.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LanguageDefaults<T: Copy + 'static> {
    /// Default value active across all supported languages unless overridden.
    pub base: T,
    /// Language-specific default values that override `base`.
    pub overrides: &'static [(SupportLang, T)],
}

impl<T: Copy + 'static> LanguageDefaults<T> {
    /// Creates a new static language-aware default descriptor.
    #[must_use]
    pub const fn new(base: T, overrides: &'static [(SupportLang, T)]) -> Self {
        Self { base, overrides }
    }

    /// Resolves the compile-time default value for a specific language.
    #[must_use]
    pub fn resolve_default_for_lang(&self, lang: SupportLang) -> T {
        for &(target_lang, value) in self.overrides {
            if target_lang == lang {
                return value;
            }
        }
        self.base
    }
}

/// Enforcement mode for rules targeting sensitive language constructs
/// (e.g. `cast`, `suppress`, `getattr`, `except Exception`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, EnumIter, IntoStaticStr)]
#[strum(serialize_all = "kebab-case")]
pub enum EnforcementMode {
    /// Completely bans the construct from targeted files (only suppressible via `# omni:ignore`).
    Ban,
    /// Permits the construct only if accompanied by an explanatory comment.
    RequireExplanation,
}

impl EnforcementMode {
    /// The key setting a code rule's enforcement mode under `[rules.<name>]`.
    pub const KEY: &'static str = "enforcement-mode";

    /// What each mode does, for `--explain`.
    pub const DOC: &'static str = "`ban` flags every occurrence; `require-explanation` accepts \
                                   an occurrence explained by an adjacent comment.";
}

/// Every language the linter analyzes.
pub const SUPPORTED_LANGUAGES: &[SupportLang] = &[SupportLang::Python, SupportLang::Rust];

/// Returns the lowercase canonical configuration key for a supported language.
#[must_use]
pub const fn support_lang_name(lang: SupportLang) -> &'static str {
    match lang {
        SupportLang::Python => "python",
        SupportLang::Rust => "rust",
        _ => "",
    }
}

/// The options a rule declares, as a typed value: nothing (`()`), one [`CountOption`], one
/// [`ListOption`], or a pair of declarations.
///
/// The declaration fixes the type of the values the rule receives, so a rule can only read
/// the options it declares.
pub trait OptionsDeclaration: Copy + 'static {
    /// The owned values resolved for one file.
    type Resolved;

    /// What a check function receives: by value for `()` and `usize`, by reference for a
    /// `HashSet<String>`, or a pair of those.
    type Param<'a>;

    /// Borrows or copies `resolved` into the check function's parameter.
    fn as_param(resolved: &Self::Resolved) -> Self::Param<'_>;

    /// Every declared option, in declaration order, for validation and `--explain`.
    fn specs(self) -> Vec<OptionSpec>;

    /// The values for a file in `language`: each comes from the language table, else the
    /// rule table, else the declared default for the language.
    fn resolve(self, language: SupportLang, overrides: Option<&RuleOverrides>) -> Self::Resolved;
}

impl OptionsDeclaration for () {
    type Resolved = ();
    type Param<'a> = ();

    fn as_param(&(): &()) {}

    fn specs(self) -> Vec<OptionSpec> {
        Vec::new()
    }

    fn resolve(self, _language: SupportLang, _overrides: Option<&RuleOverrides>) {}
}

/// A non-negative integer option, such as the maximum number of assertions in one test.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CountOption {
    /// The TOML key, naming what is counted (`max-assertions`, not `max`).
    pub key: &'static str,
    /// One sentence saying what the value bounds.
    pub doc: &'static str,
    /// The default value, per language.
    pub default: LanguageDefaults<usize>,
}

impl OptionsDeclaration for CountOption {
    type Resolved = usize;
    type Param<'a> = usize;

    fn as_param(&count: &usize) -> usize {
        count
    }

    fn specs(self) -> Vec<OptionSpec> {
        vec![OptionSpec::Count(self)]
    }

    fn resolve(self, language: SupportLang, overrides: Option<&RuleOverrides>) -> usize {
        layers(overrides, language)
            .find_map(|values| values.counts.get(self.key).copied())
            .unwrap_or_else(|| self.default.resolve_default_for_lang(language))
    }
}

/// Whether a list option holds the items a rule flags or the items it accepts. The kind
/// fixes the option's two keys, so a rule declares at most one list option.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListKind {
    /// Items the rule flags: `banned` and `extend-banned`.
    Deny,
    /// Items the rule accepts: `allowed` and `extend-allowed`.
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
            Self::Deny => "extend-banned",
            Self::Allow => "extend-allowed",
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

impl OptionsDeclaration for ListOption {
    type Resolved = HashSet<String>;
    type Param<'a> = &'a HashSet<String>;

    fn as_param(items: &HashSet<String>) -> &HashSet<String> {
        items
    }

    fn specs(self) -> Vec<OptionSpec> {
        vec![OptionSpec::List(self)]
    }

    /// The replacing items (language table, else rule table, else defaults), plus every
    /// added item.
    fn resolve(self, language: SupportLang, overrides: Option<&RuleOverrides>) -> HashSet<String> {
        let mut items = layers(overrides, language)
            .find_map(|values| values.list.replace.clone())
            .unwrap_or_else(|| self.default.resolve_default_for_lang(language));
        for values in layers(overrides, language) {
            items.extend(values.list.extend.iter().cloned());
        }
        items
    }
}

impl<First: OptionsDeclaration, Second: OptionsDeclaration> OptionsDeclaration for (First, Second) {
    type Resolved = (First::Resolved, Second::Resolved);
    type Param<'a> = (First::Param<'a>, Second::Param<'a>);

    fn as_param((first, second): &Self::Resolved) -> Self::Param<'_> {
        (First::as_param(first), Second::as_param(second))
    }

    fn specs(self) -> Vec<OptionSpec> {
        let (first, second) = self;
        let mut specs = first.specs();
        specs.extend(second.specs());
        specs
    }

    fn resolve(self, language: SupportLang, overrides: Option<&RuleOverrides>) -> Self::Resolved {
        let (first, second) = self;
        (
            first.resolve(language, overrides),
            second.resolve(language, overrides),
        )
    }
}

/// One declared option of a rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OptionSpec {
    /// A count, under one key.
    Count(CountOption),
    /// A list, under the two keys of its kind.
    List(ListOption),
}

impl OptionSpec {
    /// The TOML keys the option accepts.
    #[must_use]
    pub fn keys(self) -> Vec<&'static str> {
        match self {
            Self::Count(option) => vec![option.key],
            Self::List(option) => vec![option.kind.replace_key(), option.kind.extend_key()],
        }
    }
}

/// Everything a rule accepts under `[rules.<name>]` and `[rules.<name>.<language>]`.
/// A key that is not declared here is rejected at load.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RuleOptions<Options: OptionsDeclaration> {
    /// The default `enforcement-mode`, or `None` if the rule rejects the key because its
    /// runner always reports every finding.
    pub enforcement_mode: Option<LanguageDefaults<EnforcementMode>>,
    /// The rule's own options.
    pub options: Options,
}

impl<Options: OptionsDeclaration> RuleOptions<Options> {
    /// A code rule with `options` and an `enforcement-mode` defaulting to `ban`.
    #[must_use]
    pub const fn code_rule(options: Options) -> Self {
        Self {
            enforcement_mode: Some(LanguageDefaults::new(EnforcementMode::Ban, &[])),
            options,
        }
    }

    /// The declaration with its options listed rather than typed, for validation and
    /// `--explain`.
    #[must_use]
    pub fn declared(&self) -> DeclaredOptions {
        DeclaredOptions {
            enforcement_mode: self.enforcement_mode,
            options: self.options.specs(),
        }
    }

    /// The enforcement mode for a file in `language`; `ban` for a rule that declares none.
    #[must_use]
    pub fn enforcement_mode(
        &self,
        language: SupportLang,
        overrides: Option<&RuleOverrides>,
    ) -> EnforcementMode {
        layers(overrides, language)
            .find_map(|values| values.enforcement_mode)
            .unwrap_or_else(|| {
                self.enforcement_mode
                    .map_or(EnforcementMode::Ban, |default| {
                        default.resolve_default_for_lang(language)
                    })
            })
    }
}

impl RuleOptions<()> {
    /// A rule that accepts no options, not even `enforcement-mode`.
    #[must_use]
    pub const fn none() -> Self {
        Self {
            enforcement_mode: None,
            options: (),
        }
    }
}

/// A rule's options listed rather than typed: what the config loader validates against and
/// what `--explain` documents.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeclaredOptions {
    /// The default `enforcement-mode`, or `None` if the rule rejects the key.
    pub enforcement_mode: Option<LanguageDefaults<EnforcementMode>>,
    /// The rule's own options, in declaration order.
    pub options: Vec<OptionSpec>,
}

impl DeclaredOptions {
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

/// The items one table puts in place of, or adds to, a list's defaults.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct ListOverride {
    replace: Option<HashSet<String>>,
    extend: HashSet<String>,
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
        /// The closest accepted key, if it looks like a typo.
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
    /// `enforcement-mode` on a rule whose runner always reports every finding.
    #[error("this rule has no enforcement mode; it always reports every finding")]
    EnforcementModeRejected,
}

/// A rejected `[rules]` entry: the key path as written, and what is wrong with it.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("`{key_path}`: {problem}")]
pub struct RuleOptionsError {
    /// The dotted path to the entry, e.g. `rules.too-many-assertions.rust.max-assertions`.
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
    /// for a language the rule does not analyze, or `enforcement-mode` on a rule that
    /// rejects it.
    pub fn parse(
        rule_name: &str,
        declaration: &DeclaredOptions,
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
        declaration: &DeclaredOptions,
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
        for &option in &declaration.options {
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
            suggestion: closest_match(key, expected.iter().copied()),
            expected,
        }))
    }
}

/// The tables that apply to a file in `language`, most specific first: the language table,
/// then the rule table.
fn layers(
    overrides: Option<&RuleOverrides>,
    language: SupportLang,
) -> impl Iterator<Item = &OptionValues> {
    overrides
        .and_then(move |overrides| overrides.for_language(language))
        .into_iter()
        .chain(overrides.map(|overrides| &overrides.global))
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

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    const LIMIT: CountOption = CountOption {
        key: "max-items",
        doc: "Maximum items.",
        default: LanguageDefaults::new(4, &[(SupportLang::Rust, 6)]),
    };

    const DENY: ListOption = ListOption {
        kind: ListKind::Deny,
        doc: "Banned words.",
        default: FilterListDefaults {
            base: &["default_one", "common_ok"],
            extend: &[],
            remove: &[],
        },
    };

    const ALLOW: ListOption = ListOption {
        kind: ListKind::Allow,
        doc: "Allowed words.",
        default: FilterListDefaults {
            base: &["default_base"],
            extend: &[(SupportLang::Rust, &["rust_extra"])],
            remove: &[],
        },
    };

    const WITH_COUNT: RuleOptions<CountOption> = RuleOptions {
        enforcement_mode: Some(LanguageDefaults::new(
            EnforcementMode::Ban,
            &[(SupportLang::Rust, EnforcementMode::RequireExplanation)],
        )),
        options: LIMIT,
    };
    const DENYING_RULE: RuleOptions<ListOption> = RuleOptions::code_rule(DENY);

    const BOTH_LANGUAGES: &[SupportLang] = &[SupportLang::Python, SupportLang::Rust];

    fn parse(
        declaration: &DeclaredOptions,
        languages: &[SupportLang],
        toml_content: &str,
    ) -> Result<RuleOverrides, RuleOptionsError> {
        let value = toml::Value::Table(toml::from_str(toml_content).unwrap());
        RuleOverrides::parse("some-rule", declaration, languages, &value)
    }

    fn words(items: &[&str]) -> HashSet<String> {
        items.iter().map(|&item| item.to_owned()).collect()
    }

    #[test]
    fn test_filter_list_defaults_resolve() {
        const DEFAULTS: FilterListDefaults = FilterListDefaults {
            base: &["common", "shared", "temp"],
            extend: &[(SupportLang::Rust, &["rust_only"])],
            remove: &[(SupportLang::Rust, &["temp"])],
        };

        let python_defaults = DEFAULTS.resolve_default_for_lang(SupportLang::Python);
        assert_eq!(
            python_defaults,
            HashSet::from(["common", "shared", "temp"].map(String::from))
        );

        let rust_defaults = DEFAULTS.resolve_default_for_lang(SupportLang::Rust);
        assert_eq!(
            rust_defaults,
            HashSet::from(["common", "shared", "rust_only"].map(String::from))
        );
    }

    #[rstest]
    #[case::python_default("", SupportLang::Python, 4)]
    #[case::rust_default("", SupportLang::Rust, 6)]
    #[case::rule_table("max-items = 5", SupportLang::Rust, 5)]
    #[case::other_language_table("max-items = 5\n[rust]\nmax-items = 10", SupportLang::Python, 5)]
    #[case::language_table("max-items = 5\n[rust]\nmax-items = 10", SupportLang::Rust, 10)]
    fn count_prefers_language_then_rule_table_then_default(
        #[case] toml_content: &str,
        #[case] language: SupportLang,
        #[case] expected: usize,
    ) {
        let overrides = parse(&WITH_COUNT.declared(), BOTH_LANGUAGES, toml_content).unwrap();
        assert_eq!(LIMIT.resolve(language, Some(&overrides)), expected);
    }

    #[rstest]
    #[case::python_default("", SupportLang::Python, EnforcementMode::Ban)]
    #[case::rust_default("", SupportLang::Rust, EnforcementMode::RequireExplanation)]
    #[case::rule_table("enforcement-mode = \"ban\"", SupportLang::Rust, EnforcementMode::Ban)]
    #[case::language_table(
        "enforcement-mode = \"ban\"\n[python]\nenforcement-mode = \"require-explanation\"",
        SupportLang::Python,
        EnforcementMode::RequireExplanation
    )]
    fn enforcement_mode_prefers_language_then_rule_table_then_default(
        #[case] toml_content: &str,
        #[case] language: SupportLang,
        #[case] expected: EnforcementMode,
    ) {
        let overrides = parse(&WITH_COUNT.declared(), BOTH_LANGUAGES, toml_content).unwrap();
        assert_eq!(
            WITH_COUNT.enforcement_mode(language, Some(&overrides)),
            expected
        );
    }

    #[rstest]
    #[case::deny(
        DENY,
        indoc::indoc! {r#"
            extend-banned = ["global_bad"]

            [rust]
            extend-banned = ["rust_bad"]

            [python]
            banned = ["py_only_bad"]
        "#},
        &["default_one", "common_ok", "global_bad", "rust_bad"],
        &["py_only_bad", "global_bad"]
    )]
    #[case::allow(
        ALLOW,
        indoc::indoc! {r#"
            extend-allowed = ["global_allowed"]

            [rust]
            extend-allowed = ["rust_allowed"]

            [python]
            allowed = ["py_only_allowed"]
        "#},
        &["default_base", "rust_extra", "global_allowed", "rust_allowed"],
        &["py_only_allowed", "global_allowed"]
    )]
    fn list_replaces_from_the_nearest_table_then_extends_from_every_table(
        #[case] option: ListOption,
        #[case] toml_content: &str,
        #[case] rust_expected: &[&str],
        #[case] python_expected: &[&str],
    ) {
        let overrides = parse(
            &RuleOptions::code_rule(option).declared(),
            BOTH_LANGUAGES,
            toml_content,
        )
        .unwrap();

        let rust = option.resolve(SupportLang::Rust, Some(&overrides));
        let python = option.resolve(SupportLang::Python, Some(&overrides));
        assert_eq!(
            (rust, python),
            (words(rust_expected), words(python_expected))
        );
    }

    #[test]
    fn a_pair_resolves_each_member_and_lists_both_specs() {
        let pair = (LIMIT, DENY);
        let overrides = parse(
            &RuleOptions::code_rule(pair).declared(),
            BOTH_LANGUAGES,
            "max-items = 9\nbanned = [\"only\"]",
        )
        .unwrap();
        assert_eq!(
            (
                pair.specs(),
                pair.resolve(SupportLang::Python, Some(&overrides))
            ),
            (
                vec![OptionSpec::Count(LIMIT), OptionSpec::List(DENY)],
                (9, words(&["only"]))
            )
        );
    }

    #[test]
    fn missing_overrides_resolve_to_defaults() {
        assert_eq!(
            (
                LIMIT.resolve(SupportLang::Rust, None),
                WITH_COUNT.enforcement_mode(SupportLang::Rust, None)
            ),
            (6, EnforcementMode::RequireExplanation)
        );
    }

    #[rstest]
    #[case::typo(
        WITH_COUNT.declared(),
        BOTH_LANGUAGES,
        "max-itemz = 5",
        "`rules.some-rule.max-itemz`: unknown key; did you mean `max-items`?"
    )]
    #[case::unrelated_key(
        DENYING_RULE.declared(),
        &[SupportLang::Python],
        "colour = 1",
        "`rules.some-rule.colour`: unknown key; expected one of `enforcement-mode`, `banned`, \
         `extend-banned`, `python`"
    )]
    #[case::no_options(
        RuleOptions::none().declared(),
        &[],
        "max = 1",
        "`rules.some-rule.max`: unknown key; this rule takes no options"
    )]
    #[case::count_wrong_type(
        WITH_COUNT.declared(),
        BOTH_LANGUAGES,
        "max-items = \"five\"",
        "`rules.some-rule.max-items`: expected a non-negative integer, found \"five\""
    )]
    #[case::negative_count(
        WITH_COUNT.declared(),
        BOTH_LANGUAGES,
        "max-items = -1",
        "`rules.some-rule.max-items`: expected a non-negative integer, found -1"
    )]
    #[case::list_wrong_type(
        DENYING_RULE.declared(),
        BOTH_LANGUAGES,
        "extend-banned = [\"ok\", 3]",
        "`rules.some-rule.extend-banned`: expected an array of strings, found array"
    )]
    #[case::enforcement_mode_wrong_value(
        WITH_COUNT.declared(),
        BOTH_LANGUAGES,
        "enforcement-mode = \"bann\"",
        "`rules.some-rule.enforcement-mode`: expected `ban` or `require-explanation`, \
         found \"bann\""
    )]
    #[case::language_not_a_table(
        WITH_COUNT.declared(),
        BOTH_LANGUAGES,
        "rust = 1",
        "`rules.some-rule.rust`: expected a table, found 1"
    )]
    #[case::unsupported_language(
        WITH_COUNT.declared(),
        &[SupportLang::Python],
        "[rust]\nmax-items = 1",
        "`rules.some-rule.rust`: this rule does not analyze this language; it analyzes python"
    )]
    #[case::enforcement_mode_rejected(
        RuleOptions::none().declared(),
        BOTH_LANGUAGES,
        "enforcement-mode = \"ban\"",
        "`rules.some-rule.enforcement-mode`: this rule has no enforcement mode; it always \
         reports every finding"
    )]
    fn invalid_options_are_rejected_with_their_key_path(
        #[case] declaration: DeclaredOptions,
        #[case] languages: &[SupportLang],
        #[case] toml_content: &str,
        #[case] message: &str,
    ) {
        let error = parse(&declaration, languages, toml_content).unwrap_err();
        assert_eq!(error.to_string(), message);
    }

    #[test]
    fn rule_value_must_be_a_table() {
        let error = RuleOverrides::parse(
            "some-rule",
            &WITH_COUNT.declared(),
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
