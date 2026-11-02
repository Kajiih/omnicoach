//! Plans: specificity-ordered include/exclude directives and the decisions they make.

use std::cell::OnceCell;
use std::collections::{BTreeMap, BTreeSet};

use fixedbitset::FixedBitSet;

use crate::catalog::{Catalog, ItemId, Selector};
use crate::vocabulary::TagId;

/// What a directive, or a plan's default, decides for an item.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Verdict {
    /// Keep the item.
    Include,
    /// Drop the item.
    Exclude,
}

/// A selector given both verdicts, which [`Plan::new`] rejects.
///
/// It names the selector by id, so the caller formats it with the labels of its catalog.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Conflict {
    /// The conflicting selector.
    pub selector: Selector,
}

/// A default verdict and directives that override it, ordered by specificity.
///
/// [`Self::decide`] reads the directives that select an item as follows:
///
/// 1. A directive naming the item decides: the item is below every tag it carries.
/// 2. Otherwise, each explicit tag of the item is an independent reason. Among the directives
///    on that tag or the tags it implies, the most specific ones count: those whose tag no
///    other such directive's tag implies.
/// 3. Across the counting directives of all explicit tags, [`Verdict::Exclude`] wins.
/// 4. With no directive, the default decides.
///
/// On a tree this is "the nearest directive on each branch decides, and exclusion wins
/// across branches". On a polyhierarchy, a tag reached along several paths still counts
/// once per explicit tag, so a directive on a broad tag never resurfaces past a narrower
/// one through a second path.
#[derive(Debug)]
pub struct Plan<'a, T = ()> {
    catalog: &'a Catalog<T>,
    default: Verdict,
    items: BTreeMap<ItemId, Verdict>,
    tagged: FixedBitSet,
    excluded: FixedBitSet,
}

impl<T> Clone for Plan<'_, T> {
    fn clone(&self) -> Self {
        Self {
            catalog: self.catalog,
            default: self.default,
            items: self.items.clone(),
            tagged: self.tagged.clone(),
            excluded: self.excluded.clone(),
        }
    }
}

impl<'a, T> Plan<'a, T> {
    /// The plan over `catalog` deciding `default` unless one of `directives` applies.
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
        let tag_count = catalog.vocabulary().tags().len();
        let mut items = BTreeMap::new();
        let mut included = FixedBitSet::with_capacity(tag_count);
        let mut excluded = FixedBitSet::with_capacity(tag_count);
        let mut tagged = FixedBitSet::with_capacity(tag_count);
        let mut conflicting = BTreeSet::new();
        for (selector, verdict) in directives {
            match selector {
                Selector::Item(item) => {
                    if items
                        .insert(item, verdict)
                        .is_some_and(|other| other != verdict)
                    {
                        conflicting.insert(selector);
                    }
                }
                Selector::Tag(tag) => {
                    let index = tag.index();
                    let (same, opposite) = match verdict {
                        Verdict::Include => (&mut included, &excluded),
                        Verdict::Exclude => (&mut excluded, &included),
                    };
                    same.insert(index);
                    tagged.insert(index);
                    if opposite.contains(index) {
                        conflicting.insert(selector);
                    }
                }
            }
        }
        if conflicting.is_empty() {
            Ok(Self {
                catalog,
                default,
                items,
                tagged,
                excluded,
            })
        } else {
            Err(conflicting
                .into_iter()
                .map(|selector| Conflict { selector })
                .collect())
        }
    }

    /// The plan's verdict on `item`, and why.
    #[must_use]
    pub fn decide(&self, item: ItemId) -> Decision {
        if let Some(&verdict) = self.items.get(&item) {
            return Decision {
                verdict,
                reason: Reason::Item,
            };
        }
        let vocabulary = self.catalog.vocabulary();
        let mut first_include = None;
        for &source in self.catalog.explicit(item) {
            let closure = vocabulary.closure(source);
            for index in self.tagged.intersection(closure) {
                let tag = TagId::new(index);
                if intersects_both(&self.tagged, closure, vocabulary.strict_narrower(tag)) {
                    continue;
                }
                let reason = Reason::Tag { tag, source };
                if self.excluded.contains(index) {
                    return Decision {
                        verdict: Verdict::Exclude,
                        reason,
                    };
                }
                first_include.get_or_insert(Decision {
                    verdict: Verdict::Include,
                    reason,
                });
            }
        }
        first_include.unwrap_or(Decision {
            verdict: self.default,
            reason: Reason::Default,
        })
    }

    /// The directives that change no item's verdict: removing any one of them, with the
    /// others kept, leaves every verdict in the catalog as it is. Two directives may each be
    /// redundant only because of the other.
    #[must_use]
    pub fn ineffective(&self) -> Vec<Selector> {
        let verdicts = OnceCell::new();
        let selectors = self.items.keys().copied().map(Selector::Item).chain(
            self.tagged
                .ones()
                .map(|index| Selector::Tag(TagId::new(index))),
        );
        selectors
            .filter(|&selector| {
                let verdicts = verdicts.get_or_init(|| {
                    self.catalog
                        .items()
                        .map(|item| self.decide(item).verdict)
                        .collect::<Vec<_>>()
                });
                let mut without = self.clone();
                match selector {
                    Selector::Item(item) => {
                        without.items.remove(&item);
                    }
                    Selector::Tag(tag) => {
                        without.tagged.set(tag.index(), false);
                    }
                }
                self.catalog
                    .items()
                    .zip(verdicts)
                    .all(|(item, &verdict)| without.decide(item).verdict == verdict)
            })
            .collect()
    }
}

/// A plan's verdict on an item, and why.
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
    /// No directive selects the item: the plan's default.
    Default,
    /// The directive naming the item.
    Item,
    /// The directive on `tag`, which applies through the item's explicit tag `source`.
    /// [`crate::Vocabulary::chain`] lists the implications from `source` up to `tag`.
    Tag {
        /// The directive's tag.
        tag: TagId,
        /// The item's explicit tag that implies `tag`; `tag` itself when explicit.
        source: TagId,
    },
}

/// Whether any index belongs to `base`, `first` and `second`.
fn intersects_both(base: &FixedBitSet, first: &FixedBitSet, second: &FixedBitSet) -> bool {
    base.as_slice()
        .iter()
        .zip(first.as_slice())
        .zip(second.as_slice())
        .any(|((&word, &first), &second)| word & first & second != 0)
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
    const PRECISION: TagGroup = TagGroup {
        name: "Precision",
        cardinality: Cardinality::ExactlyOne,
    };
    const PARENT: Tag = Tag {
        label: "parent",
        group: &TOPIC,
        implies: &[],
        aliases: &[],
        description: "Parent.",
        scope_note: None,
    };
    const CHILD: Tag = Tag {
        label: "child",
        implies: &[PARENT],
        ..PARENT
    };
    const SIBLING: Tag = Tag {
        label: "sibling",
        implies: &[PARENT],
        ..PARENT
    };
    const OTHER: Tag = Tag {
        label: "other",
        ..PARENT
    };
    const HEURISTIC: Tag = Tag {
        label: "heuristic",
        group: &PRECISION,
        ..PARENT
    };
    const OS: Tag = Tag {
        label: "os",
        ..PARENT
    };
    const LINUX: Tag = Tag {
        label: "linux",
        implies: &[OS],
        ..PARENT
    };
    const KERNEL: Tag = Tag {
        label: "kernel",
        implies: &[OS],
        ..PARENT
    };
    const EBPF: Tag = Tag {
        label: "ebpf",
        implies: &[LINUX, KERNEL],
        ..PARENT
    };

    /// The catalog of one item named `rule` carrying `tags`.
    fn catalog(tags: &[Tag]) -> Catalog {
        let vocabulary = Vocabulary::new(tags.iter().copied()).unwrap();
        Catalog::new(vocabulary, [Item::named("rule", tags)]).unwrap()
    }

    /// `rule`'s decision under `default` and the directives, as `verdict (reason)`.
    fn decide(tags: &[Tag], default: Verdict, include: &[&str], exclude: &[&str]) -> String {
        let catalog = &catalog(tags);
        let directives = [(include, Verdict::Include), (exclude, Verdict::Exclude)]
            .into_iter()
            .flat_map(|(labels, verdict)| {
                labels
                    .iter()
                    .map(move |label| (catalog.resolve(label).unwrap(), verdict))
            });
        let plan = Plan::new(catalog, default, directives).unwrap();
        let decision = plan.decide(catalog.item("rule").unwrap());
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

    /// `rule` on the branches `parent > child`, `other` and `heuristic`.
    #[rstest]
    #[case::default_include(Verdict::Include, &[], &[], "Include (default)")]
    #[case::default_exclude(Verdict::Exclude, &[], &[], "Exclude (default)")]
    #[case::excluded_tag(Verdict::Include, &[], &["other"], "Exclude (other)")]
    #[case::ancestor_included(Verdict::Exclude, &["parent"], &[], "Include (parent > child)")]
    #[case::nearer_include_wins(Verdict::Exclude, &["child"], &["parent"], "Include (child)")]
    #[case::nearer_exclude_wins(Verdict::Exclude, &["parent"], &["child"], "Exclude (child)")]
    #[case::exclude_wins_across_topics(Verdict::Exclude, &["child"], &["other"], "Exclude (other)")]
    #[case::exclude_wins_across_groups(
        Verdict::Exclude,
        &["heuristic"],
        &["parent"],
        "Exclude (parent > child)"
    )]
    #[case::included_item_beats_tags(
        Verdict::Exclude,
        &["rule"],
        &["parent", "other", "heuristic"],
        "Include (item)"
    )]
    #[case::excluded_item_beats_tags(
        Verdict::Exclude,
        &["parent", "other", "heuristic"],
        &["rule"],
        "Exclude (item)"
    )]
    fn on_a_tree_the_nearest_directive_decides_and_exclusion_wins_across_branches(
        #[case] default: Verdict,
        #[case] include: &[&str],
        #[case] exclude: &[&str],
        #[case] expected: &str,
    ) {
        let tags = [CHILD, OTHER, HEURISTIC];
        assert_eq!(decide(&tags, default, include, exclude), expected);
    }

    #[test]
    fn each_explicit_tag_is_an_independent_reason() {
        let decision = decide(&[CHILD, SIBLING], Verdict::Exclude, &["child"], &["parent"]);
        assert_eq!(decision, "Exclude (parent > sibling)");
    }

    #[rstest]
    #[case::narrower_tag_on_one_path_wins(&["linux"], &["os"], "Include (linux > ebpf)")]
    #[case::incomparable_tags_exclusion_wins(&["linux"], &["kernel"], "Exclude (kernel > ebpf)")]
    #[case::shortest_chain_explains(&["os"], &[], "Include (os > linux > ebpf)")]
    fn on_a_diamond_specificity_follows_the_order_not_the_paths(
        #[case] include: &[&str],
        #[case] exclude: &[&str],
        #[case] expected: &str,
    ) {
        assert_eq!(
            decide(&[EBPF], Verdict::Exclude, include, exclude),
            expected
        );
    }

    #[test]
    fn a_directive_naming_another_item_does_not_apply() {
        let vocabulary = Vocabulary::new([CHILD]).unwrap();
        let items = ["rule", "other-rule"].map(|name| Item::named(name, [CHILD]));
        let catalog = Catalog::new(vocabulary, items).unwrap();
        let [rule, other] = ["rule", "other-rule"].map(|name| catalog.item(name).unwrap());
        let plan = Plan::new(
            &catalog,
            Verdict::Include,
            [(Selector::Item(other), Verdict::Exclude)],
        )
        .unwrap();
        assert_eq!(
            plan.decide(rule),
            Decision {
                verdict: Verdict::Include,
                reason: Reason::Default
            }
        );
    }

    /// Items `child-rule`, `sibling-rule` and `other-rule` under `parent > child`,
    /// `parent > sibling` and `other`.
    #[rstest]
    #[case::no_directives(Verdict::Include, &[], &[], &[])]
    #[case::implied_by_a_broader_selection(Verdict::Exclude, &["parent", "child"], &[], &["child"])]
    #[case::an_exception_in_either_direction(Verdict::Include, &["child"], &["parent"], &[])]
    #[case::implied_by_a_broader_exclusion(Verdict::Include, &[], &["parent", "child"], &["child"])]
    #[case::what_the_default_already_decides(Verdict::Include, &["other"], &[], &["other"])]
    #[case::item_matching_default(Verdict::Include, &["other-rule"], &[], &["other-rule"])]
    #[case::item_exception(Verdict::Exclude, &["other-rule"], &[], &[])]
    fn ineffective_directives_change_no_verdict(
        #[case] default: Verdict,
        #[case] include: &[&str],
        #[case] exclude: &[&str],
        #[case] expected: &[&str],
    ) {
        let vocabulary = Vocabulary::new([CHILD, SIBLING, OTHER]).unwrap();
        let items = [
            ("child-rule", CHILD),
            ("sibling-rule", SIBLING),
            ("other-rule", OTHER),
        ]
        .map(|(name, tag)| Item::named(name, [tag]));
        let catalog = &Catalog::new(vocabulary, items).unwrap();
        let directives = [(include, Verdict::Include), (exclude, Verdict::Exclude)]
            .into_iter()
            .flat_map(|(labels, verdict)| {
                labels
                    .iter()
                    .map(move |label| (catalog.resolve(label).unwrap(), verdict))
            });
        let plan = Plan::new(catalog, default, directives).unwrap();
        let expected: Vec<Selector> = expected
            .iter()
            .map(|label| catalog.resolve(label).unwrap())
            .collect();
        assert_eq!(plan.ineffective(), expected);
    }

    #[test]
    fn a_selector_with_both_verdicts_conflicts() {
        let catalog = catalog(&[CHILD, OTHER]);
        let [rule, child, other] =
            ["rule", "child", "other"].map(|label| catalog.resolve(label).unwrap());
        let directives = [
            (other, Verdict::Include),
            (child, Verdict::Include),
            (rule, Verdict::Include),
            (other, Verdict::Exclude),
            (child, Verdict::Exclude),
            (rule, Verdict::Exclude),
        ];
        assert_eq!(
            Plan::new(&catalog, Verdict::Include, directives).unwrap_err(),
            [
                Conflict { selector: rule },
                Conflict { selector: child },
                Conflict { selector: other },
            ]
        );
    }

    #[test]
    fn repeating_an_item_directive_is_not_a_conflict() {
        let catalog = catalog(&[CHILD]);
        let rule = catalog.resolve("rule").unwrap();
        let directives = [(rule, Verdict::Exclude), (rule, Verdict::Exclude)];
        assert!(Plan::new(&catalog, Verdict::Include, directives).is_ok());
    }

    #[test]
    fn repeating_a_directive_is_not_a_conflict() {
        let catalog = catalog(&[CHILD]);
        let child = catalog.resolve("child").unwrap();
        let directives = [(child, Verdict::Exclude), (child, Verdict::Exclude)];
        assert!(Plan::new(&catalog, Verdict::Include, directives).is_ok());
    }

    #[test]
    fn plans_clone_even_when_catalog_values_do_not() {
        struct NonClone;
        let vocabulary = Vocabulary::new([CHILD]).unwrap();
        let catalog = Catalog::new(vocabulary, [Item::new("rule", [CHILD], NonClone)]).unwrap();
        let rule = catalog.item("rule").unwrap();
        let plan = Plan::new(
            &catalog,
            Verdict::Include,
            [(Selector::Item(rule), Verdict::Exclude)],
        )
        .unwrap();
        assert_eq!(plan.clone().decide(rule), plan.decide(rule));
    }

    #[rstest]
    #[case::shared_in_later_block(&[70], &[70, 90], true)]
    #[case::pairwise_without_common_triple(&[1], &[70], false)]
    #[case::disjoint(&[], &[1, 70], false)]
    fn intersects_both_requires_a_common_index(
        #[case] first: &[usize],
        #[case] second: &[usize],
        #[case] expected: bool,
    ) {
        let bitset = |indices: &[usize]| {
            let mut set = FixedBitSet::with_capacity(130);
            for &index in indices {
                set.insert(index);
            }
            set
        };
        assert_eq!(
            intersects_both(&bitset(&[1, 70]), &bitset(first), &bitset(second)),
            expected
        );
    }
}
