//! Validated tags for a single item, storing both explicit tags and all tags they imply.

use fixedbitset::FixedBitSet;
use thiserror::Error;

use crate::tag::Tag;
use crate::vocabulary::{TagId, Vocabulary};

/// Why a list of tags is not a valid [`TagSet`] under a [`Vocabulary`].
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum TagSetError {
    /// A tag's label is not a canonical tag of the vocabulary.
    #[error("`{tag}` is not a tag of the vocabulary")]
    UnknownTag {
        /// The unknown tag's label.
        tag: &'static str,
    },
    /// A tag shares its canonical label with a vocabulary tag defined differently.
    #[error("`{tag}` differs from the vocabulary's definition")]
    RedefinedTag {
        /// The redefined tag's label.
        tag: &'static str,
    },
    /// The same tag appears more than once in the explicit list.
    #[error("`{tag}` is listed more than once")]
    RepeatedTag {
        /// The repeated tag's label.
        tag: &'static str,
    },
    /// A tag is already implied by a more specific tag on the same item (for example, listing both
    /// `linux` and `ebpf` when `ebpf` already implies `linux`).
    #[error("remove `{tag}`, it is implied by `{implied_by}`")]
    RedundantTag {
        /// The implied broader tag's label.
        tag: &'static str,
        /// The narrower explicit tag's label.
        implied_by: &'static str,
    },
    /// None of the item's explicit or implied tags belong to a required group (`AtLeastOne` or
    /// `ExactlyOne`).
    #[error("carries no tag of group `{group}`")]
    MissingGroup {
        /// The required group's name.
        group: &'static str,
    },
    /// Two of the item's explicit or implied tags belong to an exclusive group (`AtMostOne` or
    /// `ExactlyOne`) and neither implies the other (for example, `linux` and `windows`).
    #[error(
        "carries both `{}` and `{}`, which group `{group}` allows only one of",
        conflicting[0],
        conflicting[1]
    )]
    ExclusiveGroup {
        /// The exclusive group's name.
        group: &'static str,
        /// The two conflicting tags' labels.
        conflicting: [&'static str; 2],
    },
}

/// A single item's validated tags under a [`Vocabulary`].
///
/// Every `TagSet` guarantees three properties:
///
/// - **Most-specific explicit tags only**: no tag is repeated or already implied by another
///   explicit tag (for example, `[ebpf]` is valid; `[ebpf, linux]` is rejected as redundant).
/// - **Precomputed implied tags**: [`TagSet::carries`] checks both explicit tags and every broader
///   tag they imply in constant time.
/// - **Group rules satisfied**: every required group has at least one tag, and every exclusive
///   group has at most one tag (or one implication chain).
///
/// A `TagSet` is only valid with the [`Vocabulary`] that created it: passing it to queries or
/// selections built on another vocabulary can panic or return wrong results.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TagSet {
    explicit: Vec<TagId>,
    effective: FixedBitSet,
}

impl TagSet {
    /// Validates `tags` against `vocabulary` and precomputes all tags they imply.
    ///
    /// # Errors
    ///
    /// Returns every [`TagSetError`] found across `tags` and `vocabulary`'s group rules.
    pub fn new(
        vocabulary: &Vocabulary,
        tags: impl IntoIterator<Item = Tag>,
    ) -> Result<Self, Vec<TagSetError>> {
        let mut errors = Vec::new();
        let mut explicit = Vec::new();
        let mut repeated = FixedBitSet::with_capacity(vocabulary.tags().len());
        for tag in tags {
            match vocabulary.by_canonical_label(tag.label) {
                None => errors.push(TagSetError::UnknownTag { tag: tag.label }),
                Some(id) if vocabulary.tag(id) != &tag => {
                    errors.push(TagSetError::RedefinedTag { tag: tag.label });
                }
                Some(id) if explicit.contains(&id) => {
                    if !repeated.put(id.index()) {
                        errors.push(TagSetError::RepeatedTag { tag: tag.label });
                    }
                }
                Some(id) => explicit.push(id),
            }
        }
        for &tag in &explicit {
            if let Some(&narrower) = explicit
                .iter()
                .find(|&&other| other != tag && vocabulary.implies(other, tag))
            {
                errors.push(TagSetError::RedundantTag {
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
            let has_member = || {
                effective
                    .ones()
                    .any(|index| vocabulary.tag(TagId::new(index)).belongs_to(group))
            };
            if group.cardinality.is_required() && !has_member() {
                errors.push(TagSetError::MissingGroup { group: group.name });
            }
            if let Some([first, second]) = vocabulary.exclusive_conflict(&effective, group) {
                errors.push(TagSetError::ExclusiveGroup {
                    group: group.name,
                    conflicting: [vocabulary.tag(first).label, vocabulary.tag(second).label],
                });
            }
        }
        if errors.is_empty() {
            Ok(Self {
                explicit,
                effective,
            })
        } else {
            Err(errors)
        }
    }

    /// The explicit tags, in the order given to [`Self::new`].
    #[must_use]
    pub fn explicit(&self) -> &[TagId] {
        &self.explicit
    }

    /// Whether the set carries `tag`, explicitly or by implication.
    #[must_use]
    pub fn carries(&self, tag: TagId) -> bool {
        self.effective.contains(tag.index())
    }

    /// Whether `tag` was attached explicitly (rather than only implied by a narrower tag).
    #[must_use]
    pub fn carries_explicit(&self, tag: TagId) -> bool {
        self.explicit.contains(&tag)
    }

    /// Every tag carried explicitly or by implication, in vocabulary order.
    pub fn effective(&self) -> impl Iterator<Item = TagId> + '_ {
        self.effective.ones().map(TagId::new)
    }

    /// Whether the set carries zero tags.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.explicit.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;
    use crate::tag::{Cardinality, TagGroup};

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
    const FREE: TagGroup = TagGroup {
        name: "Free",
        cardinality: Cardinality::Any,
    };
    const VCS: Tag = Tag::new(&TOPIC, "vcs", "Version control.").aliases(&["version-control"]);
    const JJ: Tag = Tag::new(&TOPIC, "jj", "Jujutsu.").implies(&[VCS]);
    const TESTING: Tag = Tag::new(&TOPIC, "testing", "Testing.");
    const OPINIONATED: Tag = Tag::new(&CONSENSUS, "opinionated", "Opinionated.");
    const UNOPINIONATED: Tag = Tag::new(&CONSENSUS, "unopinionated", "Unopinionated.");
    const TESTS_ONLY: Tag = Tag::new(&SCOPE, "tests-only", "Tests only.");

    fn vocabulary() -> Vocabulary {
        Vocabulary::new([JJ, TESTING, OPINIONATED, UNOPINIONATED, TESTS_ONLY]).unwrap()
    }

    #[test]
    fn explicit_and_effective_queries_reflect_implication_closure() {
        let vocab = vocabulary();
        let set = TagSet::new(&vocab, [JJ, OPINIONATED, TESTS_ONLY]).unwrap();
        let [jj, vcs, testing] = ["jj", "vcs", "testing"].map(|label| vocab.id(label).unwrap());

        assert!(set.carries_explicit(jj));
        assert!(!set.carries_explicit(vcs));
        assert!(set.carries(vcs));
        assert!(!set.carries(testing));
    }

    #[test]
    fn effective_and_explicit_lists_preserve_expected_order() {
        let vocab = vocabulary();
        let set = TagSet::new(&vocab, [TESTS_ONLY, JJ, OPINIONATED]).unwrap();
        let explicit_labels: Vec<_> = set
            .explicit()
            .iter()
            .map(|&id| vocab.tag(id).label)
            .collect();
        let effective_labels: Vec<_> = set.effective().map(|id| vocab.tag(id).label).collect();

        assert_eq!(explicit_labels, ["tests-only", "jj", "opinionated"]);
        assert_eq!(effective_labels, ["jj", "opinionated", "tests-only", "vcs"]);
        assert!(!set.is_empty());
    }

    #[test]
    fn empty_tag_set_is_allowed_when_no_group_is_required() {
        let optional = Tag::new(&FREE, "optional", "Optional.");
        let vocab = Vocabulary::new([optional]).unwrap();
        let empty = TagSet::new(&vocab, []).unwrap();

        assert!(empty.is_empty());
        assert_eq!(empty.effective().count(), 0);
    }

    #[rstest]
    #[case::unknown_tag(
        &[JJ, OPINIONATED, Tag { label: "other", ..JJ }],
        &["`other` is not a tag of the vocabulary"]
    )]
    #[case::unregistered_tag_sharing_alias_label_is_unknown_not_redefined(
        &[JJ, OPINIONATED, Tag { label: "version-control", ..VCS }],
        &["`version-control` is not a tag of the vocabulary"]
    )]
    #[case::redefined_tag(
        &[Tag { description: "Other.", ..JJ }, OPINIONATED],
        &[
            "`jj` differs from the vocabulary's definition",
            "carries no tag of group `Topic`"
        ]
    )]
    #[case::repeated_tag(
        &[JJ, OPINIONATED, JJ, JJ],
        &["`jj` is listed more than once"]
    )]
    #[case::redundant_ancestor(
        &[VCS, JJ, OPINIONATED],
        &["remove `vcs`, it is implied by `jj`"]
    )]
    #[case::missing_at_least_one(
        &[OPINIONATED],
        &["carries no tag of group `Topic`"]
    )]
    #[case::missing_exactly_one(
        &[JJ],
        &["carries no tag of group `Consensus`"]
    )]
    #[case::exclusive_conflict(
        &[JJ, OPINIONATED, UNOPINIONATED],
        &["carries both `opinionated` and `unopinionated`, which group `Consensus` allows only one of"]
    )]
    fn invalid_tag_sets_report_every_violation(#[case] tags: &[Tag], #[case] expected: &[&str]) {
        let vocab = vocabulary();
        let errors = TagSet::new(&vocab, tags.iter().copied()).unwrap_err();
        let messages: Vec<String> = errors.iter().map(ToString::to_string).collect();
        assert_eq!(messages, expected);
    }
}
