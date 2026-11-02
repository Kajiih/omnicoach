//! Vocabularies: a closed, validated set of tags with its implication closure and label
//! registry.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use fixedbitset::FixedBitSet;
use thiserror::Error;

use crate::tag::{Tag, TagGroup};

/// A tag's position in a [`Vocabulary`].
///
/// An id is only meaningful for the vocabulary that issued it, and for catalogs and plans
/// built on that vocabulary: other vocabularies panic or answer wrongly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TagId(usize);

impl TagId {
    /// The id at position `index`.
    pub(crate) const fn new(index: usize) -> Self {
        Self(index)
    }

    /// The id's position.
    pub(crate) const fn index(self) -> usize {
        self.0
    }
}

/// Why a set of tags is not a valid [`Vocabulary`].
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum VocabularyError {
    /// The label names two different tags or groups, or a tag and an alias. Group names
    /// are compared loosely (ASCII case-insensitively, with spaces, underscores and hyphens
    /// equivalent).
    #[error("`{label}` names more than one tag, alias or group")]
    DuplicateLabel {
        /// The label.
        label: &'static str,
    },
    /// The tag implies, or is, two tags that no item may carry together.
    #[error(
        "`{tag}` implies both `{}` and `{}`, which group `{group}` allows only one of",
        conflicting[0],
        conflicting[1]
    )]
    Unsatisfiable {
        /// The tag.
        tag: &'static str,
        /// The exclusive group of the two tags.
        group: &'static str,
        /// Two tags of `group` in the tag's closure (possibly the tag itself) that do not
        /// imply each other.
        conflicting: [&'static str; 2],
    },
}

/// A closed set of tags: every tag implied by a member is a member. Labels, aliases and
/// group names are unique, and every tag can be carried without breaking an exclusive
/// group.
#[derive(Debug)]
pub struct Vocabulary {
    tags: Vec<Tag>,
    parents: Vec<Vec<TagId>>,
    /// Each tag's reflexive upward closure.
    closures: Vec<FixedBitSet>,
    /// For each tag, the strictly narrower tags that imply it.
    strict_narrower: Vec<FixedBitSet>,
    /// Canonical labels and aliases.
    labels: BTreeMap<&'static str, TagId>,
    groups: Vec<&'static TagGroup>,
}

impl Vocabulary {
    /// The vocabulary of `tags`, in the order given, followed by the tags they imply that were
    /// not given, in the order first reached.
    ///
    /// # Errors
    ///
    /// Returns every [`VocabularyError`], in a deterministic order. Duplicate labels are
    /// reported alone: implications are only defined once labels are unique.
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
        let closures = closures(&parents);
        let strict_narrower = strict_narrower(&closures);
        let vocabulary = Self {
            tags,
            parents,
            closures,
            strict_narrower,
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
    pub fn tags(&self) -> impl ExactSizeIterator<Item = TagId> {
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
        self.tags().filter(|&id| self.tag(id).belongs_to(group))
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

    /// Whether `narrower` implies `broader`. Every tag implies itself.
    #[must_use]
    pub fn implies(&self, narrower: TagId, broader: TagId) -> bool {
        self.closures[narrower.0].contains(broader.0)
    }

    /// The tags `id` implies, including itself.
    pub(crate) fn closure(&self, id: TagId) -> &FixedBitSet {
        &self.closures[id.0]
    }

    /// The tags that strictly imply `id` (excluding `id` itself).
    pub(crate) fn strict_narrower(&self, id: TagId) -> &FixedBitSet {
        &self.strict_narrower[id.0]
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

    /// The unsatisfiable tags none of whose ancestors is unsatisfiable: the others inherit
    /// the conflict.
    fn unsatisfiable(&self) -> Vec<VocabularyError> {
        let conflicts: Vec<Option<[TagId; 2]>> = self
            .tags()
            .map(|id| {
                self.groups
                    .iter()
                    .find_map(|group| self.exclusive_conflict(&self.closures[id.0], group))
            })
            .collect();
        self.tags()
            .filter(|&id| {
                self.closures[id.0]
                    .ones()
                    .all(|ancestor| ancestor == id.0 || conflicts[ancestor].is_none())
            })
            .filter_map(|id| {
                let [first, second] = conflicts[id.0]?;
                Some(VocabularyError::Unsatisfiable {
                    tag: self.tag(id).label,
                    group: self.tag(first).group.name,
                    conflicting: [self.tag(first).label, self.tag(second).label],
                })
            })
            .collect()
    }
}

/// A tag reached by a breadth-first walk up the implications, and the tag it was reached
/// from.
struct Reached {
    tag: TagId,
    from: TagId,
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

/// Whether `left` and `right` are equal ASCII case-insensitively, with spaces, underscores
/// and hyphens equivalent.
fn loosely_equal(left: &str, right: &str) -> bool {
    let normalize = |byte: u8| match byte {
        b' ' | b'_' => b'-',
        other => other.to_ascii_lowercase(),
    };
    left.bytes().map(normalize).eq(right.bytes().map(normalize))
}

/// Each tag's reflexive upward closure.
///
/// `parents` is acyclic: tags are finite values, so a tag cannot transitively imply itself,
/// and a label never names two different tags here.
fn closures(parents: &[Vec<TagId>]) -> Vec<FixedBitSet> {
    let len = parents.len();
    let mut visited = FixedBitSet::with_capacity(len);
    let mut parents_first = Vec::with_capacity(len);
    for index in 0..len {
        visit(index, parents, &mut visited, &mut parents_first);
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
fn visit(index: usize, parents: &[Vec<TagId>], visited: &mut FixedBitSet, order: &mut Vec<usize>) {
    if visited.contains(index) {
        return;
    }
    visited.insert(index);
    for parent in &parents[index] {
        visit(parent.0, parents, visited, order);
    }
    order.push(index);
}

/// For each tag, the strictly narrower tags that imply it.
fn strict_narrower(closures: &[FixedBitSet]) -> Vec<FixedBitSet> {
    let len = closures.len();
    let mut narrower = vec![FixedBitSet::with_capacity(len); len];
    for (sub, closure) in closures.iter().enumerate() {
        for sup in closure.ones() {
            if sup != sub {
                narrower[sup].insert(sub);
            }
        }
    }
    narrower
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

    fn vocabulary(tags: &[Tag]) -> Vocabulary {
        Vocabulary::new(tags.iter().copied()).unwrap()
    }

    fn labels(vocabulary: &Vocabulary, ids: impl IntoIterator<Item = TagId>) -> Vec<&str> {
        ids.into_iter().map(|id| vocabulary.tag(id).label).collect()
    }

    /// Given tags keep their order, even when an earlier one implies a later one; implied tags
    /// that were not given follow them.
    #[rstest]
    #[case::given_after_a_tag_implying_it(&[JJ, ROOT, VCS], &["jj", "root", "vcs"])]
    #[case::implied_but_not_given(&[JJ, ROOT], &["jj", "root", "vcs"])]
    #[case::given_before_a_tag_implying_it(&[VCS, ROOT, JJ], &["vcs", "root", "jj"])]
    fn given_tags_keep_their_order_and_implied_tags_follow(
        #[case] given: &[Tag],
        #[case] expected: &[&str],
    ) {
        let vocabulary = vocabulary(given);
        assert_eq!(labels(&vocabulary, vocabulary.tags()), expected);
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
        let vocabulary = vocabulary(&[EBPF]);
        let [narrower, broader] = [narrower, broader].map(|label| vocabulary.id(label).unwrap());
        assert_eq!(vocabulary.implies(narrower, broader), expected);
    }

    #[test]
    fn members_are_the_tags_of_a_group() {
        let vocabulary = vocabulary(&[EBPF]);
        assert_eq!(
            labels(&vocabulary, vocabulary.members(&OS)),
            ["linux", "unix"]
        );
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
        let vocabulary = vocabulary(&[DIAMOND]);
        let [narrower, broader] = [narrower, broader].map(|label| vocabulary.id(label).unwrap());
        let chain = vocabulary.chain(narrower, broader);
        assert_eq!(labels(&vocabulary, chain), expected);
    }

    #[rstest]
    #[case::root_itself("unix", &["unix"])]
    #[case::single_parent("linux", &["unix", "linux"])]
    #[case::nearest_root_beats_deeper_earlier_parent("ebpf", &["kernel", "ebpf"])]
    #[case::earlier_parent_breaks_equal_depth_ties("diamond", &["kernel", "ebpf", "diamond"])]
    fn a_root_chain_reaches_the_nearest_root(#[case] tag: &str, #[case] expected: &[&str]) {
        let vocabulary = vocabulary(&[DIAMOND]);
        let id = vocabulary.id(tag).unwrap();
        assert_eq!(labels(&vocabulary, vocabulary.root_chain(id)), expected);
    }

    #[test]
    fn groups_are_listed_in_the_order_first_reached() {
        let vocabulary = vocabulary(&[JJ, LINUX]);
        let groups: Vec<_> = vocabulary.groups().map(|group| group.name).collect();
        assert_eq!(groups, ["Topic", "Operating system"]);
    }

    #[test]
    fn labels_include_aliases() {
        let labels: Vec<_> = vocabulary(&[JJ, LINUX]).labels().collect();
        assert_eq!(labels, ["jj", "jujutsu", "linux", "unix", "vcs"]);
    }

    #[test]
    fn an_alias_resolves_to_its_tag() {
        let vocabulary = vocabulary(&[JJ]);
        assert_eq!(vocabulary.id("jujutsu"), vocabulary.id("jj"));
    }

    #[rstest]
    #[case::exact("Operating system")]
    #[case::case_and_separators("operating_SYSTEM")]
    fn group_names_match_loosely(#[case] name: &str) {
        assert_eq!(vocabulary(&[LINUX]).group(name), Some(&OS));
    }

    #[test]
    fn an_unknown_group_name_matches_nothing() {
        assert_eq!(vocabulary(&[LINUX]).group("operating"), None);
    }

    #[rstest]
    #[case::two_tags_one_label(
        &[VCS, Tag { description: "Other.", ..VCS }],
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
    fn invalid_vocabularies_report_every_violation(
        #[case] tags: &[Tag],
        #[case] expected: &[&str],
    ) {
        let errors = Vocabulary::new(tags.iter().copied()).unwrap_err();
        let messages: Vec<String> = errors.iter().map(ToString::to_string).collect();
        assert_eq!(messages, expected);
    }

    #[test]
    fn a_chain_in_an_exclusive_group_is_satisfiable() {
        assert!(Vocabulary::new([LINUX]).is_ok());
    }

    #[test]
    fn tags_of_a_non_exclusive_group_combine_freely() {
        let both = Tag {
            label: "both",
            implies: &[VCS, KERNEL],
            ..ROOT
        };
        assert!(Vocabulary::new([both]).is_ok());
    }
}
