//! Catalogs of named items with validated [`TagSet`]s and a shared item/tag selector namespace.

use std::collections::{BTreeMap, BTreeSet};

use thiserror::Error;

use crate::tag::{Tag, TagGroup};
use crate::tag_set::{TagSet, TagSetError};
use crate::vocabulary::{TagId, TagLookupError, Vocabulary};

/// Compact handle for an item inside a [`Catalog`].
///
/// An `ItemId` is only valid with the [`Catalog`] that created it (and selections built from that
/// catalog). Passing it to a different catalog can panic or return the wrong item.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ItemId(usize);

/// An item to catalog: a unique name, its explicit tags, and its domain payload.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Item<T = ()> {
    /// The item's name, which selects it like a label selects a tag.
    pub name: &'static str,
    /// The item's most specific tags; [`TagSet`] adds the tags they imply.
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

/// What a selector string resolves to in a [`Catalog`]: one specific item, or every item carrying
/// a tag.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Selector {
    /// One item by name (overrides any tag directive on that item).
    Item(ItemId),
    /// Every item carrying this tag, explicitly or by implication.
    Tag(TagId),
}

/// Why a runtime label string (for example, from a config file or CLI flag) could not be resolved
/// to a [`Selector`] in a [`Catalog`].
///
/// In Rust code, [`Tag`] and [`TagGroup`] are already distinct types, so you cannot pass a group
/// where a tag or selector is expected at compile time. [`Self::Group`] exists only for
/// [`Catalog::resolve`], which parses untrusted user strings (such as `select = ["consensus"]` in
/// a config file) and needs to tell the user they typed a group name instead of a selector.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Error)]
pub enum LabelError {
    /// The string matches a [`TagGroup`] name (ignoring ASCII case and `-`/`_`/space differences),
    /// which is never a selector.
    #[error("`{}` is a group, not a selector", .0.name)]
    Group(&'static TagGroup),
    /// The string matches no item name, tag label, or tag alias.
    #[error("the label names no item, tag or alias")]
    Unknown,
}

/// Why a set of items is not a valid [`Catalog`].
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum CatalogError {
    /// An item's name is already the name of another item, or a tag label, alias or group
    /// name.
    #[error("`{name}` names more than one item, tag, alias or group")]
    NameCollision {
        /// The colliding name.
        name: &'static str,
    },
    /// An item's tags fail [`TagSet`] validation.
    #[error("`{item}`: {source}")]
    InvalidTag {
        /// The item's name.
        item: &'static str,
        /// The underlying tag-set violation.
        #[source]
        source: TagSetError,
    },
}

/// Named items and their validated [`TagSet`]s under a [`Vocabulary`].
///
/// Every item's tags form a valid [`TagSet`], and names are unique across items, tag labels,
/// aliases, and group names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Catalog<T = ()> {
    vocabulary: Vocabulary,
    entries: Vec<Entry>,
    values: Vec<T>,
    names: BTreeMap<&'static str, ItemId>,
}

impl<T> Catalog<T> {
    /// Builds the catalog of `items`, in order, tagged from `vocabulary`.
    ///
    /// # Errors
    ///
    /// Returns every [`CatalogError`], in item order.
    pub fn new(
        vocabulary: Vocabulary,
        items: impl IntoIterator<Item = Item<T>>,
    ) -> Result<Self, Vec<CatalogError>> {
        Self::new_with(vocabulary, items, |_, _, value| value)
    }

    /// Validates all `items` first, then builds each item's stored value `T` by calling
    /// `build(id, &tags, seed)` with its assigned [`ItemId`] and validated [`TagSet`].
    ///
    /// This lets your domain type `T` store its own [`ItemId`] directly instead of looking itself
    /// up by name later. If any item fails name-uniqueness or [`TagSet`] validation, `build` is
    /// never called.
    ///
    /// # Errors
    ///
    /// Returns every [`CatalogError`], in item order.
    pub fn new_with<S>(
        vocabulary: Vocabulary,
        items: impl IntoIterator<Item = Item<S>>,
        mut build: impl FnMut(ItemId, &TagSet, S) -> T,
    ) -> Result<Self, Vec<CatalogError>> {
        let mut errors = Vec::new();
        let mut names = BTreeMap::new();
        let mut reported_collisions = BTreeSet::new();
        let mut validated = Vec::new();

        for (index, item) in items.into_iter().enumerate() {
            let id = ItemId(index);
            let name_taken = names.insert(item.name, id).is_some()
                || vocabulary.id(item.name).is_some()
                || vocabulary.group(item.name).is_some();
            if name_taken && reported_collisions.insert(item.name) {
                errors.push(CatalogError::NameCollision { name: item.name });
            }
            match TagSet::new(&vocabulary, item.tags) {
                Ok(tags) => validated.push((item.name, tags, item.value)),
                Err(tag_errors) => {
                    errors.extend(
                        tag_errors
                            .into_iter()
                            .map(|source| CatalogError::InvalidTag {
                                item: item.name,
                                source,
                            }),
                    );
                }
            }
        }
        if !errors.is_empty() {
            return Err(errors);
        }

        let mut entries = Vec::with_capacity(validated.len());
        let mut values = Vec::with_capacity(validated.len());
        for (index, (name, tags, seed)) in validated.into_iter().enumerate() {
            let id = ItemId(index);
            values.push(build(id, &tags, seed));
            entries.push(Entry { name, tags });
        }
        Ok(Self {
            vocabulary,
            entries,
            values,
            names,
        })
    }

    /// The vocabulary the items are tagged from.
    #[must_use]
    pub const fn vocabulary(&self) -> &Vocabulary {
        &self.vocabulary
    }

    /// Every item, in catalog order.
    pub fn items(&self) -> impl ExactSizeIterator<Item = ItemId> + use<T> {
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

    /// The item's validated [`TagSet`].
    #[must_use]
    pub fn tags(&self, item: ItemId) -> &TagSet {
        &self.entries[item.0].tags
    }

    /// Whether `selector` selects `item`.
    #[must_use]
    pub fn selects(&self, selector: Selector, item: ItemId) -> bool {
        match selector {
            Selector::Item(selected) => selected == item,
            Selector::Tag(tag) => self.entries[item.0].tags.carries(tag),
        }
    }

    /// The tags no item in the catalog carries, in vocabulary order.
    pub fn unused_tags(&self) -> impl Iterator<Item = TagId> + '_ {
        self.vocabulary
            .tags()
            .filter(|&tag| !self.items().any(|item| self.tags(item).carries(tag)))
    }

    /// The selector `label` names: an item name, a tag label, or a tag alias.
    ///
    /// # Errors
    ///
    /// Returns [`LabelError::Group`] if `label` loosely names a group, and
    /// [`LabelError::Unknown`] otherwise.
    pub fn resolve(&self, label: &str) -> Result<Selector, LabelError> {
        if let Some(item) = self.item(label) {
            return Ok(Selector::Item(item));
        }
        match self.vocabulary.resolve(label) {
            Ok(tag) => Ok(Selector::Tag(tag)),
            Err(TagLookupError::Group(group)) => Err(LabelError::Group(group)),
            Err(TagLookupError::Unknown) => Err(LabelError::Unknown),
        }
    }

    /// Every label [`Self::resolve`] accepts: item names, tag labels, and aliases.
    pub fn labels(&self) -> impl Iterator<Item = &'static str> + '_ {
        self.names.keys().copied().chain(self.vocabulary.labels())
    }
}

impl<T> std::ops::Index<ItemId> for Catalog<T> {
    type Output = T;

    fn index(&self, item: ItemId) -> &Self::Output {
        &self.values[item.0]
    }
}

/// One cataloged item's name and validated tags.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Entry {
    name: &'static str,
    tags: TagSet,
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
    const CONSENSUS: TagGroup = TagGroup {
        name: "Consensus",
        cardinality: Cardinality::ExactlyOne,
    };
    const SCOPE: TagGroup = TagGroup {
        name: "Scope",
        cardinality: Cardinality::AtMostOne,
    };
    const VCS: Tag = Tag::new(&TOPIC, "vcs", "Version control.").aliases(&["version-control"]);
    const JJ: Tag = Tag::new(&TOPIC, "jj", "Jujutsu.").implies(&[VCS]);
    const TESTING: Tag = Tag::new(&TOPIC, "testing", "Testing.");
    const OPINIONATED: Tag = Tag::new(&CONSENSUS, "opinionated", "Opinionated.");
    const UNOPINIONATED: Tag = Tag::new(&CONSENSUS, "unopinionated", "Unopinionated.");
    const TESTS_ONLY: Tag = Tag::new(&SCOPE, "tests-only", "Tests only.");

    fn item(name: &'static str, tags: &[Tag]) -> Item {
        Item::named(name, tags)
    }

    fn build_catalog(items: impl IntoIterator<Item = Item>) -> Result<Catalog, Vec<CatalogError>> {
        let vocabulary =
            Vocabulary::new([JJ, TESTING, OPINIONATED, UNOPINIONATED, TESTS_ONLY]).unwrap();
        Catalog::new(vocabulary, items)
    }

    fn valid_catalog() -> Catalog {
        build_catalog([
            item("commit-check", &[JJ, UNOPINIONATED]),
            item("sleep-check", &[TESTING, OPINIONATED, TESTS_ONLY]),
        ])
        .unwrap()
    }

    #[test]
    fn items_and_values_keep_catalog_order_and_index_by_id() {
        let vocabulary = Vocabulary::new([JJ, TESTING, UNOPINIONATED]).unwrap();
        let catalog = Catalog::new(
            vocabulary,
            [
                Item::new("first", [JJ, UNOPINIONATED], 10_u32),
                Item::new("second", [TESTING, UNOPINIONATED], 20_u32),
            ],
        )
        .unwrap();
        let [first, second] = ["first", "second"].map(|name| catalog.item(name).unwrap());
        let names: Vec<_> = catalog.items().map(|id| catalog.name(id)).collect();

        assert_eq!(names, ["first", "second"]);
        assert_eq!(catalog.values(), [10, 20]);
        assert_eq!(*catalog.value(second), 20);
        assert_eq!(catalog[first], 10);
    }

    #[test]
    fn new_with_passes_assigned_id_and_tag_set_only_when_valid() {
        let vocabulary = Vocabulary::new([JJ, TESTING, UNOPINIONATED]).unwrap();
        let catalog = Catalog::new_with(
            vocabulary,
            [
                Item::new("first", [JJ, UNOPINIONATED], "a"),
                Item::new("second", [TESTING, UNOPINIONATED], "b"),
            ],
            |id, tags, seed| (id, tags.explicit().len(), seed),
        )
        .unwrap();
        let second = catalog.item("second").unwrap();
        assert_eq!(catalog[second], (second, 2, "b"));
    }

    #[test]
    fn new_with_never_invokes_builder_when_any_item_is_invalid() {
        let vocabulary = Vocabulary::new([JJ, UNOPINIONATED]).unwrap();
        let result: Result<Catalog<u32>, _> = Catalog::new_with(
            vocabulary,
            [
                Item::new("valid", [JJ, UNOPINIONATED], 1_u32),
                Item::new("invalid", [JJ], 2_u32),
            ],
            |_, _, _| panic!("builder must not run on invalid catalog"),
        );
        assert!(result.is_err());
    }

    #[test]
    fn tags_accessor_exposes_item_tag_set() {
        let catalog = valid_catalog();
        let commit = catalog.item("commit-check").unwrap();
        let vcs = catalog.vocabulary().id("vcs").unwrap();
        assert!(catalog.tags(commit).carries(vcs));
    }

    #[rstest]
    #[case::item_name("commit-check", true)]
    #[case::other_item("sleep-check", false)]
    #[case::implied_tag("vcs", true)]
    #[case::alias("version-control", true)]
    #[case::unmatched_tag("testing", false)]
    fn resolve_and_selects_match_items_and_tags(#[case] label: &str, #[case] expected: bool) {
        let catalog = valid_catalog();
        let commit = catalog.item("commit-check").unwrap();
        let selector = catalog.resolve(label).unwrap();
        assert_eq!(catalog.selects(selector, commit), expected);
    }

    #[rstest]
    #[case::group_name(
        "consensus",
        Err(LabelError::Group(&CONSENSUS)),
        "`Consensus` is a group, not a selector"
    )]
    #[case::unknown(
        "tesing",
        Err(LabelError::Unknown),
        "the label names no item, tag or alias"
    )]
    fn resolve_rejects_groups_and_unknown_labels(
        #[case] label: &str,
        #[case] expected: Result<Selector, LabelError>,
        #[case] message: &str,
    ) {
        let result = valid_catalog().resolve(label);
        assert_eq!(result, expected);
        assert_eq!(result.unwrap_err().to_string(), message);
    }

    #[test]
    fn unused_tags_and_labels_reflect_catalog_contents() {
        let catalog = build_catalog([item("commit-check", &[JJ, UNOPINIONATED])]).unwrap();
        let unused: Vec<_> = catalog
            .unused_tags()
            .map(|id| catalog.vocabulary().tag(id).label)
            .collect();
        let labels: Vec<_> = catalog.labels().collect();

        assert_eq!(unused, ["testing", "opinionated", "tests-only"]);
        assert_eq!(
            labels,
            [
                "commit-check",
                "jj",
                "opinionated",
                "testing",
                "tests-only",
                "unopinionated",
                "vcs",
                "version-control"
            ]
        );
    }

    #[rstest]
    #[case::unknown_tag(
        &[item("check", &[JJ, UNOPINIONATED, Tag { label: "other", ..JJ }])],
        &["`check`: `other` is not a tag of the vocabulary"]
    )]
    #[case::redundant_tag(
        &[item("check", &[VCS, JJ, UNOPINIONATED])],
        &["`check`: remove `vcs`, it is implied by `jj`"]
    )]
    #[case::name_collides_with_tag(
        &[item("vcs", &[JJ, UNOPINIONATED])],
        &["`vcs` names more than one item, tag, alias or group"]
    )]
    #[case::name_collides_with_alias(
        &[item("version-control", &[JJ, UNOPINIONATED])],
        &["`version-control` names more than one item, tag, alias or group"]
    )]
    #[case::name_collides_with_group(
        &[item("topic", &[JJ, UNOPINIONATED])],
        &["`topic` names more than one item, tag, alias or group"]
    )]
    #[case::three_duplicate_names_emit_one_collision_in_item_order(
        &[
            item("dup", &[JJ, UNOPINIONATED]),
            item("dup", &[TESTING, UNOPINIONATED]),
            item("dup", &[TESTING, OPINIONATED]),
        ],
        &["`dup` names more than one item, tag, alias or group"]
    )]
    #[case::errors_preserve_item_order(
        &[item("second", &[UNOPINIONATED]), item("first", &[JJ])],
        &[
            "`second`: carries no tag of group `Topic`",
            "`first`: carries no tag of group `Consensus`",
        ]
    )]
    fn invalid_catalogs_report_every_violation(#[case] items: &[Item], #[case] expected: &[&str]) {
        let errors = build_catalog(items.iter().cloned()).unwrap_err();
        let messages: Vec<String> = errors.iter().map(ToString::to_string).collect();
        assert_eq!(messages, expected);
    }
}
