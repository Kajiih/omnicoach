//! Taxonomy queries over the registered rules: derived facets, topic ancestry, branches and
//! the single global label registry.

use std::collections::BTreeSet;
use std::sync::LazyLock;

use strum::{EnumIter, IntoEnumIterator as _, IntoStaticStr};

use crate::code_lint::contract::RuleTarget;
use crate::code_lint::rules::CODE_RULES;
use crate::code_lint::suppression::SUPPRESSION_AUDITS;
use crate::command_lint::rules::COMMAND_RULES;
use crate::diagnostic::{Language, RuleName, ViolationTemplate};
use crate::rule_declaration::{
    Classification, Consensus, DeclaredOptions, DeclaredRule, ImpactedQuality, Precision, RuleDoc,
    Topic, closest_match,
};

/// A value of a derived facet, computed from the rule and never declared.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, EnumIter, IntoStaticStr)]
#[strum(serialize_all = "kebab-case")]
pub enum Derived {
    /// Supports Python sources.
    Python,
    /// Supports Rust sources.
    Rust,
    /// Analyzes source files (suppression audits included).
    Code,
    /// Analyzes shell commands.
    Command,
    /// Runs on test files only.
    TestsOnly,
    /// Runs on non-test files only.
    SourceOnly,
}

/// A facet. Its label is for display only and is never a selector.
#[derive(Debug, Clone, Copy, PartialEq, Eq, EnumIter)]
pub enum Facet {
    /// Declared, multi-valued, hierarchical.
    Topic,
    /// Declared, single-valued.
    Precision,
    /// Declared, single-valued.
    Consensus,
    /// Declared, single-valued.
    ImpactedQuality,
    /// Derived from the rule's supported languages.
    Languages,
    /// Derived from the rule's registry (code or command).
    Input,
    /// Derived from the rule's file target.
    FileScope,
}

impl Facet {
    /// Whether `candidate` names this facet (case-insensitively, spaces/underscores/hyphens
    /// equivalent).
    #[must_use]
    pub fn matches_label(self, candidate: &str) -> bool {
        let normalize = |byte: u8| match byte {
            b' ' | b'_' => b'-',
            other => other.to_ascii_lowercase(),
        };
        let label = self.label().as_bytes();
        let candidate = candidate.as_bytes();
        label.len() == candidate.len()
            && label
                .iter()
                .zip(candidate)
                .all(|(&left, &right)| normalize(left) == normalize(right))
    }

    /// The display label.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Topic => "Topic",
            Self::Precision => "Precision",
            Self::Consensus => "Consensus",
            Self::ImpactedQuality => "Impacted quality",
            Self::Languages => "Languages",
            Self::Input => "Analyzed input",
            Self::FileScope => "File scope",
        }
    }
}

/// One value of one facet: what a tag selector names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Tag {
    /// A topic.
    Topic(Topic),
    /// A precision value.
    Precision(Precision),
    /// A consensus value.
    Consensus(Consensus),
    /// An impacted-quality value.
    ImpactedQuality(ImpactedQuality),
    /// A derived value.
    Derived(Derived),
}

impl Tag {
    /// The tag's canonical label, e.g. `heuristic`.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Topic(value) => value.label,
            Self::Precision(value) => value.into(),
            Self::Consensus(value) => value.into(),
            Self::ImpactedQuality(value) => value.into(),
            Self::Derived(value) => value.into(),
        }
    }

    /// The facet the tag belongs to.
    #[must_use]
    pub const fn facet(self) -> Facet {
        match self {
            Self::Topic(_) => Facet::Topic,
            Self::Precision(_) => Facet::Precision,
            Self::Consensus(_) => Facet::Consensus,
            Self::ImpactedQuality(_) => Facet::ImpactedQuality,
            Self::Derived(Derived::Python | Derived::Rust) => Facet::Languages,
            Self::Derived(Derived::Code | Derived::Command) => Facet::Input,
            Self::Derived(Derived::TestsOnly | Derived::SourceOnly) => Facet::FileScope,
        }
    }
}

/// A validated, canonical selector: a rule name or a tag (synonyms already resolved).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Selector {
    /// A rule name: the leaf under every tag the rule carries.
    Rule(RuleName),
    /// A tag, with all its descendants.
    Tag(Tag),
}

impl Selector {
    /// The canonical label.
    pub fn label(self) -> &'static str {
        match self {
            Self::Rule(rule) => rule.0,
            Self::Tag(tag) => tag.label(),
        }
    }
}

/// A registered rule as the taxonomy sees it: its name, declared and derived facets, doc and
/// raw message template.
#[derive(Debug)]
pub struct RegisteredRule {
    /// The rule's name.
    pub name: RuleName,
    /// The rule's user-facing doc.
    pub doc: RuleDoc,
    /// The rule's raw violation template.
    pub template: &'static ViolationTemplate,
    /// What the rule accepts under `[rules.<name>]`.
    pub options: DeclaredOptions,
    /// The languages the rule analyzes; empty for command rules.
    pub languages: &'static [Language],
    classification: Classification,
    derived: Vec<Derived>,
}

impl RegisteredRule {
    /// The rule declared by `declared`, with its language facets and the `registry` facets.
    fn new(declared: DeclaredRule, registry: impl IntoIterator<Item = Derived>) -> Self {
        let languages = declared.languages.iter().map(|language| match language {
            Language::Python => Derived::Python,
            Language::Rust => Derived::Rust,
        });
        Self {
            name: declared.name,
            doc: declared.doc,
            template: declared.template,
            options: declared.options,
            languages: declared.languages,
            classification: declared.classification,
            derived: languages.chain(registry).collect(),
        }
    }
}

/// Every registered rule (code rules, suppression audits, command rules).
pub static REGISTERED_RULES: LazyLock<Vec<RegisteredRule>> = LazyLock::new(|| {
    let code = CODE_RULES.iter().map(|rule| {
        let scope = match rule.target() {
            RuleTarget::All => None,
            RuleTarget::TestsOnly => Some(Derived::TestsOnly),
            RuleTarget::SourceOnly => Some(Derived::SourceOnly),
        };
        RegisteredRule::new(
            rule.declaration(),
            std::iter::once(Derived::Code).chain(scope),
        )
    });
    let audits = SUPPRESSION_AUDITS
        .iter()
        .map(|audit| RegisteredRule::new(audit.declared(), [Derived::Code]));
    let commands = COMMAND_RULES
        .iter()
        .map(|rule| RegisteredRule::new(rule.declaration.declared(), [Derived::Command]));
    code.chain(audits).chain(commands).collect()
});

impl Topic {
    /// The path from the root down to the topic.
    fn path(self) -> Vec<Self> {
        let mut path: Vec<_> = self.ancestors().collect();
        path.reverse();
        path.push(self);
        path
    }

    /// The topic's ancestors, nearest first.
    fn ancestors(self) -> impl Iterator<Item = Self> {
        std::iter::successors(self.parent.copied(), |topic| topic.parent.copied())
    }
}

/// Every tag of every facet.
pub fn all_tags() -> impl Iterator<Item = Tag> {
    all_topics()
        .map(Tag::Topic)
        .chain(Precision::iter().map(Tag::Precision))
        .chain(Consensus::iter().map(Tag::Consensus))
        .chain(ImpactedQuality::iter().map(Tag::ImpactedQuality))
        .chain(Derived::iter().map(Tag::Derived))
}

impl RegisteredRule {
    /// A rule outside the registry, with no derived facets.
    #[cfg(test)]
    pub(crate) fn synthetic(name: &'static str, classification: Classification) -> Self {
        static TEMPLATE: ViolationTemplate = ViolationTemplate::from_static("", "", "");
        Self {
            name: RuleName(name),
            doc: RuleDoc::TODO,
            template: &TEMPLATE,
            options: crate::rule_declaration::RuleOptions::none().declared(),
            languages: &[],
            classification,
            derived: Vec::new(),
        }
    }

    /// Whether `selector` selects the rule.
    #[must_use]
    pub fn matches(&self, selector: Selector) -> bool {
        match selector {
            Selector::Rule(name) => name == self.name,
            Selector::Tag(tag) => self.branches().iter().flatten().any(|&own| own == tag),
        }
    }

    /// The rule's branches: each root-to-leaf tag path it sits on. A topic branch is the
    /// topic's path; every other facet value is a one-tag branch. The rule is the implicit leaf.
    pub fn branches(&self) -> Vec<Vec<Tag>> {
        let declared = self.classification;
        let topic_paths = declared
            .topics
            .iter()
            .map(|topic| topic.path().into_iter().map(Tag::Topic).collect());
        let values = [
            Tag::Precision(declared.precision),
            Tag::Consensus(declared.consensus),
            Tag::ImpactedQuality(declared.impacted_quality),
        ]
        .into_iter()
        .chain(self.derived.iter().copied().map(Tag::Derived))
        .map(|tag| vec![tag]);
        topic_paths.chain(values).collect()
    }
}

/// Resolves a label (canonical or synonym) to its canonical selector.
pub fn lookup(label: &str) -> Option<Selector> {
    labels().find_map(|(known, selector)| (known == label).then_some(selector))
}

/// The registered label closest to `label`, if it is close enough to be a typo.
pub fn closest_label(label: &str) -> Option<&'static str> {
    closest_match(label, labels().map(|(known, _)| known))
}

/// The registered rule name closest to `label`, if it is close enough to be a typo.
pub fn closest_rule_name(label: &str) -> Option<&'static str> {
    closest_match(label, REGISTERED_RULES.iter().map(|rule| rule.name.0))
}

/// The single global label registry: rule names, tag labels and topic synonyms.
fn labels() -> impl Iterator<Item = (&'static str, Selector)> {
    let rules = REGISTERED_RULES
        .iter()
        .map(|rule| (rule.name.0, Selector::Rule(rule.name)));
    let tags = all_tags().map(|tag| (tag.label(), Selector::Tag(tag)));
    let synonyms = all_topics().flat_map(|topic| {
        let selector = Selector::Tag(Tag::Topic(topic));
        topic
            .synonyms
            .iter()
            .map(move |&synonym| (synonym, selector))
    });
    rules.chain(tags).chain(synonyms)
}

/// Every topic in use, collected from registered rules and their ancestors.
fn all_topics() -> impl Iterator<Item = Topic> {
    let topics: BTreeSet<Topic> = REGISTERED_RULES
        .iter()
        .flat_map(|rule| rule.classification.topics.iter().copied())
        .flat_map(Topic::path)
        .collect();
    topics.into_iter()
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::rule_declaration::is_kebab_case;

    #[test]
    fn every_rule_has_a_topic() {
        let untopical: Vec<_> = REGISTERED_RULES
            .iter()
            .filter(|rule| rule.classification.topics.is_empty())
            .map(|rule| rule.name.0)
            .collect();
        assert!(
            untopical.is_empty(),
            "declare at least one topic for {untopical:?}"
        );
    }

    #[test]
    fn no_rule_lists_a_topic_with_its_ancestor_or_twice() {
        let mut failures = Vec::new();
        for rule in REGISTERED_RULES.iter() {
            let topics = rule.classification.topics;
            for (index, &topic) in topics.iter().enumerate() {
                if topics[..index].contains(&topic) {
                    failures.push(format!("{}: `{}` is listed twice", rule.name, topic.label));
                }
                if let Some(ancestor) = topic.ancestors().find(|ancestor| topics.contains(ancestor))
                {
                    failures.push(format!(
                        "{}: remove `{}`, it is implied by `{}`",
                        rule.name, ancestor.label, topic.label
                    ));
                }
            }
        }
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }

    #[test]
    fn every_tag_has_a_rule() {
        let empty: Vec<_> = all_tags()
            .filter(|&tag| {
                !REGISTERED_RULES
                    .iter()
                    .any(|rule| rule.matches(Selector::Tag(tag)))
            })
            .map(Tag::label)
            .collect();
        assert!(empty.is_empty(), "no rule carries {empty:?}: remove them");
    }

    #[test]
    fn every_topic_is_documented() {
        // Facet value enums are covered by the crate's `missing_docs` lint.
        let undocumented: Vec<_> = all_topics()
            .filter(|topic| {
                topic.description.trim().is_empty() || topic.scope_note.trim().is_empty()
            })
            .map(|topic| topic.label)
            .collect();
        assert!(undocumented.is_empty(), "document {undocumented:?}");
    }

    #[test]
    fn topic_tree_table_matches_the_topics() {
        const GUIDE: &str = include_str!("../../docs/dev/tag_guide.md");
        let (_, section) = GUIDE
            .split_once("## 5. Current topic tree")
            .expect("tag_guide.md has a topic tree section");
        let (section, _) = section
            .split_once("\n## ")
            .expect("the topic tree section is followed by another section");
        let documented: BTreeSet<Vec<String>> = section
            .lines()
            .filter(|line| line.starts_with("| `"))
            .map(|row| {
                row.trim_matches('|')
                    .split('|')
                    .map(|cell| cell.trim().to_owned())
                    .collect()
            })
            .collect();
        let declared: BTreeSet<Vec<String>> = all_topics()
            .map(|topic| {
                let synonyms: Vec<_> = topic
                    .synonyms
                    .iter()
                    .map(|synonym| format!("`{synonym}`"))
                    .collect();
                vec![
                    format!("`{}`", topic.label),
                    topic.parent.map_or("—", |parent| parent.label).to_owned(),
                    synonyms.join(", "),
                    topic.description.to_owned(),
                    topic.scope_note.to_owned(),
                ]
            })
            .collect();
        let stale: Vec<_> = documented.difference(&declared).collect();
        assert!(
            stale.is_empty(),
            "fix or remove in tag_guide.md §5: {stale:#?}"
        );
        let missing: Vec<_> = declared.difference(&documented).collect();
        assert!(missing.is_empty(), "add to tag_guide.md §5: {missing:#?}");
    }

    #[test]
    fn labels_are_globally_unique_and_not_facet_labels() {
        let mut owners: BTreeMap<&str, Vec<Selector>> = BTreeMap::new();
        for (label, selector) in labels() {
            owners.entry(label).or_default().push(selector);
        }
        let collisions: Vec<_> = owners
            .iter()
            .filter(|(_, selectors)| selectors.len() > 1)
            .map(|(label, selectors)| format!("`{label}` names {selectors:?}"))
            .collect();
        assert!(collisions.is_empty(), "{}", collisions.join("\n"));
        let non_kebab: Vec<_> = owners
            .keys()
            .filter(|label| !is_kebab_case(label))
            .collect();
        assert!(non_kebab.is_empty(), "{non_kebab:?} are not kebab-case");
        let facet_like: Vec<_> = owners
            .keys()
            .filter(|label| Facet::iter().any(|facet| facet.matches_label(label)))
            .collect();
        assert!(facet_like.is_empty(), "{facet_like:?} read as facet labels");
    }

    #[test]
    fn code_rules_and_only_code_rules_have_a_language() {
        for rule in REGISTERED_RULES.iter() {
            let has_language = rule
                .derived
                .iter()
                .any(|&value| Tag::Derived(value).facet() == Facet::Languages);
            assert_eq!(
                has_language,
                rule.derived.contains(&Derived::Code),
                "{}",
                rule.name
            );
        }
    }

    #[test]
    fn a_topic_branch_is_its_path_from_the_root() {
        const PARENT: Topic = Topic {
            label: "parent",
            parent: None,
            description: "",
            scope_note: "",
            synonyms: &[],
        };
        const CHILD: Topic = Topic {
            label: "child",
            parent: Some(&PARENT),
            ..PARENT
        };
        let rule = RegisteredRule::synthetic(
            "rule",
            Classification {
                topics: &[CHILD],
                precision: Precision::Exact,
                consensus: Consensus::Unopinionated,
                impacted_quality: ImpactedQuality::Reliability,
            },
        );
        assert_eq!(
            rule.branches(),
            [
                vec![Tag::Topic(PARENT), Tag::Topic(CHILD)],
                vec![Tag::Precision(Precision::Exact)],
                vec![Tag::Consensus(Consensus::Unopinionated)],
                vec![Tag::ImpactedQuality(ImpactedQuality::Reliability)],
            ]
        );
    }
}
