//! Taxonomy queries over the registered rules: derived facets, the unified rule catalog,
//! root-to-leaf facet chains and the single global label registry.

use std::sync::LazyLock;

use strum::{EnumIter, IntoEnumIterator as _, VariantArray as _};
use tagonomy::{Cardinality, Catalog, Item, ItemId, Selector, Tag, TagGroup, TagSet, Vocabulary};

use crate::code_lint::contract::RuleTarget;
use crate::code_lint::rules::CODE_RULES;
use crate::code_lint::suppression::SUPPRESSION_AUDITS;
use crate::command_lint::rules::COMMAND_RULES;
use crate::diagnostic::{Language, RuleName, ViolationTemplate};
use crate::rule_declaration::{
    Classification, Consensus, DeclaredExamples, DeclaredOptions, DeclaredRule, ImpactedQuality,
    RuleDoc, Topic, closest_match,
};

/// The languages facet, derived from the languages a rule analyzes.
const LANGUAGES_GROUP: TagGroup = TagGroup {
    name: "Languages",
    cardinality: Cardinality::Any,
};

/// The analyzed-input facet, derived from the rule's registry.
const INPUT_GROUP: TagGroup = TagGroup {
    name: "Analyzed input",
    cardinality: Cardinality::ExactlyOne,
};

/// The file-scope facet, derived from the rule's file target.
const FILE_SCOPE_GROUP: TagGroup = TagGroup {
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
/// raw message template, and the input it analyzes.
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
    item: ItemId,
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
    /// The rule's unforgeable [`ItemId`] in [`CATALOG`].
    pub(super) const fn item_id(&self) -> ItemId {
        self.item
    }

    /// Canonical labels for the rule's supported languages, or its analyzed-input label when
    /// the rule analyzes commands rather than source files.
    #[must_use]
    pub(crate) fn language_or_input_labels(&self) -> Vec<&'static str> {
        if self.languages.is_empty() {
            vec![input_tag(self.input).label]
        } else {
            self.languages
                .iter()
                .map(|&language| language_tag(language).label)
                .collect()
        }
    }

    /// Non-empty `(facet_name, root_to_leaf_chains)` pairs in display order.
    #[must_use]
    pub(crate) fn facet_chains(&self) -> Vec<(&'static str, Vec<Vec<&'static str>>)> {
        let vocabulary = CATALOG.vocabulary();
        let tags = CATALOG.tags(self.item);
        vocabulary
            .groups()
            .filter_map(|group| {
                let chains = facet_chains_in(vocabulary, tags, group);
                (!chains.is_empty()).then_some((group.name, chains))
            })
            .collect()
    }
}

/// Every topic in use, collected from registered rules and their ancestors, ordered by
/// root-to-leaf path so each parent precedes its children.
#[must_use]
pub fn registered_topics() -> Vec<Topic> {
    let vocabulary = CATALOG.vocabulary();
    let mut topics: Vec<_> = vocabulary.members(Topic::GROUP).collect();
    topics.sort_by_cached_key(|&id| {
        vocabulary
            .root_chain(id)
            .into_iter()
            .map(|step| vocabulary.tag(step).label)
            .collect::<Vec<_>>()
    });
    topics
        .into_iter()
        .map(|id| Topic::from_tag(*vocabulary.tag(id)))
        .collect()
}

/// The canonical label of `selector` in `catalog`: a rule name or a tag label.
pub(super) fn selector_label<T>(catalog: &Catalog<T>, selector: Selector) -> &'static str {
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

/// Root-to-leaf label chains of `tags`'s explicit tags in `facet`, in declaration order.
fn facet_chains_in(
    vocabulary: &Vocabulary,
    tags: &TagSet,
    facet: &TagGroup,
) -> Vec<Vec<&'static str>> {
    tags.explicit()
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

/// Builds the static rule catalog, panicking only if a static rule registration is invalid.
#[expect(
    clippy::panic,
    reason = "the registry is static and `the_registered_rules_form_a_valid_catalog` reports \
              every error in `cargo test`"
)]
fn registered_catalog() -> Catalog<RegisteredRule> {
    build_rule_catalog(registered_items())
        .unwrap_or_else(|errors| panic!("invalid rule taxonomy:\n{}", errors.join("\n")))
}

/// Builds a validated [`Catalog`] from `items`, tagged from every topic they use and every
/// declared and derived facet value; returns formatted error messages otherwise.
fn build_rule_catalog(
    items: impl IntoIterator<Item = Item<(DeclaredRule, AnalyzedInput)>>,
) -> Result<Catalog<RegisteredRule>, Vec<String>> {
    let items: Vec<_> = items.into_iter().collect();
    let mut topics: Vec<Tag> = items
        .iter()
        .flat_map(|item| {
            let (declared, _) = &item.value;
            declared.classification.topics.iter().copied()
        })
        .flat_map(|topic| std::iter::successors(Some(topic), |current| current.parent()))
        .map(Topic::tag)
        .collect();
    topics.sort_by_key(|topic| topic.label);
    topics.dedup();
    let tags = topics
        .into_iter()
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
    let vocabulary = Vocabulary::new(tags).map_err(format_errors)?;
    Catalog::new_with(vocabulary, items, |item, _, (declared, input)| {
        RegisteredRule {
            name: declared.name,
            doc: declared.doc,
            template: declared.template,
            options: declared.options,
            languages: declared.languages,
            classification: declared.classification,
            input,
            item,
        }
    })
    .map_err(format_errors)
}

/// Every registered rule item (code rules, suppression audits, command rules), in registry order.
fn registered_items() -> Vec<Item<(DeclaredRule, AnalyzedInput)>> {
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
) -> Item<(DeclaredRule, AnalyzedInput)> {
    let Classification {
        topics,
        consensus,
        impacted_quality,
    } = declared.classification;
    let declared_tags = topics
        .iter()
        .map(|topic| topic.tag())
        .chain([consensus.tag(), impacted_quality.tag()]);
    let languages = declared.languages.iter().copied().map(language_tag);
    let tags: Vec<Tag> = declared_tags
        .chain(languages)
        .chain([input_tag(input)])
        .chain(file_scope_tag(target))
        .collect();
    Item::new(declared.name.0, tags, (declared, input))
}

/// Formats `errors` as display strings.
fn format_errors(errors: Vec<impl ToString>) -> Vec<String> {
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
        let errors = build_rule_catalog(registered_items())
            .err()
            .unwrap_or_default();
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
        let labels: Vec<_> = registered_topics().into_iter().map(Topic::label).collect();
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
            .filter(|topic| {
                topic.description().trim().is_empty() || topic.scope_note().trim().is_empty()
            })
            .map(Topic::label)
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
                .any(|language| CATALOG.tags(item).carries(language));
            assert_eq!(
                has_language,
                CATALOG.tags(item).carries(code),
                "{}",
                CATALOG.name(item)
            );
        }
    }

    #[test]
    fn facet_chains_expand_topics_from_root_to_leaf() {
        const DOC: crate::rule_declaration::TopicDoc = crate::rule_declaration::TopicDoc {
            description: "Doc.",
            scope_note: "Doc.",
        };
        const PARENT: Topic = Topic::root("parent", DOC);
        const CHILD: Topic = Topic::sub("child", &PARENT, DOC);
        let vocabulary = Vocabulary::new([
            CHILD.tag(),
            Consensus::Unopinionated.tag(),
            ImpactedQuality::Reliability.tag(),
        ])
        .unwrap();
        let tags = TagSet::new(
            &vocabulary,
            [
                CHILD.tag(),
                Consensus::Unopinionated.tag(),
                ImpactedQuality::Reliability.tag(),
            ],
        )
        .unwrap();
        assert_eq!(
            facet_chains_in(&vocabulary, &tags, Topic::GROUP),
            [vec!["parent", "child"]]
        );
    }

    #[test]
    fn registered_rule_facets_and_group_values_preserve_contracts() {
        let code_rule = CATALOG
            .values()
            .iter()
            .find(|rule| rule.name.0 == "sleep-in-tests")
            .unwrap();
        let command_rule = CATALOG
            .values()
            .iter()
            .find(|rule| rule.name.0 == "unbounded-jj-log")
            .unwrap();

        assert_eq!(code_rule.language_or_input_labels(), ["python", "rust"]);
        assert_eq!(command_rule.language_or_input_labels(), ["command"]);
        assert!(group_values(Topic::GROUP).contains("abbreviated-names, architecture"));
        assert!(
            code_rule.facet_chains().iter().any(
                |&(facet, ref chains)| facet == "File scope" && chains == &[vec!["tests-only"]]
            )
        );
    }

    #[test]
    fn build_rule_catalog_formats_taxonomy_violations() {
        let mut invalid = registered_items();
        invalid[0].tags.retain(|tag| !tag.belongs_to(Topic::GROUP));
        let errors = build_rule_catalog(invalid).unwrap_err();
        assert!(
            errors
                .iter()
                .any(|message| message.contains("carries no tag of group `Topic`"))
        );
    }
}
