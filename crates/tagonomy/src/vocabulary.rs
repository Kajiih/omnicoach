//! Validated tag vocabularies: implied-tag lookup, alias resolution, and conflict checks.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use fixedbitset::FixedBitSet;
use thiserror::Error;

use crate::tag::{Tag, TagGroup};

/// Compact handle for a tag inside a [`Vocabulary`].
///
/// A `TagId` is only valid with the [`Vocabulary`] that issued it (and tag sets, catalogs, and
/// selections built from that vocabulary). Passing it to a different vocabulary can panic or
/// return the wrong tag.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TagId(usize);

/// Why a set of tags is not a valid [`Vocabulary`].
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum VocabularyError {
    /// Two tags, aliases, or groups share the same name.
    ///
    /// Group names are compared loosely (ignoring ASCII case and treating spaces, `_`, and `-` as
    /// equal, so `"File scope"` collides with `"file-scope"`).
    #[error("`{label}` names more than one tag, alias or group")]
    DuplicateLabel {
        /// The colliding label or group name.
        label: &'static str,
    },
    /// A tag implies two conflicting tags from an exclusive group (for example, a `portable` tag
    /// implying both `linux` and `windows` in an `AtMostOne` `OS` group), so no valid item could
    /// ever carry it.
    #[error(
        "`{tag}` implies both `{}` and `{}`, which group `{group}` allows only one of",
        conflicting[0],
        conflicting[1]
    )]
    Unsatisfiable {
        /// The conflicting tag's canonical label.
        tag: &'static str,
        /// The exclusive group of the two conflicting tags.
        group: &'static str,
        /// Two tags from `group` implied by `tag` (possibly including `tag` itself) where neither
        /// implies the other.
        conflicting: [&'static str; 2],
    },
}

/// Why a runtime label string (for example, from a CLI flag or config file) did not resolve to a
/// [`TagId`] in a [`Vocabulary`].
///
/// In Rust code, [`Tag`] and [`TagGroup`] are distinct types, so passing a group where a tag is
/// expected is already a compile error. [`Self::Group`] exists so string parsers like
/// [`Vocabulary::resolve`] can tell users they typed a group name instead of a tag name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Error)]
pub enum TagLookupError {
    /// The string matches a [`TagGroup`] name (for example, `"speed"` for `Speed`), not a tag.
    #[error("`{}` is a group, not a tag", .0.name)]
    Group(&'static TagGroup),
    /// The string matches no tag label or alias.
    #[error("the label names no tag or alias")]
    Unknown,
}

/// A validated set of tags with precomputed parent-tag implications and label lookups.
///
/// Building a `Vocabulary` automatically pulls in every broader tag implied by the input tags,
/// checks that all tag labels, aliases, and group names are unique, and verifies that no tag
/// implies two conflicting tags from an exclusive group.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Vocabulary {
    tags: Vec<Tag>,
    parents: Vec<Vec<TagId>>,
    /// Each tag's reflexive upward closure.
    closures: Vec<FixedBitSet>,
    /// Canonical labels and aliases.
    labels: BTreeMap<&'static str, TagId>,
    groups: Vec<&'static TagGroup>,
}

impl Vocabulary {
    /// Builds a vocabulary from `tags` plus any broader tags they imply.
    ///
    /// Directly passed `tags` come first (in input order), followed by any implied tags that were
    /// not listed directly (in the order first reached).
    ///
    /// # Errors
    ///
    /// Returns every [`VocabularyError`] in a deterministic order. Duplicate labels are reported
    /// first on their own, since implication checks require unique labels.
    pub fn new(tags: impl IntoIterator<Item = Tag>) -> Result<Self, Vec<VocabularyError>> {
        let mut interner = Interner::default();
        let given: Vec<TagId> = tags
            .into_iter()
            .filter_map(|tag| interner.add(tag))
            .collect();
        for id in given {
            interner.link(id);
        }
        let Interner {
            tags,
            parents,
            mut labels,
            mut duplicates,
        } = interner;
        for (index, tag) in tags.iter().enumerate() {
            for &alias in tag.aliases {
                if labels.insert(alias, TagId(index)).is_some() {
                    duplicates.insert(alias);
                }
            }
        }
        let mut groups: Vec<&'static TagGroup> = Vec::new();
        for tag in &tags {
            match groups
                .iter()
                .find(|group| loosely_equal(group.name, tag.group.name))
            {
                Some(&known) if known != tag.group => {
                    duplicates.insert(tag.group.name);
                }
                Some(_) => {}
                None => groups.push(tag.group),
            }
        }
        duplicates.extend(
            labels
                .keys()
                .copied()
                .filter(|&label| groups.iter().any(|group| loosely_equal(label, group.name))),
        );
        if !duplicates.is_empty() {
            return Err(duplicates
                .into_iter()
                .map(|label| VocabularyError::DuplicateLabel { label })
                .collect());
        }
        let closures = compute_closures(&parents);
        let vocabulary = Self {
            tags,
            parents,
            closures,
            labels,
            groups,
        };
        let unsatisfiable = vocabulary.unsatisfiable();
        if unsatisfiable.is_empty() {
            Ok(vocabulary)
        } else {
            Err(unsatisfiable)
        }
    }

    /// The tag at `id`.
    #[must_use]
    pub fn tag(&self, id: TagId) -> &Tag {
        &self.tags[id.0]
    }

    /// The tag whose canonical label or alias is `label`.
    #[must_use]
    pub fn id(&self, label: &str) -> Option<TagId> {
        self.labels.get(label).copied()
    }

    /// Resolves a runtime tag label or alias string to its [`TagId`].
    ///
    /// # Errors
    ///
    /// Returns [`TagLookupError::Group`] if `label` matches a group name (ignoring ASCII case and
    /// `-`/`_`/space differences), or [`TagLookupError::Unknown`] if it matches nothing.
    pub fn resolve(&self, label: &str) -> Result<TagId, TagLookupError> {
        if let Some(tag) = self.id(label) {
            return Ok(tag);
        }
        Err(self
            .group(label)
            .map_or(TagLookupError::Unknown, TagLookupError::Group))
    }

    /// The group whose name loosely equals `name` (ASCII case-insensitively, with spaces,
    /// underscores and hyphens equivalent).
    #[must_use]
    pub fn group(&self, name: &str) -> Option<&'static TagGroup> {
        self.groups
            .iter()
            .copied()
            .find(|group| loosely_equal(name, group.name))
    }

    /// Every tag, in vocabulary order: the tags given to [`Self::new`] in the order given, then
    /// the implied tags that were not given, in the order first reached.
    pub fn tags(&self) -> impl ExactSizeIterator<Item = TagId> + use<> {
        (0..self.tags.len()).map(TagId)
    }

    /// Every group of a vocabulary tag, in the order its first tag appears in vocabulary order.
    pub fn groups(&self) -> impl Iterator<Item = &'static TagGroup> {
        self.groups.iter().copied()
    }

    /// Every canonical label and alias.
    pub fn labels(&self) -> impl Iterator<Item = &'static str> {
        self.labels.keys().copied()
    }

    /// The tags of `group`, in vocabulary order.
    pub fn members(&self, group: &TagGroup) -> impl Iterator<Item = TagId> {
        self.tags()
            .filter(move |&id| self.tag(id).belongs_to(group))
    }

    /// Whether `narrower` implies `broader`. Every tag implies itself.
    #[must_use]
    pub fn implies(&self, narrower: TagId, broader: TagId) -> bool {
        self.closures[narrower.0].contains(broader.0)
    }

    /// A shortest implication chain from `narrower` up to `broader`, listed from `broader`
    /// down to `narrower`; empty if `narrower` does not imply `broader`. Among chains of
    /// equal length, the one through earlier-declared parents wins.
    #[must_use]
    pub fn chain(&self, narrower: TagId, broader: TagId) -> Vec<TagId> {
        let reached = self.breadth_first(narrower);
        chain_from(&reached, narrower, broader)
    }

    /// A shortest implication chain from `id` up to a root tag (one that implies no broader
    /// tag), listed from that root down to `id`. Among roots at equal distance, the one
    /// reached through earlier-declared parents wins.
    #[must_use]
    pub fn root_chain(&self, id: TagId) -> Vec<TagId> {
        let reached = self.breadth_first(id);
        let root = reached
            .iter()
            .map(|step| step.tag)
            .find(|&tag| self.parents[tag.0].is_empty())
            .unwrap_or(id);
        chain_from(&reached, id, root)
    }

    /// A shortest implication chain from a root tag down through `through` to `narrower`, listed
    /// from that root down to `narrower`; empty if `narrower` does not imply `through`.
    ///
    /// Combines [`Self::root_chain`] for `through` with [`Self::chain`] from `narrower` up to
    /// `through` without repeating `through`. Among chains of equal length, the one through
    /// earlier-declared parents wins. This guarantees the returned path passes through `through`
    /// even when `narrower` has a shorter or earlier root path on another parent branch.
    #[must_use]
    pub fn chain_through(&self, narrower: TagId, through: TagId) -> Vec<TagId> {
        if !self.implies(narrower, through) {
            return Vec::new();
        }
        let mut path = self.root_chain(through);
        path.extend(self.chain(narrower, through).into_iter().skip(1));
        path
    }

    /// Two tags of `tags` in `group` that do not imply each other, if `group` is exclusive.
    pub(crate) fn exclusive_conflict(
        &self,
        tags: &FixedBitSet,
        group: &TagGroup,
    ) -> Option<[TagId; 2]> {
        if !group.cardinality.is_exclusive() {
            return None;
        }
        let members: Vec<TagId> = tags
            .ones()
            .map(TagId)
            .filter(|&member| self.tag(member).belongs_to(group))
            .collect();
        members
            .iter()
            .flat_map(|&first| members.iter().map(move |&second| [first, second]))
            .find(|&[first, second]| !self.implies(first, second) && !self.implies(second, first))
    }

    /// Finds tags that imply two conflicting tags in the same exclusive group, omitting tags whose
    /// conflict is already inherited from an implied parent tag.
    fn unsatisfiable(&self) -> Vec<VocabularyError> {
        let mut errors = Vec::new();
        for &group in &self.groups {
            let group_conflicts: Vec<Option<[TagId; 2]>> = self
                .tags()
                .map(|id| self.exclusive_conflict(&self.closures[id.0], group))
                .collect();
            for id in self.tags() {
                let Some([first, second]) = group_conflicts[id.0] else {
                    continue;
                };
                let inherited = self.closures[id.0]
                    .ones()
                    .any(|ancestor| ancestor != id.0 && group_conflicts[ancestor].is_some());
                if !inherited {
                    errors.push(VocabularyError::Unsatisfiable {
                        tag: self.tag(id).label,
                        group: group.name,
                        conflicting: [self.tag(first).label, self.tag(second).label],
                    });
                }
            }
        }
        errors
    }

    /// The tags `id` implies, excluding itself, nearest first, each with the tag it was
    /// first reached from.
    fn breadth_first(&self, id: TagId) -> Vec<Reached> {
        let mut seen = FixedBitSet::with_capacity(self.tags.len());
        seen.insert(id.0);
        let mut queue = VecDeque::from([id]);
        let mut reached = Vec::new();
        while let Some(current) = queue.pop_front() {
            for &parent in &self.parents[current.0] {
                if !seen.contains(parent.0) {
                    seen.insert(parent.0);
                    reached.push(Reached {
                        tag: parent,
                        from: current,
                    });
                    queue.push_back(parent);
                }
            }
        }
        reached
    }
}

/// Tags collected by label, with the labels that name two different tags.
#[derive(Default)]
struct Interner {
    tags: Vec<Tag>,
    parents: Vec<Vec<TagId>>,
    labels: BTreeMap<&'static str, TagId>,
    duplicates: BTreeSet<&'static str>,
}

impl Interner {
    /// Records the tags `id` directly implies, adding and linking those not added yet.
    fn link(&mut self, id: TagId) {
        let implies = self.tags[id.0].implies;
        let parents = implies
            .iter()
            .map(|&parent| {
                if let Some(added) = self.add(parent) {
                    self.link(added);
                }
                self.labels[parent.label]
            })
            .collect();
        self.parents[id.0] = parents;
    }

    /// Adds `tag` and returns its id if its label is new; otherwise records the label as a
    /// duplicate when it names a different tag.
    fn add(&mut self, tag: Tag) -> Option<TagId> {
        if let Some(&id) = self.labels.get(tag.label) {
            if self.tags[id.0] != tag {
                self.duplicates.insert(tag.label);
            }
            return None;
        }
        let id = TagId(self.tags.len());
        self.tags.push(tag);
        self.parents.push(Vec::new());
        self.labels.insert(tag.label, id);
        Some(id)
    }
}

/// A tag reached by a breadth-first walk up the implications, and the tag it was reached from.
struct Reached {
    tag: TagId,
    from: TagId,
}

/// Whether `left` and `right` are equal ASCII case-insensitively, with spaces, underscores
/// and hyphens equivalent.
fn loosely_equal(left: &str, right: &str) -> bool {
    let normalize = |byte: u8| match byte {
        b' ' | b'_' => b'-',
        other => other.to_ascii_lowercase(),
    };
    left.bytes().map(normalize).eq(right.bytes().map(normalize))
}

/// Computes, for each tag, the set of tags it implies (including itself).
fn compute_closures(parents: &[Vec<TagId>]) -> Vec<FixedBitSet> {
    let len = parents.len();
    let mut visited = FixedBitSet::with_capacity(len);
    let mut parents_first = Vec::with_capacity(len);
    for index in 0..len {
        visit_post_order(index, parents, &mut visited, &mut parents_first);
    }
    let mut closures = vec![FixedBitSet::with_capacity(len); len];
    for index in parents_first {
        let mut closure = FixedBitSet::with_capacity(len);
        closure.insert(index);
        for parent in &parents[index] {
            closure.union_with(&closures[parent.0]);
        }
        closures[index] = closure;
    }
    closures
}

/// Appends `index` to `order` after every tag it implies.
fn visit_post_order(
    index: usize,
    parents: &[Vec<TagId>],
    visited: &mut FixedBitSet,
    order: &mut Vec<usize>,
) {
    if visited.contains(index) {
        return;
    }
    visited.insert(index);
    for parent in &parents[index] {
        visit_post_order(parent.0, parents, visited, order);
    }
    order.push(index);
}

/// Reconstructs the path from `broader` down to `narrower` recorded in `reached`; empty if
/// `broader` was not reached.
fn chain_from(reached: &[Reached], narrower: TagId, broader: TagId) -> Vec<TagId> {
    let mut chain = vec![broader];
    let mut current = broader;
    while current != narrower {
        let Some(step) = reached.iter().find(|step| step.tag == current) else {
            return Vec::new();
        };
        current = step.from;
        chain.push(current);
    }
    chain
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;
    use crate::tag::Cardinality;

    const TOPIC: TagGroup = TagGroup {
        name: "Topic",
        cardinality: Cardinality::Any,
    };
    const OS: TagGroup = TagGroup {
        name: "Operating system",
        cardinality: Cardinality::AtMostOne,
    };
    const SPEED: TagGroup = TagGroup {
        name: "Speed",
        cardinality: Cardinality::ExactlyOne,
    };
    const OTHER_OS: TagGroup = TagGroup {
        name: "Operating system",
        cardinality: Cardinality::Any,
    };
    const LOWERCASE_OS: TagGroup = TagGroup {
        name: "operating_system",
        ..OS
    };
    const ROOT: Tag = Tag {
        label: "root",
        group: &TOPIC,
        implies: &[],
        aliases: &[],
        description: "Root.",
        scope_note: None,
    };
    const VCS: Tag = Tag {
        label: "vcs",
        ..ROOT
    };
    const JJ: Tag = Tag {
        label: "jj",
        implies: &[VCS],
        aliases: &["jujutsu"],
        ..ROOT
    };
    const KERNEL: Tag = Tag {
        label: "kernel",
        ..ROOT
    };
    const UNIX: Tag = Tag {
        label: "unix",
        group: &OS,
        ..ROOT
    };
    const LINUX: Tag = Tag {
        label: "linux",
        group: &OS,
        implies: &[UNIX],
        ..ROOT
    };
    const WINDOWS: Tag = Tag {
        label: "windows",
        group: &OS,
        ..ROOT
    };
    const FAST: Tag = Tag {
        label: "fast",
        group: &SPEED,
        ..ROOT
    };
    const SLOW: Tag = Tag {
        label: "slow",
        group: &SPEED,
        ..ROOT
    };
    const EBPF: Tag = Tag {
        label: "ebpf",
        implies: &[LINUX, KERNEL],
        ..ROOT
    };
    const DIAMOND: Tag = Tag {
        label: "diamond",
        implies: &[EBPF, LINUX],
        ..ROOT
    };
    const PORTABLE: Tag = Tag {
        label: "portable",
        implies: &[LINUX, WINDOWS],
        ..ROOT
    };
    const PORTABLE_TOOL: Tag = Tag {
        label: "portable-tool",
        implies: &[PORTABLE],
        ..ROOT
    };
    const PORTABLE_CONFUSED: Tag = Tag {
        label: "portable-confused",
        implies: &[PORTABLE, FAST, SLOW],
        ..ROOT
    };

    fn vocabulary(tags: &[Tag]) -> Vocabulary {
        Vocabulary::new(tags.iter().copied()).unwrap()
    }

    fn labels(vocabulary: &Vocabulary, ids: impl IntoIterator<Item = TagId>) -> Vec<&str> {
        ids.into_iter().map(|id| vocabulary.tag(id).label).collect()
    }

    #[rstest]
    #[case::empty(&[], &[])]
    #[case::given_after_a_tag_implying_it(&[JJ, ROOT, VCS], &["jj", "root", "vcs"])]
    #[case::implied_but_not_given(&[JJ, ROOT], &["jj", "root", "vcs"])]
    #[case::given_before_a_tag_implying_it(&[VCS, ROOT, JJ], &["vcs", "root", "jj"])]
    fn given_tags_keep_their_order_and_implied_tags_follow(
        #[case] given: &[Tag],
        #[case] expected: &[&str],
    ) {
        let built = vocabulary(given);
        assert_eq!(labels(&built, built.tags()), expected);
    }

    #[rstest]
    #[case::itself("linux", "linux", true)]
    #[case::parent("linux", "unix", true)]
    #[case::grandparent("ebpf", "unix", true)]
    #[case::second_parent("ebpf", "kernel", true)]
    #[case::descendant("unix", "linux", false)]
    #[case::sibling("linux", "kernel", false)]
    fn implication_is_the_reflexive_transitive_closure(
        #[case] narrower: &str,
        #[case] broader: &str,
        #[case] expected: bool,
    ) {
        let built = vocabulary(&[EBPF]);
        let [narrower_id, broader_id] = [narrower, broader].map(|label| built.id(label).unwrap());
        assert_eq!(built.implies(narrower_id, broader_id), expected);
    }

    #[test]
    fn members_are_the_tags_of_a_group() {
        let built = vocabulary(&[EBPF]);
        assert_eq!(labels(&built, built.members(&OS)), ["linux", "unix"]);
    }

    #[rstest]
    #[case::itself("linux", "linux", &["linux"])]
    #[case::shortest_of_two_paths("diamond", "unix", &["unix", "linux", "diamond"])]
    #[case::through_the_second_parent("diamond", "kernel", &["kernel", "ebpf", "diamond"])]
    #[case::not_implied("linux", "kernel", &[])]
    fn a_chain_is_a_shortest_implication_path(
        #[case] narrower: &str,
        #[case] broader: &str,
        #[case] expected: &[&str],
    ) {
        let built = vocabulary(&[DIAMOND]);
        let [narrower_id, broader_id] = [narrower, broader].map(|label| built.id(label).unwrap());
        let chain = built.chain(narrower_id, broader_id);
        assert_eq!(labels(&built, chain), expected);
    }

    #[rstest]
    #[case::root_itself("unix", &["unix"])]
    #[case::single_parent("linux", &["unix", "linux"])]
    #[case::nearest_root_beats_deeper_earlier_parent("ebpf", &["kernel", "ebpf"])]
    #[case::earlier_parent_breaks_equal_depth_ties("diamond", &["kernel", "ebpf", "diamond"])]
    fn a_root_chain_reaches_the_nearest_root(#[case] tag: &str, #[case] expected: &[&str]) {
        let built = vocabulary(&[DIAMOND]);
        let id = built.id(tag).unwrap();
        assert_eq!(labels(&built, built.root_chain(id)), expected);
    }

    #[rstest]
    #[case::root_itself("unix", "unix", &["unix"])]
    #[case::non_root_itself("linux", "linux", &["unix", "linux"])]
    #[case::through_root("ebpf", "unix", &["unix", "linux", "ebpf"])]
    #[case::routes_through_longer_parent_branch("ebpf", "linux", &["unix", "linux", "ebpf"])]
    #[case::diamond_through_intermediate("diamond", "ebpf", &["kernel", "ebpf", "diamond"])]
    #[case::unrelated_tags("linux", "kernel", &[])]
    #[case::reversed_direction("unix", "linux", &[])]
    fn chain_through_routes_from_root_through_the_given_tag(
        #[case] narrower: &str,
        #[case] through: &str,
        #[case] expected: &[&str],
    ) {
        let built = vocabulary(&[DIAMOND]);
        let [narrower_id, through_id] = [narrower, through].map(|label| built.id(label).unwrap());
        let chain = built.chain_through(narrower_id, through_id);
        assert_eq!(labels(&built, chain), expected);
    }

    #[test]
    fn groups_are_listed_in_the_order_first_reached() {
        let built = vocabulary(&[JJ, LINUX]);
        let groups: Vec<_> = built.groups().map(|group| group.name).collect();
        assert_eq!(groups, ["Topic", "Operating system"]);
    }

    #[test]
    fn labels_include_aliases_and_canonical_filter_excludes_aliases() {
        let built = vocabulary(&[JJ, LINUX]);
        let all_labels: Vec<_> = built.labels().collect();
        assert_eq!(all_labels, ["jj", "jujutsu", "linux", "unix", "vcs"]);
    }

    #[rstest]
    #[case::canonical("jj", Ok(TagId(0)))]
    #[case::alias("jujutsu", Ok(TagId(0)))]
    #[case::group("operating_SYSTEM", Err(TagLookupError::Group(&OS)))]
    #[case::unknown("tesing", Err(TagLookupError::Unknown))]
    fn resolve_distinguishes_tags_groups_and_unknown_labels(
        #[case] label: &str,
        #[case] expected: Result<TagId, TagLookupError>,
    ) {
        let built = vocabulary(&[JJ, LINUX]);
        assert_eq!(built.resolve(label), expected);
    }

    #[rstest]
    #[case::group(TagLookupError::Group(&OS), "`Operating system` is a group, not a tag")]
    #[case::unknown(TagLookupError::Unknown, "the label names no tag or alias")]
    fn tag_lookup_errors_explain_themselves(#[case] error: TagLookupError, #[case] expected: &str) {
        assert_eq!(error.to_string(), expected);
    }

    #[rstest]
    #[case::two_tags_one_label(
        &[VCS, Tag { description: "Other.", ..VCS }],
        &["`vcs` names more than one tag, alias or group"]
    )]
    #[case::redefined_tag_in_implies(
        &[JJ, Tag { description: "Other.", ..VCS }],
        &["`vcs` names more than one tag, alias or group"]
    )]
    #[case::alias_is_a_label(
        &[JJ, Tag { label: "jujutsu", ..ROOT }],
        &["`jujutsu` names more than one tag, alias or group"]
    )]
    #[case::label_is_a_group_name(
        &[LINUX, Tag { label: "operating-system", ..ROOT }],
        &["`operating-system` names more than one tag, alias or group"]
    )]
    #[case::two_groups_one_name(
        &[LINUX, Tag { label: "bsd", group: &OTHER_OS, ..ROOT }],
        &["`Operating system` names more than one tag, alias or group"]
    )]
    #[case::two_groups_loosely_one_name(
        &[LINUX, Tag { label: "bsd", group: &LOWERCASE_OS, ..ROOT }],
        &["`operating_system` names more than one tag, alias or group"]
    )]
    #[case::several_violations_in_label_order(
        &[JJ, Tag { label: "jujutsu", ..ROOT }, VCS, Tag { description: "Other.", ..VCS }],
        &[
            "`jujutsu` names more than one tag, alias or group",
            "`vcs` names more than one tag, alias or group"
        ]
    )]
    #[case::exclusive_tags_together(
        &[PORTABLE_TOOL],
        &[
            "`portable` implies both `linux` and `windows`, which group `Operating system` \
             allows only one of"
        ]
    )]
    #[case::child_introduces_conflict_in_second_exclusive_group(
        &[PORTABLE_CONFUSED],
        &[
            "`portable` implies both `linux` and `windows`, which group `Operating system` \
             allows only one of",
            "`portable-confused` implies both `fast` and `slow`, which group `Speed` \
             allows only one of"
        ]
    )]
    fn invalid_vocabularies_report_every_violation(
        #[case] tags: &[Tag],
        #[case] expected: &[&str],
    ) {
        let errors = Vocabulary::new(tags.iter().copied()).unwrap_err();
        let messages: Vec<String> = errors.iter().map(ToString::to_string).collect();
        assert_eq!(messages, expected);
    }

    #[test]
    fn chains_in_exclusive_and_non_exclusive_groups_are_satisfiable() {
        let both = Tag {
            label: "both",
            implies: &[VCS, KERNEL],
            ..ROOT
        };
        assert!(Vocabulary::new([LINUX, both]).is_ok());
    }
}
