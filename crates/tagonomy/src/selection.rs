//! Include/exclude rules over a [`Catalog`] combining exact item overrides with tag rules.

use std::collections::{BTreeMap, BTreeSet};

use crate::catalog::{Catalog, ItemId, Selector};
use crate::tag_selection::{TagReason, TagSelection, Verdict};
use crate::vocabulary::TagId;

/// A [`Selector`] given both [`Verdict::Include`] and [`Verdict::Exclude`], which
/// [`Selection::new`] and [`Selection::override_with`] reject.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Conflict {
    /// The conflicting selector.
    pub selector: Selector,
}

/// A [`Selection`]'s verdict on a catalog item, and why.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Decision {
    /// The verdict.
    pub verdict: Verdict,
    /// What decided it.
    pub reason: Reason,
}

/// What decided a [`Decision`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Reason {
    /// No directive selected the item: the selection's default.
    Default,
    /// A directive naming the item directly.
    Item,
    /// The directive on `tag`, applied through the item's explicit tag `source`.
    /// [`crate::Vocabulary::chain`] lists the implications from `tag` down to `source`, and
    /// [`crate::Vocabulary::chain_through`] prepends the root chain above `tag`.
    Tag {
        /// The directive's tag.
        tag: TagId,
        /// The item's explicit tag that implies `tag` (`tag` itself when explicit).
        source: TagId,
    },
}

/// Include/exclude rules over a [`Catalog`] where exact item names beat tags and narrower tags
/// override broader ones.
///
/// [`Self::decide`] evaluates an item in three steps:
///
/// 1. If a directive names the item directly, that directive wins over all tags.
/// 2. Otherwise, [`Self::tags`] checks the item's tags: along each tag's parent chain, the most
///    specific matching tag wins; if two branches disagree, [`Verdict::Exclude`] wins.
/// 3. If no directive matches, the selection's default verdict applies.
#[derive(Debug)]
pub struct Selection<'a, T = ()> {
    catalog: &'a Catalog<T>,
    items: BTreeMap<ItemId, Verdict>,
    tags: TagSelection<'a>,
}

impl<T> Clone for Selection<'_, T> {
    fn clone(&self) -> Self {
        Self {
            catalog: self.catalog,
            items: self.items.clone(),
            tags: self.tags.clone(),
        }
    }
}

impl<'a, T> Selection<'a, T> {
    /// Builds the selection over `catalog` deciding `default` unless overridden by
    /// `directives`.
    ///
    /// # Errors
    ///
    /// Returns every [`Conflict`], in selector order (items before tags, then by id), if any
    /// selector is given both verdicts.
    pub fn new(
        catalog: &'a Catalog<T>,
        default: Verdict,
        directives: impl IntoIterator<Item = (Selector, Verdict)>,
    ) -> Result<Self, Vec<Conflict>> {
        let mut items = BTreeMap::new();
        let mut tag_directives = Vec::new();
        let mut conflicting = BTreeSet::new();

        for (selector, verdict) in directives {
            match selector {
                Selector::Item(item) => {
                    let _ = catalog.name(item);
                    if items
                        .insert(item, verdict)
                        .is_some_and(|existing| existing != verdict)
                    {
                        conflicting.insert(selector);
                    }
                }
                Selector::Tag(tag) => tag_directives.push((tag, verdict)),
            }
        }
        let tags = match TagSelection::new(catalog.vocabulary(), default, tag_directives) {
            Ok(built) => Some(built),
            Err(tag_conflicts) => {
                conflicting.extend(
                    tag_conflicts
                        .into_iter()
                        .map(|conflict| Selector::Tag(conflict.tag)),
                );
                None
            }
        };
        match tags {
            Some(built) if conflicting.is_empty() => Ok(Self {
                catalog,
                items,
                tags: built,
            }),
            _ => Err(conflicting
                .into_iter()
                .map(|selector| Conflict { selector })
                .collect()),
        }
    }

    /// Builds a new selection by applying `directives` on top of `self`.
    ///
    /// Keeps `self`'s default verdict and every directive not mentioned in `directives`; any item
    /// or tag in `directives` replaces only its own previous verdict in `self` without removing
    /// directives on other items or narrower/broader tags (normal specificity precedence still
    /// applies to the merged selection). Repeating the same verdict for a selector within
    /// `directives` is allowed.
    ///
    /// # Errors
    ///
    /// Returns every [`Conflict`], in selector order (items before tags, then by id), if
    /// `directives` itself gives the same selector both verdicts.
    pub fn override_with(
        &self,
        directives: impl IntoIterator<Item = (Selector, Verdict)>,
    ) -> Result<Self, Vec<Conflict>> {
        let layer = Self::new(self.catalog, self.tags.default_verdict(), directives)?;
        let mut items = self.items.clone();
        items.extend(layer.items);
        let tags = self.tags.merge_layer(&layer.tags);
        Ok(Self {
            catalog: self.catalog,
            items,
            tags,
        })
    }

    /// The selection's verdict on `item`, and why.
    #[must_use]
    pub fn decide(&self, item: ItemId) -> Decision {
        self.decide_skipping(item, None)
    }

    /// Whether `self` includes `item` (`self.decide(item).verdict == Verdict::Include`).
    #[must_use]
    pub fn matches(&self, item: ItemId) -> bool {
        self.decide(item).verdict == Verdict::Include
    }

    /// The underlying [`TagSelection`] (excluding item-specific overrides).
    #[must_use]
    pub const fn tags(&self) -> &TagSelection<'a> {
        &self.tags
    }

    /// Returns every directive that has no effect on any item in `catalog`, paired with its
    /// [`Verdict`], in selector order (items before tags, then by id).
    ///
    /// A directive is ineffective when removing it (while keeping all other directives) leaves
    /// every item's final [`Verdict`] unchanged — for example, including a tag that is already
    /// covered by the default verdict or by a broader included tag.
    #[must_use]
    pub fn ineffective(&self) -> Vec<(Selector, Verdict)> {
        let directives: Vec<(Selector, Verdict)> = self
            .items
            .iter()
            .map(|(&item, &verdict)| (Selector::Item(item), verdict))
            .chain(
                self.tags
                    .directives()
                    .map(|(tag, verdict)| (Selector::Tag(tag), verdict)),
            )
            .collect();
        if directives.is_empty() {
            return Vec::new();
        }
        let baseline: Vec<Verdict> = self
            .catalog
            .items()
            .map(|item| self.decide(item).verdict)
            .collect();
        directives
            .into_iter()
            .filter(|&(selector, _)| {
                self.catalog.items().zip(&baseline).all(|(item, &verdict)| {
                    self.decide_skipping(item, Some(selector)).verdict == verdict
                })
            })
            .collect()
    }

    fn decide_skipping(&self, item: ItemId, skipped: Option<Selector>) -> Decision {
        if skipped != Some(Selector::Item(item))
            && let Some(&verdict) = self.items.get(&item)
        {
            return Decision {
                verdict,
                reason: Reason::Item,
            };
        }
        let skipped_tag = match skipped {
            Some(Selector::Tag(tag)) => Some(tag),
            Some(Selector::Item(_)) | None => None,
        };
        let tag_decision = self
            .tags
            .decide_skipping(self.catalog.tags(item), skipped_tag);
        let reason = match tag_decision.reason {
            TagReason::Default => Reason::Default,
            TagReason::Tag { tag, source } => Reason::Tag { tag, source },
        };
        Decision {
            verdict: tag_decision.verdict,
            reason,
        }
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;
    use crate::catalog::Item;
    use crate::tag::{Cardinality, Tag, TagGroup};
    use crate::vocabulary::Vocabulary;

    const TOPIC: TagGroup = TagGroup {
        name: "Topic",
        cardinality: Cardinality::Any,
    };
    const CONSENSUS: TagGroup = TagGroup {
        name: "Consensus",
        cardinality: Cardinality::ExactlyOne,
    };
    const PARENT: Tag = Tag::new(&TOPIC, "parent", "Parent.");
    const CHILD: Tag = Tag::new(&TOPIC, "child", "Child.").implies(&[PARENT]);
    const SIBLING: Tag = Tag::new(&TOPIC, "sibling", "Sibling.").implies(&[PARENT]);
    const OTHER: Tag = Tag::new(&TOPIC, "other", "Other.");
    const OPINIONATED: Tag = Tag::new(&CONSENSUS, "opinionated", "Opinionated.");
    const OS: Tag = Tag::new(&TOPIC, "os", "OS.");
    const LINUX: Tag = Tag::new(&TOPIC, "linux", "Linux.").implies(&[OS]);
    const KERNEL: Tag = Tag::new(&TOPIC, "kernel", "Kernel.").implies(&[OS]);
    const EBPF: Tag = Tag::new(&TOPIC, "ebpf", "eBPF.").implies(&[LINUX, KERNEL]);

    fn one_rule_catalog(tags: &[Tag]) -> Catalog {
        let vocabulary = Vocabulary::new(tags.iter().copied()).unwrap();
        Catalog::new(vocabulary, [Item::named("rule", tags)]).unwrap()
    }

    fn format_decision(
        tags: &[Tag],
        default: Verdict,
        include: &[&str],
        exclude: &[&str],
    ) -> String {
        let catalog = one_rule_catalog(tags);
        let directives = [(include, Verdict::Include), (exclude, Verdict::Exclude)]
            .into_iter()
            .flat_map(|(labels, verdict)| {
                labels
                    .iter()
                    .map(|label| (catalog.resolve(label).unwrap(), verdict))
                    .collect::<Vec<_>>()
            });
        let selection = Selection::new(&catalog, default, directives).unwrap();
        let item = catalog.item("rule").unwrap();
        let decision = selection.decide(item);
        assert_eq!(
            selection.matches(item),
            decision.verdict == Verdict::Include
        );
        let reason = match decision.reason {
            Reason::Default => "default".to_owned(),
            Reason::Item => "item".to_owned(),
            Reason::Tag { tag, source } => {
                let vocabulary = catalog.vocabulary();
                let labels: Vec<_> = vocabulary
                    .chain(source, tag)
                    .into_iter()
                    .map(|id| vocabulary.tag(id).label)
                    .collect();
                labels.join(" > ")
            }
        };
        format!("{:?} ({reason})", decision.verdict)
    }

    #[rstest]
    #[case::default_include(Verdict::Include, &[], &[], "Include (default)")]
    #[case::default_exclude(Verdict::Exclude, &[], &[], "Exclude (default)")]
    #[case::excluded_tag(Verdict::Include, &[], &["other"], "Exclude (other)")]
    #[case::ancestor_included(Verdict::Exclude, &["parent"], &[], "Include (parent > child)")]
    #[case::nearer_include_wins(Verdict::Exclude, &["child"], &["parent"], "Include (child)")]
    #[case::nearer_exclude_wins(Verdict::Exclude, &["parent"], &["child"], "Exclude (child)")]
    #[case::exclude_wins_across_topics(
        Verdict::Exclude,
        &["child"],
        &["other"],
        "Exclude (other)"
    )]
    #[case::exclude_wins_across_groups(
        Verdict::Exclude,
        &["opinionated"],
        &["parent"],
        "Exclude (parent > child)"
    )]
    #[case::included_item_beats_tags(
        Verdict::Exclude,
        &["rule"],
        &["parent", "other", "opinionated"],
        "Include (item)"
    )]
    #[case::excluded_item_beats_tags(
        Verdict::Exclude,
        &["parent", "other", "opinionated"],
        &["rule"],
        "Exclude (item)"
    )]
    fn nearest_directive_decides_and_exclusion_wins_across_branches(
        #[case] default: Verdict,
        #[case] include: &[&str],
        #[case] exclude: &[&str],
        #[case] expected: &str,
    ) {
        let tags = [CHILD, OTHER, OPINIONATED];
        assert_eq!(format_decision(&tags, default, include, exclude), expected);
    }

    #[test]
    fn each_explicit_tag_is_an_independent_reason() {
        let result = format_decision(
            &[CHILD, SIBLING, OPINIONATED],
            Verdict::Exclude,
            &["child"],
            &["parent"],
        );
        assert_eq!(result, "Exclude (parent > sibling)");
    }

    #[rstest]
    #[case::narrower_tag_on_one_path_wins(&["linux"], &["os"], "Include (linux > ebpf)")]
    #[case::incomparable_tags_exclusion_wins(&["linux"], &["kernel"], "Exclude (kernel > ebpf)")]
    #[case::shortest_chain_explains(&["os"], &[], "Include (os > linux > ebpf)")]
    fn diamond_specificity_follows_poset_order_not_path_counts(
        #[case] include: &[&str],
        #[case] exclude: &[&str],
        #[case] expected: &str,
    ) {
        assert_eq!(
            format_decision(&[EBPF, OPINIONATED], Verdict::Exclude, include, exclude),
            expected
        );
    }

    #[test]
    fn directive_naming_another_item_does_not_apply_and_tags_projection_is_exposed() {
        let vocabulary = Vocabulary::new([CHILD, OPINIONATED]).unwrap();
        let items = ["rule", "other-rule"].map(|name| Item::named(name, [CHILD, OPINIONATED]));
        let catalog = Catalog::new(vocabulary, items).unwrap();
        let [rule, other] = ["rule", "other-rule"].map(|name| catalog.item(name).unwrap());
        let selection = Selection::new(
            &catalog,
            Verdict::Include,
            [(Selector::Item(other), Verdict::Exclude)],
        )
        .unwrap();
        assert_eq!(
            selection.decide(rule),
            Decision {
                verdict: Verdict::Include,
                reason: Reason::Default
            }
        );
        assert_eq!(selection.tags().default_verdict(), Verdict::Include);
    }

    #[rstest]
    #[case::no_directives(Verdict::Include, &[], &[], &[])]
    #[case::implied_by_broader_selection(
        Verdict::Exclude,
        &["parent", "child"],
        &[],
        &[("child", Verdict::Include)]
    )]
    #[case::exception_in_either_direction(Verdict::Include, &["child"], &["parent"], &[])]
    #[case::implied_by_broader_exclusion(
        Verdict::Include,
        &[],
        &["parent", "child"],
        &[("child", Verdict::Exclude)]
    )]
    #[case::matches_default_verdict(
        Verdict::Include,
        &["other"],
        &[],
        &[("other", Verdict::Include)]
    )]
    #[case::item_included_matching_default(
        Verdict::Include,
        &["other-rule"],
        &[],
        &[("other-rule", Verdict::Include)]
    )]
    #[case::item_excluded_matching_default(
        Verdict::Exclude,
        &[],
        &["other-rule"],
        &[("other-rule", Verdict::Exclude)]
    )]
    #[case::item_exception(Verdict::Exclude, &["other-rule"], &[], &[])]
    #[case::items_before_tags_and_tag_id_order(
        Verdict::Include,
        &["other", "other-rule", "child"],
        &[],
        &[
            ("other-rule", Verdict::Include),
            ("child", Verdict::Include),
            ("other", Verdict::Include),
        ]
    )]
    #[case::mixed_include_and_exclude_ineffective_tags(
        Verdict::Include,
        &["other"],
        &["parent", "child"],
        &[("child", Verdict::Exclude), ("other", Verdict::Include)]
    )]
    fn ineffective_directives_change_no_verdict_without_cloning(
        #[case] default: Verdict,
        #[case] include: &[&str],
        #[case] exclude: &[&str],
        #[case] expected: &[(&str, Verdict)],
    ) {
        let vocabulary = Vocabulary::new([CHILD, SIBLING, OTHER, OPINIONATED]).unwrap();
        let items = [
            ("child-rule", CHILD),
            ("sibling-rule", SIBLING),
            ("other-rule", OTHER),
        ]
        .map(|(name, topic)| Item::named(name, [topic, OPINIONATED]));
        let catalog = Catalog::new(vocabulary, items).unwrap();
        let directives = [(include, Verdict::Include), (exclude, Verdict::Exclude)]
            .into_iter()
            .flat_map(|(labels, verdict)| {
                labels
                    .iter()
                    .map(|label| (catalog.resolve(label).unwrap(), verdict))
                    .collect::<Vec<_>>()
            });
        let selection = Selection::new(&catalog, default, directives).unwrap();
        let expected_directives: Vec<(Selector, Verdict)> = expected
            .iter()
            .map(|&(label, verdict)| (catalog.resolve(label).unwrap(), verdict))
            .collect();
        assert_eq!(selection.ineffective(), expected_directives);
    }

    #[rstest]
    #[case::narrower_tag_survives_broader_override("child-rule", true, true)]
    #[case::broader_tag_override_excludes_sibling("sibling-rule", true, false)]
    #[case::item_override_replaces_base_item("replaced-item", true, false)]
    #[case::untouched_base_item_preserved("kept-item", true, true)]
    #[case::newly_added_item_included("added-item", false, true)]
    #[case::newly_added_tag_included("added-tag-item", false, true)]
    #[case::unmatched_item_falls_back_to_default("fallback-item", false, false)]
    fn override_with_replaces_items_and_tags_while_preserving_base_and_precedence(
        #[case] rule_name: &str,
        #[case] expected_base: bool,
        #[case] expected_after_override: bool,
    ) {
        let vocabulary = Vocabulary::new([CHILD, SIBLING, OTHER, LINUX, OPINIONATED]).unwrap();
        let items = [
            ("child-rule", CHILD),
            ("sibling-rule", SIBLING),
            ("replaced-item", OTHER),
            ("kept-item", OTHER),
            ("added-item", OTHER),
            ("added-tag-item", LINUX),
            ("fallback-item", OTHER),
        ]
        .map(|(name, topic)| Item::named(name, [topic, OPINIONATED]));
        let catalog = Catalog::new(vocabulary, items).unwrap();
        let resolve = |label| catalog.resolve(label).unwrap();
        let item = catalog.item(rule_name).unwrap();

        let base = Selection::new(
            &catalog,
            Verdict::Exclude,
            [
                (resolve("parent"), Verdict::Include),
                (resolve("child"), Verdict::Include),
                (resolve("replaced-item"), Verdict::Include),
                (resolve("kept-item"), Verdict::Include),
            ],
        )
        .unwrap();
        let overridden = base
            .override_with([
                (resolve("parent"), Verdict::Exclude),
                (resolve("replaced-item"), Verdict::Exclude),
                (resolve("added-item"), Verdict::Include),
                (resolve("linux"), Verdict::Include),
            ])
            .unwrap();

        assert_eq!(base.matches(item), expected_base);
        assert_eq!(overridden.matches(item), expected_after_override);
        assert_eq!(overridden.tags().default_verdict(), Verdict::Exclude);
    }

    #[test]
    fn override_with_rejects_intra_layer_conflicts_in_selector_order() {
        let catalog = one_rule_catalog(&[CHILD, OTHER, OPINIONATED]);
        let [rule, child, other] =
            ["rule", "child", "other"].map(|label| catalog.resolve(label).unwrap());
        let base = Selection::new(&catalog, Verdict::Include, [(rule, Verdict::Exclude)]).unwrap();
        let conflicts = base
            .override_with([
                (other, Verdict::Include),
                (child, Verdict::Include),
                (rule, Verdict::Include),
                (other, Verdict::Exclude),
                (child, Verdict::Exclude),
                (rule, Verdict::Exclude),
            ])
            .unwrap_err();
        assert_eq!(
            conflicts,
            [
                Conflict { selector: rule },
                Conflict { selector: child },
                Conflict { selector: other },
            ]
        );
    }

    #[test]
    fn conflicts_order_items_before_tags() {
        let catalog = one_rule_catalog(&[CHILD, OTHER, OPINIONATED]);
        let [rule, child, other] =
            ["rule", "child", "other"].map(|label| catalog.resolve(label).unwrap());
        let conflicting = [
            (other, Verdict::Include),
            (child, Verdict::Include),
            (rule, Verdict::Include),
            (other, Verdict::Exclude),
            (child, Verdict::Exclude),
            (rule, Verdict::Exclude),
        ];
        assert_eq!(
            Selection::new(&catalog, Verdict::Include, conflicting).unwrap_err(),
            [
                Conflict { selector: rule },
                Conflict { selector: child },
                Conflict { selector: other },
            ]
        );
    }

    #[test]
    fn repeating_the_same_verdict_on_a_selector_is_allowed() {
        let catalog = one_rule_catalog(&[CHILD, OTHER, OPINIONATED]);
        let [rule, child] = ["rule", "child"].map(|label| catalog.resolve(label).unwrap());
        let repeated = [
            (rule, Verdict::Exclude),
            (rule, Verdict::Exclude),
            (child, Verdict::Include),
            (child, Verdict::Include),
        ];
        assert!(Selection::new(&catalog, Verdict::Include, repeated).is_ok());
    }

    #[test]
    fn selection_clones_when_catalog_payload_is_not_clone() {
        struct NonClone;
        let vocabulary = Vocabulary::new([CHILD, OPINIONATED]).unwrap();
        let catalog = Catalog::new(
            vocabulary,
            [Item::new("rule", [CHILD, OPINIONATED], NonClone)],
        )
        .unwrap();
        let rule = catalog.item("rule").unwrap();
        let selection = Selection::new(
            &catalog,
            Verdict::Include,
            [(Selector::Item(rule), Verdict::Exclude)],
        )
        .unwrap();
        assert_eq!(selection.clone().decide(rule), selection.decide(rule));
    }
}
