//! Taxonomy queries over the registered rules: derived facets, the unified rule catalog,
//! root-to-leaf facet chains and the single global label registry.

use std::sync::LazyLock;

use strum::{EnumIter, IntoEnumIterator as _, VariantArray as _};
use tagonomy::{Cardinality, Catalog, Item, Selector, Tag, TagGroup, Vocabulary};

use crate::code_lint::contract::RuleTarget;
use crate::code_lint::rules::CODE_RULES;
use crate::code_lint::suppression::SUPPRESSION_AUDITS;
use crate::command_lint::rules::COMMAND_RULES;
use crate::diagnostic::{Language, RuleName, ViolationTemplate};
use crate::rule_declaration::{
    Classification, Consensus, DeclaredExamples, DeclaredOptions, DeclaredRule, ImpactedQuality,
    Precision, RuleDoc, Topic, closest_match,
};

/// The languages facet, derived from the languages a rule analyzes.
pub const LANGUAGES_GROUP: TagGroup = TagGroup {
    name: "Languages",
    cardinality: Cardinality::Any,
};

/// The analyzed-input facet, derived from the rule's registry.
pub const INPUT_GROUP: TagGroup = TagGroup {
    name: "Analyzed input",
    cardinality: Cardinality::ExactlyOne,
};

/// The file-scope facet, derived from the rule's file target.
pub const FILE_SCOPE_GROUP: TagGroup = TagGroup {
    name: "File scope",
    cardinality: Cardinality::AtMostOne,
};

const PYTHON: Tag = Tag::new(&LANGUAGES_GROUP, "python", "Supports Python sources.");
const RUST: Tag = Tag::new(&LANGUAGES_GROUP, "rust", "Supports Rust sources.");
const CODE: Tag = Tag::new(
    &INPUT_GROUP,
    "code",
    "Analyzes source files (suppression audits included).",
);
const COMMAND: Tag = Tag::new(&INPUT_GROUP, "command", "Analyzes shell commands.");
const TESTS_ONLY: Tag = Tag::new(&FILE_SCOPE_GROUP, "tests-only", "Runs on test files only.");
const SOURCE_ONLY: Tag = Tag::new(
    &FILE_SCOPE_GROUP,
    "source-only",
    "Runs on non-test files only.",
);

/// The catalog of every registered rule (code rules, suppression audits, command rules), in
/// registry order.
pub(super) static CATALOG: LazyLock<Catalog<RegisteredRule>> = LazyLock::new(registered_catalog);

/// A registered rule: its name, declared classification, supported languages, options, doc,
/// raw message template and the input it analyzes.
#[derive(Debug)]
pub struct RegisteredRule {
    /// The rule's name.
    pub name: RuleName,
    /// The rule's user-facing doc.
    pub doc: RuleDoc<DeclaredExamples>,
    /// The rule's raw violation template.
    pub template: &'static ViolationTemplate,
    /// What the rule accepts under `[rules.<name>]`.
    pub options: DeclaredOptions,
    /// The languages the rule analyzes; empty for command rules.
    pub languages: &'static [Language],
    /// The rule's declared classification facets.
    pub classification: Classification,
    /// What the rule analyzes, from the registry it is registered in; the source of its
    /// analyzed-input tag.
    pub input: AnalyzedInput,
}

/// What a rule analyzes, given by the registry it is registered in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, EnumIter)]
pub enum AnalyzedInput {
    /// Source files: code rules and suppression audits.
    Code,
    /// Shell commands: command rules.
    Command,
}

impl RegisteredRule {
    /// Root-to-leaf label chains of the rule's explicit tags in `facet`, in declaration order.
    #[must_use]
    pub fn facet_chains(&self, facet: &TagGroup) -> Vec<Vec<&'static str>> {
        let Some(item) = CATALOG.item(self.name.0) else {
            return Vec::new();
        };
        let vocabulary = CATALOG.vocabulary();
        CATALOG
            .explicit(item)
            .iter()
            .copied()
            .filter(|&tag| vocabulary.tag(tag).belongs_to(facet))
            .map(|tag| {
                vocabulary
                    .root_chain(tag)
                    .into_iter()
                    .map(|step| vocabulary.tag(step).label)
                    .collect()
            })
            .collect()
    }
}

/// Every facet, in display order: the vocabulary's groups, in the order `catalog` adds
/// their tags. A facet's name is for display only and is never a selector.
pub fn facets() -> impl Iterator<Item = &'static TagGroup> {
    CATALOG.vocabulary().groups()
}

/// The tags of every topic in use (the topics rules carry and their ancestors), ordered by
/// root-to-leaf path so each parent precedes its children.
#[must_use]
pub fn registered_topics() -> Vec<Tag> {
    let vocabulary = CATALOG.vocabulary();
    let mut topics: Vec<_> = vocabulary.members(Topic::GROUP).collect();
    topics.sort_by_cached_key(|&id| {
        vocabulary
            .root_chain(id)
            .into_iter()
            .map(|step| vocabulary.tag(step).label)
            .collect::<Vec<_>>()
    });
    topics.into_iter().map(|id| *vocabulary.tag(id)).collect()
}

/// The canonical label of `selector` in `catalog`: a rule name or a tag label.
pub(super) fn selector_label(
    catalog: &Catalog<RegisteredRule>,
    selector: Selector,
) -> &'static str {
    match selector {
        Selector::Item(item) => catalog.name(item),
        Selector::Tag(tag) => catalog.vocabulary().tag(tag).label,
    }
}

/// The labels of `group`'s tags, comma-separated.
pub(super) fn group_values(group: &TagGroup) -> String {
    let vocabulary = CATALOG.vocabulary();
    let labels: Vec<_> = vocabulary
        .members(group)
        .map(|id| vocabulary.tag(id).label)
        .collect();
    labels.join(", ")
}

/// The registered label closest to `label`, if it is close enough to be a typo.
pub(super) fn closest_label(label: &str) -> Option<&'static str> {
    closest_match(label, CATALOG.labels())
}

/// The registered rule name closest to `label`, if it is close enough to be a typo.
pub(super) fn closest_rule_name(label: &str) -> Option<&'static str> {
    closest_match(label, CATALOG.values().iter().map(|rule| rule.name.0))
}

/// Builds a validated [`Catalog`] from `items`, tagged from every topic they use and every
/// declared and derived facet value; returns formatted error messages otherwise.
pub(super) fn catalog(
    items: impl IntoIterator<Item = Item<RegisteredRule>>,
) -> Result<Catalog<RegisteredRule>, Vec<String>> {
    let items: Vec<_> = items.into_iter().collect();
    let mut topics: Vec<Tag> = items
        .iter()
        .flat_map(|item| item.value.classification.topics.iter().copied())
        .flat_map(|topic| std::iter::successors(Some(topic), |topic| topic.parent()))
        .map(Topic::tag)
        .collect();
    topics.sort_by_key(|topic| topic.label);
    topics.dedup();
    let tags = topics
        .into_iter()
        .chain(Precision::iter().map(Precision::tag))
        .chain(Consensus::iter().map(Consensus::tag))
        .chain(ImpactedQuality::iter().map(ImpactedQuality::tag))
        .chain(Language::VARIANTS.iter().copied().map(language_tag))
        .chain(AnalyzedInput::iter().map(input_tag))
        .chain(
            RuleTarget::VARIANTS
                .iter()
                .copied()
                .filter_map(file_scope_tag),
        );
    let vocabulary = Vocabulary::new(tags).map_err(messages)?;
    Catalog::new(vocabulary, items).map_err(messages)
}

/// A synthetic rule item outside the registry, analyzing code, with no other derived facets.
#[cfg(test)]
pub(super) fn synthetic_item(
    name: &'static str,
    classification: Classification,
) -> Item<RegisteredRule> {
    static TEMPLATE: ViolationTemplate = ViolationTemplate::from_static("", "", "");
    let declared = DeclaredRule {
        name: RuleName(name),
        template: &TEMPLATE,
        languages: &[],
        options: crate::rule_declaration::RuleOptions::none().declared(),
        classification,
        doc: RuleDoc::TODO.declared(),
    };
    rule_item(declared, AnalyzedInput::Code, RuleTarget::All)
}

/// Builds the static rule catalog, panicking only if a static rule registration is invalid.
#[expect(
    clippy::panic,
    reason = "the registry is static and `the_registered_rules_form_a_valid_catalog` reports \
              every error in `cargo test`"
)]
fn registered_catalog() -> Catalog<RegisteredRule> {
    catalog(registered_items())
        .unwrap_or_else(|errors| panic!("invalid rule taxonomy:\n{}", errors.join("\n")))
}

/// Every registered rule item (code rules, suppression audits, command rules), in registry order.
fn registered_items() -> Vec<Item<RegisteredRule>> {
    let code = CODE_RULES
        .iter()
        .map(|rule| rule_item(rule.declaration(), AnalyzedInput::Code, rule.target()));
    let audits = SUPPRESSION_AUDITS
        .iter()
        .map(|audit| rule_item(audit.declared(), AnalyzedInput::Code, RuleTarget::All));
    let commands = COMMAND_RULES.iter().map(|rule| {
        rule_item(
            rule.declaration.declared(),
            AnalyzedInput::Command,
            RuleTarget::All,
        )
    });
    code.chain(audits).chain(commands).collect()
}

/// Pairs `declared` with its explicit tags in branch-precedence order: topics, declared facet
/// values, language tags, the analyzed-input tag, then the file-scope tag.
fn rule_item(
    declared: DeclaredRule,
    input: AnalyzedInput,
    target: RuleTarget,
) -> Item<RegisteredRule> {
    let Classification {
        topics,
        precision,
        consensus,
        impacted_quality,
    } = declared.classification;
    let declared_tags = topics.iter().map(|topic| topic.tag()).chain([
        precision.tag(),
        consensus.tag(),
        impacted_quality.tag(),
    ]);
    let languages = declared.languages.iter().copied().map(language_tag);
    let tags: Vec<Tag> = declared_tags
        .chain(languages)
        .chain([input_tag(input)])
        .chain(file_scope_tag(target))
        .collect();
    let rule = RegisteredRule {
        name: declared.name,
        doc: declared.doc,
        template: declared.template,
        options: declared.options,
        languages: declared.languages,
        classification: declared.classification,
        input,
    };
    Item::new(declared.name.0, tags, rule)
}

/// Formats `errors` as display strings.
fn messages(errors: Vec<impl ToString>) -> Vec<String> {
    errors.into_iter().map(|error| error.to_string()).collect()
}

const fn language_tag(language: Language) -> Tag {
    match language {
        Language::Python => PYTHON,
        Language::Rust => RUST,
    }
}

const fn input_tag(input: AnalyzedInput) -> Tag {
    match input {
        AnalyzedInput::Code => CODE,
        AnalyzedInput::Command => COMMAND,
    }
}

const fn file_scope_tag(target: RuleTarget) -> Option<Tag> {
    match target {
        RuleTarget::All => None,
        RuleTarget::TestsOnly => Some(TESTS_ONLY),
        RuleTarget::SourceOnly => Some(SOURCE_ONLY),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rule_declaration::is_kebab_case;

    #[test]
    fn the_registered_rules_form_a_valid_catalog() {
        let errors = catalog(registered_items()).err().unwrap_or_default();
        assert!(errors.is_empty(), "{}", errors.join("\n"));
    }

    #[test]
    fn every_tag_has_a_rule() {
        let vocabulary = CATALOG.vocabulary();
        let empty: Vec<_> = CATALOG
            .unused_tags()
            .map(|tag| vocabulary.tag(tag).label)
            .collect();
        assert!(empty.is_empty(), "no rule carries {empty:?}: remove them");
    }

    #[test]
    fn registered_topics_list_ancestors_before_their_subtopics() {
        let labels: Vec<_> = registered_topics().iter().map(|tag| tag.label).collect();
        let testing_subtree = ["testing", "test-assertions", "test-doubles", "test-timing"];
        assert!(
            labels.windows(4).any(|window| window == testing_subtree),
            "{labels:?}"
        );
    }

    #[test]
    fn every_topic_is_documented() {
        let undocumented: Vec<_> = registered_topics()
            .into_iter()
            .filter(|tag| {
                tag.description.trim().is_empty()
                    || tag.scope_note.is_none_or(|note| note.trim().is_empty())
            })
            .map(|tag| tag.label)
            .collect();
        assert!(undocumented.is_empty(), "document {undocumented:?}");
    }

    #[test]
    fn labels_are_kebab_case() {
        let non_kebab: Vec<_> = CATALOG
            .labels()
            .filter(|label| !is_kebab_case(label))
            .collect();
        assert!(non_kebab.is_empty(), "{non_kebab:?} are not kebab-case");
    }

    #[test]
    fn code_rules_and_only_code_rules_have_a_language() {
        let vocabulary = CATALOG.vocabulary();
        let code = vocabulary.id(CODE.label).unwrap();
        // omni:ignore[logic-in-tests] -- verifies language facet invariant across all registered rules
        for item in CATALOG.items() {
            let has_language = vocabulary
                .members(&LANGUAGES_GROUP)
                .any(|language| CATALOG.carries(item, language));
            assert_eq!(
                has_language,
                CATALOG.carries(item, code),
                "{}",
                CATALOG.name(item)
            );
        }
    }

    #[test]
    fn facet_chains_expand_topics_from_root_to_leaf() {
        let item = CATALOG.item("sleep-in-tests").unwrap();
        let topic_group = Topic::TEST_TIMING.tag().group;
        assert_eq!(
            CATALOG[item].facet_chains(topic_group),
            [vec!["testing", "test-timing"]]
        );
        assert_eq!(
            CATALOG[item].facet_chains(&FILE_SCOPE_GROUP),
            [vec!["tests-only"]]
        );
    }
}
