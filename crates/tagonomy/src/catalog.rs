//! Catalogs: named items carrying validated tags, and the selectors that name them.

use std::collections::BTreeMap;

use fixedbitset::FixedBitSet;
use thiserror::Error;

use crate::tag::{Tag, TagGroup};
use crate::vocabulary::{TagId, Vocabulary};

/// An item's position in a [`Catalog`].
///
/// An id is only meaningful for the catalog that issued it, and for plans built on that
/// catalog: other catalogs panic or answer wrongly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ItemId(usize);

/// An item to catalog: a unique name, its explicit tags, and its domain payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item<T = ()> {
    /// The item's name, which selects it like a label selects a tag.
    pub name: &'static str,
    /// The item's most specific tags; the catalog adds the tags they imply.
    pub tags: Vec<Tag>,
    /// Domain data carried alongside the item.
    pub value: T,
}

impl Item<()> {
    /// An item carrying no domain payload, named `name` with explicit `tags`.
    #[must_use]
    pub fn named(name: &'static str, tags: impl Into<Vec<Tag>>) -> Self {
        Self {
            name,
            tags: tags.into(),
            value: (),
        }
    }
}

impl<T> Item<T> {
    /// An item named `name` with explicit `tags` and domain payload `value`.
    #[must_use]
    pub fn new(name: &'static str, tags: impl Into<Vec<Tag>>, value: T) -> Self {
        Self {
            name,
            tags: tags.into(),
            value,
        }
    }
}

/// What a label names: one item, or every item carrying a tag.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Selector {
    /// The item, below every tag it carries.
    Item(ItemId),
    /// Every item carrying the tag, explicitly or by implication.
    Tag(TagId),
}

/// Why a label is not a [`Selector`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum LabelError {
    /// The label names a group, which is never a selector.
    #[error("`{}` is a group, not a selector", .0.name)]
    Group(&'static TagGroup),
    /// The label names nothing.
    #[error("the label names no item, tag or alias")]
    Unknown,
}

/// Why a set of items is not a valid [`Catalog`].
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum CatalogError {
    /// The item carries a tag the vocabulary does not define.
    #[error("`{item}`: `{tag}` is not a tag of the vocabulary")]
    UnknownTag {
        /// The item.
        item: &'static str,
        /// The tag's label.
        tag: &'static str,
    },
    /// The item carries a tag whose label the vocabulary defines differently.
    #[error("`{item}`: `{tag}` differs from the vocabulary's definition")]
    RedefinedTag {
        /// The item.
        item: &'static str,
        /// The tag's label.
        tag: &'static str,
    },
    /// The item lists a tag twice.
    #[error("`{item}`: `{tag}` is listed twice")]
    RepeatedTag {
        /// The item.
        item: &'static str,
        /// The tag's label.
        tag: &'static str,
    },
    /// The item lists a tag that another of its tags implies.
    #[error("`{item}`: remove `{tag}`, it is implied by `{implied_by}`")]
    RedundantTag {
        /// The item.
        item: &'static str,
        /// The implied tag's label.
        tag: &'static str,
        /// The label of the item's tag that implies it.
        implied_by: &'static str,
    },
    /// The item carries no tag of a group that requires one.
    #[error("`{item}`: carries no tag of group `{group}`")]
    MissingGroup {
        /// The item.
        item: &'static str,
        /// The group's name.
        group: &'static str,
    },
    /// The item carries two tags of an exclusive group that do not imply each other.
    #[error(
        "`{item}`: carries both `{}` and `{}`, which group `{group}` allows only one of",
        conflicting[0],
        conflicting[1]
    )]
    ExclusiveGroup {
        /// The item.
        item: &'static str,
        /// The group's name.
        group: &'static str,
        /// The two tags' labels.
        conflicting: [&'static str; 2],
    },
    /// The item's name is already the name of another item, or a tag label, alias or group
    /// name.
    #[error("`{name}` names more than one item, tag, alias or group")]
    NameCollision {
        /// The name.
        name: &'static str,
    },
}

/// Named items and their tags, validated against a [`Vocabulary`].
///
/// Every tag is defined, listed once and not implied by another of the item's tags, every
/// group's cardinality holds, and names are unique across items, labels, aliases and group
/// names.
#[derive(Debug)]
pub struct Catalog<T = ()> {
    vocabulary: Vocabulary,
    entries: Vec<Entry>,
    values: Vec<T>,
    names: BTreeMap<&'static str, ItemId>,
}

impl<T> Catalog<T> {
    /// The catalog of `items`, in order, tagged from `vocabulary`.
    ///
    /// # Errors
    ///
    /// Returns every [`CatalogError`], in item order.
    pub fn new(
        vocabulary: Vocabulary,
        items: impl IntoIterator<Item = Item<T>>,
    ) -> Result<Self, Vec<CatalogError>> {
        let mut errors = Vec::new();
        let mut names = BTreeMap::new();
        let mut entries = Vec::new();
        let mut values = Vec::new();
        for item in items {
            let taken = names.insert(item.name, ItemId(entries.len())).is_some()
                || vocabulary.id(item.name).is_some()
                || vocabulary.group(item.name).is_some();
            if taken {
                errors.push(CatalogError::NameCollision { name: item.name });
            }
            entries.push(Entry::new(&vocabulary, item.name, &item.tags, &mut errors));
            values.push(item.value);
        }
        if errors.is_empty() {
            Ok(Self {
                vocabulary,
                entries,
                values,
                names,
            })
        } else {
            Err(errors)
        }
    }

    /// The vocabulary the items are tagged from.
    #[must_use]
    pub const fn vocabulary(&self) -> &Vocabulary {
        &self.vocabulary
    }

    /// Every item, in catalog order.
    pub fn items(&self) -> impl Iterator<Item = ItemId> {
        (0..self.entries.len()).map(ItemId)
    }

    /// Every item's domain payload, in catalog order.
    #[must_use]
    pub fn values(&self) -> &[T] {
        &self.values
    }

    /// The item's domain payload.
    #[must_use]
    pub fn value(&self, item: ItemId) -> &T {
        &self.values[item.0]
    }

    /// The item's name.
    #[must_use]
    pub fn name(&self, item: ItemId) -> &'static str {
        self.entries[item.0].name
    }

    /// The item named `name`.
    #[must_use]
    pub fn item(&self, name: &str) -> Option<ItemId> {
        self.names.get(name).copied()
    }

    /// The item's explicit tags, in the order given.
    #[must_use]
    pub fn explicit(&self, item: ItemId) -> &[TagId] {
        &self.entries[item.0].explicit
    }

    /// Whether the item carries `tag`, explicitly or by implication.
    #[must_use]
    pub fn carries(&self, item: ItemId, tag: TagId) -> bool {
        self.entries[item.0].effective.contains(tag.index())
    }

    /// Whether `selector` selects the item.
    #[must_use]
    pub fn selects(&self, selector: Selector, item: ItemId) -> bool {
        match selector {
            Selector::Item(selected) => selected == item,
            Selector::Tag(tag) => self.carries(item, tag),
        }
    }

    /// The tags no item carries, in vocabulary order.
    pub fn unused_tags(&self) -> impl Iterator<Item = TagId> {
        self.vocabulary
            .tags()
            .filter(|&tag| !self.items().any(|item| self.carries(item, tag)))
    }

    /// The selector `label` names: an item name, a tag label or a tag alias.
    ///
    /// # Errors
    ///
    /// Returns [`LabelError::Group`] if `label` loosely names a group, and
    /// [`LabelError::Unknown`] otherwise.
    pub fn resolve(&self, label: &str) -> Result<Selector, LabelError> {
        if let Some(item) = self.item(label) {
            return Ok(Selector::Item(item));
        }
        if let Some(tag) = self.vocabulary.id(label) {
            return Ok(Selector::Tag(tag));
        }
        Err(self
            .vocabulary
            .group(label)
            .map_or(LabelError::Unknown, LabelError::Group))
    }

    /// Every label [`Self::resolve`] accepts: item names, tag labels and aliases.
    pub fn labels(&self) -> impl Iterator<Item = &'static str> {
        self.names.keys().copied().chain(self.vocabulary.labels())
    }
}

impl<T> std::ops::Index<ItemId> for Catalog<T> {
    type Output = T;

    fn index(&self, item: ItemId) -> &Self::Output {
        &self.values[item.0]
    }
}

/// One cataloged item.
#[derive(Debug)]
struct Entry {
    name: &'static str,
    explicit: Vec<TagId>,
    /// The explicit tags and every tag they imply.
    effective: FixedBitSet,
}

impl Entry {
    /// The entry of `name` carrying `tags`, pushing its tag and cardinality violations to
    /// `errors`.
    fn new(
        vocabulary: &Vocabulary,
        name: &'static str,
        tags: &[Tag],
        errors: &mut Vec<CatalogError>,
    ) -> Self {
        let mut explicit = Vec::new();
        for tag in tags {
            match vocabulary.id(tag.label) {
                None => errors.push(CatalogError::UnknownTag {
                    item: name,
                    tag: tag.label,
                }),
                Some(id) if vocabulary.tag(id) != tag => {
                    errors.push(CatalogError::RedefinedTag {
                        item: name,
                        tag: tag.label,
                    });
                }
                Some(id) if explicit.contains(&id) => errors.push(CatalogError::RepeatedTag {
                    item: name,
                    tag: tag.label,
                }),
                Some(id) => explicit.push(id),
            }
        }
        for &tag in &explicit {
            if let Some(&narrower) = explicit
                .iter()
                .find(|&&other| other != tag && vocabulary.implies(other, tag))
            {
                errors.push(CatalogError::RedundantTag {
                    item: name,
                    tag: vocabulary.tag(tag).label,
                    implied_by: vocabulary.tag(narrower).label,
                });
            }
        }
        let mut effective = FixedBitSet::with_capacity(vocabulary.tags().len());
        for &tag in &explicit {
            effective.union_with(vocabulary.closure(tag));
        }
        for group in vocabulary.groups() {
            let carried = || {
                effective
                    .ones()
                    .any(|index| vocabulary.tag(TagId::new(index)).belongs_to(group))
            };
            if group.cardinality.is_required() && !carried() {
                errors.push(CatalogError::MissingGroup {
                    item: name,
                    group: group.name,
                });
            }
            if let Some([first, second]) = vocabulary.exclusive_conflict(&effective, group) {
                errors.push(CatalogError::ExclusiveGroup {
                    item: name,
                    group: group.name,
                    conflicting: [vocabulary.tag(first).label, vocabulary.tag(second).label],
                });
            }
        }
        Self {
            name,
            explicit,
            effective,
        }
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;
    use crate::tag::Cardinality;

    const TOPIC: TagGroup = TagGroup {
        name: "Topic",
        cardinality: Cardinality::AtLeastOne,
    };
    const PRECISION: TagGroup = TagGroup {
        name: "Precision",
        cardinality: Cardinality::ExactlyOne,
    };
    const VCS: Tag = Tag {
        label: "vcs",
        group: &TOPIC,
        implies: &[],
        aliases: &["version-control"],
        description: "Version control.",
        scope_note: None,
    };
    const JJ: Tag = Tag {
        label: "jj",
        implies: &[VCS],
        aliases: &[],
        ..VCS
    };
    const TESTING: Tag = Tag {
        label: "testing",
        aliases: &[],
        ..VCS
    };
    const EXACT: Tag = Tag {
        label: "exact",
        group: &PRECISION,
        aliases: &[],
        ..VCS
    };
    const HEURISTIC: Tag = Tag {
        label: "heuristic",
        ..EXACT
    };
    const SCOPE: TagGroup = TagGroup {
        name: "Scope",
        cardinality: Cardinality::AtMostOne,
    };
    const TESTS_ONLY: Tag = Tag {
        label: "tests-only",
        group: &SCOPE,
        ..EXACT
    };

    fn item(name: &'static str, tags: &[Tag]) -> Item {
        Item::named(name, tags)
    }

    fn catalog(items: impl IntoIterator<Item = Item>) -> Result<Catalog, Vec<CatalogError>> {
        let vocabulary = Vocabulary::new([JJ, TESTING, EXACT, HEURISTIC, TESTS_ONLY]).unwrap();
        Catalog::new(vocabulary, items)
    }

    fn valid() -> Catalog {
        catalog([
            item("commit-check", &[JJ, EXACT]),
            item("sleep-check", &[TESTING, HEURISTIC, TESTS_ONLY]),
        ])
        .unwrap()
    }

    fn tag(catalog: &Catalog, label: &str) -> TagId {
        catalog.vocabulary().id(label).unwrap()
    }

    #[rstest]
    #[case::explicit("commit-check", "jj", true)]
    #[case::implied("commit-check", "vcs", true)]
    #[case::other_branch("commit-check", "testing", false)]
    #[case::other_group("sleep-check", "exact", false)]
    fn an_item_carries_its_tags_and_their_implications(
        #[case] name: &str,
        #[case] label: &str,
        #[case] expected: bool,
    ) {
        let catalog = valid();
        let item = catalog.item(name).unwrap();
        assert_eq!(catalog.carries(item, tag(&catalog, label)), expected);
    }

    #[test]
    fn explicit_tags_keep_their_order() {
        let catalog = valid();
        let item = catalog.item("sleep-check").unwrap();
        let labels: Vec<_> = catalog
            .explicit(item)
            .iter()
            .map(|&id| catalog.vocabulary().tag(id).label)
            .collect();
        assert_eq!(labels, ["testing", "heuristic", "tests-only"]);
    }

    #[test]
    fn items_are_named_in_catalog_order() {
        let catalog = valid();
        let names: Vec<_> = catalog.items().map(|item| catalog.name(item)).collect();
        assert_eq!(names, ["commit-check", "sleep-check"]);
    }

    #[test]
    fn typed_values_keep_catalog_order_and_index_by_id() {
        let vocabulary = Vocabulary::new([JJ, TESTING, EXACT]).unwrap();
        let catalog = Catalog::new(
            vocabulary,
            [
                Item::new("first", [JJ, EXACT], 10_u32),
                Item::new("second", [TESTING, EXACT], 20_u32),
            ],
        )
        .unwrap();
        let [first, second] = ["first", "second"].map(|name| catalog.item(name).unwrap());
        assert_eq!(catalog.values(), [10, 20]);
        assert_eq!(*catalog.value(second), 20);
        assert_eq!(catalog[first], 10);
    }

    #[test]
    fn an_item_name_resolves_to_the_item() {
        let catalog = valid();
        let item = catalog.item("commit-check").unwrap();
        assert_eq!(catalog.resolve("commit-check"), Ok(Selector::Item(item)));
    }

    #[rstest]
    #[case::label("vcs")]
    #[case::alias("version-control")]
    fn a_tag_label_or_alias_resolves_to_the_tag(#[case] label: &str) {
        let catalog = valid();
        assert_eq!(
            catalog.resolve(label),
            Ok(Selector::Tag(tag(&catalog, "vcs")))
        );
    }

    #[rstest]
    #[case::group_name("precision", LabelError::Group(&PRECISION))]
    #[case::unknown("tesing", LabelError::Unknown)]
    fn other_labels_are_not_selectors(#[case] label: &str, #[case] expected: LabelError) {
        assert_eq!(valid().resolve(label), Err(expected));
    }

    /// Whether the selector named `label` selects `commit-check`.
    #[rstest]
    #[case::the_item("commit-check", true)]
    #[case::another_item("sleep-check", false)]
    #[case::an_implied_tag("vcs", true)]
    #[case::a_tag_it_lacks("testing", false)]
    fn a_selector_selects_its_item_or_its_tag_carriers(
        #[case] label: &str,
        #[case] expected: bool,
    ) {
        let catalog = valid();
        let item = catalog.item("commit-check").unwrap();
        assert_eq!(
            catalog.selects(catalog.resolve(label).unwrap(), item),
            expected
        );
    }

    #[rstest]
    #[case::group(LabelError::Group(&PRECISION), "`Precision` is a group, not a selector")]
    #[case::unknown(LabelError::Unknown, "the label names no item, tag or alias")]
    fn label_errors_explain_themselves(#[case] error: LabelError, #[case] expected: &str) {
        assert_eq!(error.to_string(), expected);
    }

    #[test]
    fn unused_tags_are_carried_by_no_item() {
        let catalog = catalog([item("commit-check", &[JJ, EXACT])]).unwrap();
        let unused: Vec<_> = catalog
            .unused_tags()
            .map(|id| catalog.vocabulary().tag(id).label)
            .collect();
        assert_eq!(unused, ["testing", "heuristic", "tests-only"]);
    }

    #[test]
    fn labels_cover_items_tags_and_aliases() {
        let labels: Vec<_> = valid().labels().collect();
        assert_eq!(
            labels,
            [
                "commit-check",
                "sleep-check",
                "exact",
                "heuristic",
                "jj",
                "testing",
                "tests-only",
                "vcs",
                "version-control"
            ]
        );
    }

    #[rstest]
    #[case::unknown_tag(
        item("check", &[JJ, EXACT, Tag { label: "other", ..JJ }]),
        &["`check`: `other` is not a tag of the vocabulary"]
    )]
    #[case::redefined_tag(
        item("check", &[Tag { description: "Other.", ..JJ }, EXACT]),
        &[
            "`check`: `jj` differs from the vocabulary's definition",
            "`check`: carries no tag of group `Topic`"
        ]
    )]
    #[case::repeated_tag(
        item("check", &[JJ, EXACT, JJ]),
        &["`check`: `jj` is listed twice"]
    )]
    #[case::redundant_tag(
        item("check", &[VCS, JJ, EXACT]),
        &["`check`: remove `vcs`, it is implied by `jj`"]
    )]
    #[case::missing_required(
        item("check", &[EXACT]),
        &["`check`: carries no tag of group `Topic`"]
    )]
    #[case::missing_exactly_one(
        item("check", &[JJ]),
        &["`check`: carries no tag of group `Precision`"]
    )]
    #[case::exclusive_pair(
        item("check", &[JJ, EXACT, HEURISTIC]),
        &[
            "`check`: carries both `exact` and `heuristic`, which group `Precision` allows only \
             one of"
        ]
    )]
    #[case::name_is_a_label(
        item("vcs", &[JJ, EXACT]),
        &["`vcs` names more than one item, tag, alias or group"]
    )]
    #[case::name_is_a_group(
        item("Topic", &[JJ, EXACT]),
        &["`Topic` names more than one item, tag, alias or group"]
    )]
    fn invalid_items_report_every_violation(#[case] invalid: Item, #[case] expected: &[&str]) {
        let errors = catalog([item("fine", &[TESTING, EXACT]), invalid]).unwrap_err();
        let messages: Vec<String> = errors.iter().map(ToString::to_string).collect();
        assert_eq!(messages, expected);
    }

    #[test]
    fn errors_are_reported_in_item_order() {
        let errors = catalog([item("second", &[EXACT]), item("first", &[JJ])]).unwrap_err();
        let messages: Vec<String> = errors.iter().map(ToString::to_string).collect();
        assert_eq!(
            messages,
            [
                "`second`: carries no tag of group `Topic`",
                "`first`: carries no tag of group `Precision`"
            ]
        );
    }

    #[test]
    fn two_items_with_one_name_collide() {
        let errors = catalog([
            item("check", &[JJ, EXACT]),
            item("check", &[TESTING, EXACT]),
        ])
        .unwrap_err();
        assert_eq!(errors, [CatalogError::NameCollision { name: "check" }]);
    }
}
