//! Include/exclude tag rules where narrower tags override broader ones.

use std::collections::BTreeSet;

use fixedbitset::{Block, FixedBitSet};

use crate::expr::Expr;
use crate::tag_set::TagSet;
use crate::vocabulary::{TagId, Vocabulary};

/// What a directive, or a selection's default, decides for an item.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Verdict {
    /// Keep the item.
    Include,
    /// Drop the item.
    Exclude,
}

/// A tag given both [`Verdict::Include`] and [`Verdict::Exclude`], which [`TagSelection::new`]
/// and [`TagSelection::override_with`] reject.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TagConflict {
    /// The conflicting tag.
    pub tag: TagId,
}

/// A [`TagSelection`]'s verdict on a [`TagSet`], and why.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TagDecision {
    /// The verdict.
    pub verdict: Verdict,
    /// What decided it.
    pub reason: TagReason,
}

/// What decided a [`TagDecision`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TagReason {
    /// No directive matched any tag of the set: the selection's default.
    Default,
    /// The directive on `tag`, applied through the item's explicit tag `source`.
    /// [`Vocabulary::chain`] lists the implications from `tag` down to `source`, and
    /// [`Vocabulary::chain_through`] prepends the root chain above `tag`.
    Tag {
        /// The directive's tag.
        tag: TagId,
        /// The item's explicit tag that implies `tag` (`tag` itself when explicit).
        source: TagId,
    },
}

/// Include/exclude rules over a [`Vocabulary`] where narrower tags override broader ones.
///
/// For example, if `ebpf` implies `linux` and `linux` implies `os`:
///
/// - Excluding `os` and including `linux` drops other `os` items while keeping `linux` and `ebpf`.
/// - Each explicit tag on an item is checked independently; if one branch resolves to
///   [`Verdict::Include`] and another resolves to [`Verdict::Exclude`], [`Verdict::Exclude`] wins.
/// - If no rule matches any of the item's tags, the selection falls back to its default verdict.
#[derive(Debug, Clone)]
pub struct TagSelection<'a> {
    vocabulary: &'a Vocabulary,
    default: Verdict,
    tagged: FixedBitSet,
    excluded: FixedBitSet,
}

impl<'a> TagSelection<'a> {
    /// Builds a tag selection over `vocabulary` deciding `default` unless overridden by
    /// `directives`.
    ///
    /// # Errors
    ///
    /// Returns every [`TagConflict`], ordered by [`TagId`], if any tag is given both
    /// verdicts.
    pub fn new(
        vocabulary: &'a Vocabulary,
        default: Verdict,
        directives: impl IntoIterator<Item = (TagId, Verdict)>,
    ) -> Result<Self, Vec<TagConflict>> {
        let tag_count = vocabulary.tags().len();
        let mut included = FixedBitSet::with_capacity(tag_count);
        let mut excluded = FixedBitSet::with_capacity(tag_count);
        let mut tagged = FixedBitSet::with_capacity(tag_count);
        let mut conflicting = BTreeSet::new();

        for (tag, verdict) in directives {
            let index = tag.index();
            let (same, opposite) = match verdict {
                Verdict::Include => (&mut included, &excluded),
                Verdict::Exclude => (&mut excluded, &included),
            };
            same.insert(index);
            tagged.insert(index);
            if opposite.contains(index) {
                conflicting.insert(tag);
            }
        }
        if conflicting.is_empty() {
            Ok(Self {
                vocabulary,
                default,
                tagged,
                excluded,
            })
        } else {
            Err(conflicting
                .into_iter()
                .map(|tag| TagConflict { tag })
                .collect())
        }
    }

    /// Builds a new tag selection by applying `directives` on top of `self`.
    ///
    /// Keeps [`Self::default_verdict`] and every directive from `self` not mentioned in
    /// `directives`; any tag in `directives` replaces only its own previous verdict in `self`
    /// without removing directives on narrower or broader tags (normal specificity precedence
    /// still applies to the merged selection). Repeating the same verdict for a tag within
    /// `directives` is allowed.
    ///
    /// # Errors
    ///
    /// Returns every [`TagConflict`], ordered by [`TagId`], if `directives` itself gives the same
    /// tag both verdicts.
    pub fn override_with(
        &self,
        directives: impl IntoIterator<Item = (TagId, Verdict)>,
    ) -> Result<Self, Vec<TagConflict>> {
        let layer = Self::new(self.vocabulary, self.default, directives)?;
        Ok(self.merge_layer(&layer))
    }

    /// The selection's verdict on `tags`, and why.
    #[must_use]
    pub fn decide(&self, tags: &TagSet) -> TagDecision {
        self.decide_skipping(tags, None)
    }

    /// Whether `self` includes `tags` (`self.decide(tags).verdict == Verdict::Include`).
    #[must_use]
    pub fn matches(&self, tags: &TagSet) -> bool {
        self.decide(tags).verdict == Verdict::Include
    }

    /// Converts this selection into an equivalent boolean [`Expr`].
    ///
    /// For every valid [`TagSet`] under `vocabulary`, `self.to_expr().matches(&tags)` returns the
    /// exact same result as `self.matches(&tags)`. This lets you combine a config-based selection
    /// with an ad-hoc CLI filter (`selection.to_expr() & cli_query`).
    #[must_use]
    pub fn to_expr(&self) -> Expr {
        let mut include_sources = Vec::new();
        let mut exclude_sources = Vec::new();

        for source in self.vocabulary.tags() {
            let closure = self.vocabulary.closure(source);
            let mut has_include = false;
            let mut has_exclude = false;
            for index in self.tagged.intersection(closure) {
                let tag = TagId::new(index);
                if intersects_both_except(
                    &self.tagged,
                    closure,
                    self.vocabulary.strict_narrower(tag),
                    None,
                ) {
                    continue;
                }
                if self.excluded.contains(index) {
                    has_exclude = true;
                    break;
                }
                has_include = true;
            }
            if has_exclude {
                exclude_sources.push(Expr::ExplicitTag(source));
            } else if has_include {
                include_sources.push(Expr::ExplicitTag(source));
            }
        }

        let not_excluded = !Expr::any(exclude_sources);
        match self.default {
            Verdict::Include => not_excluded,
            Verdict::Exclude => {
                if include_sources.is_empty() {
                    Expr::None
                } else {
                    Expr::any(include_sources) & not_excluded
                }
            }
        }
    }

    /// The selection's default verdict when no directive matches.
    #[must_use]
    pub const fn default_verdict(&self) -> Verdict {
        self.default
    }

    /// Combines `self` with a validated override `layer` built on the same vocabulary.
    pub(crate) fn merge_layer(&self, layer: &Self) -> Self {
        let mut tagged = self.tagged.clone();
        tagged.union_with(&layer.tagged);
        let mut excluded = self.excluded.clone();
        excluded.difference_with(&layer.tagged);
        excluded.union_with(&layer.excluded);
        Self {
            vocabulary: self.vocabulary,
            default: self.default,
            tagged,
            excluded,
        }
    }

    /// Every tag carrying a directive in this selection paired with its [`Verdict`], in
    /// vocabulary order.
    pub(crate) fn directives(&self) -> impl Iterator<Item = (TagId, Verdict)> + '_ {
        self.tagged.ones().map(|index| {
            let verdict = if self.excluded.contains(index) {
                Verdict::Exclude
            } else {
                Verdict::Include
            };
            (TagId::new(index), verdict)
        })
    }

    /// Evaluates `tags` while ignoring `skipped` as if it had never been passed to [`Self::new`].
    pub(crate) fn decide_skipping(&self, tags: &TagSet, skipped: Option<TagId>) -> TagDecision {
        let skipped_index = skipped.map(TagId::index);
        let mut first_include = None;
        for &source in tags.explicit() {
            let closure = self.vocabulary.closure(source);
            for index in self.tagged.intersection(closure) {
                if Some(index) == skipped_index {
                    continue;
                }
                let tag = TagId::new(index);
                if intersects_both_except(
                    &self.tagged,
                    closure,
                    self.vocabulary.strict_narrower(tag),
                    skipped_index,
                ) {
                    continue;
                }
                let reason = TagReason::Tag { tag, source };
                if self.excluded.contains(index) {
                    return TagDecision {
                        verdict: Verdict::Exclude,
                        reason,
                    };
                }
                first_include.get_or_insert(TagDecision {
                    verdict: Verdict::Include,
                    reason,
                });
            }
        }
        first_include.unwrap_or(TagDecision {
            verdict: self.default,
            reason: TagReason::Default,
        })
    }
}

impl From<&TagSelection<'_>> for Expr {
    fn from(selection: &TagSelection<'_>) -> Self {
        selection.to_expr()
    }
}

impl From<TagSelection<'_>> for Expr {
    fn from(selection: TagSelection<'_>) -> Self {
        selection.to_expr()
    }
}

/// Whether any bit other than `skipped` belongs to `base`, `first`, and `second`.
fn intersects_both_except(
    base: &FixedBitSet,
    first: &FixedBitSet,
    second: &FixedBitSet,
    skipped: Option<usize>,
) -> bool {
    let block_bits = Block::BITS as usize;
    let skipped_block = skipped.map(|index| (index / block_bits, index % block_bits));
    base.as_slice()
        .iter()
        .zip(first.as_slice())
        .zip(second.as_slice())
        .enumerate()
        .any(|(block_index, ((&word, &left), &right))| {
            let mut mask = word & left & right;
            if let Some((target_block, bit_offset)) = skipped_block
                && block_index == target_block
            {
                mask &= !(1 << bit_offset);
            }
            mask != 0
        })
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;
    use crate::tag::{Cardinality, Tag, TagGroup};

    const TOPIC: TagGroup = TagGroup {
        name: "Topic",
        cardinality: Cardinality::Any,
    };
    const OS: Tag = Tag::new(&TOPIC, "os", "OS.");
    const LINUX: Tag = Tag::new(&TOPIC, "linux", "Linux.").implies(&[OS]);
    const KERNEL: Tag = Tag::new(&TOPIC, "kernel", "Kernel.").implies(&[OS]);
    const EBPF: Tag = Tag::new(&TOPIC, "ebpf", "eBPF.").implies(&[LINUX, KERNEL]);
    const SIBLING: Tag = Tag::new(&TOPIC, "sibling", "Sibling.").implies(&[OS]);
    const OTHER: Tag = Tag::new(&TOPIC, "other", "Other.");

    fn vocabulary() -> Vocabulary {
        Vocabulary::new([EBPF, SIBLING, OTHER]).unwrap()
    }

    #[rstest]
    #[case::default_include(Verdict::Include, &[], &[])]
    #[case::default_exclude(Verdict::Exclude, &[], &[])]
    #[case::include_only_under_default_exclude(Verdict::Exclude, &["linux"], &[])]
    #[case::parent_include_child_exclude(Verdict::Exclude, &["os"], &["linux"])]
    #[case::parent_exclude_child_include(Verdict::Include, &["linux"], &["os"])]
    #[case::parent_exclude_child_include_default_exclude(Verdict::Exclude, &["linux"], &["os"])]
    #[case::diamond_tie_exclude_wins(Verdict::Exclude, &["linux"], &["kernel"])]
    fn to_expr_matches_tag_selection_across_all_tag_sets(
        #[case] default: Verdict,
        #[case] include: &[&str],
        #[case] exclude: &[&str],
    ) {
        let vocab = vocabulary();
        let directives = include
            .iter()
            .map(|label| (vocab.id(label).unwrap(), Verdict::Include))
            .chain(
                exclude
                    .iter()
                    .map(|label| (vocab.id(label).unwrap(), Verdict::Exclude)),
            );
        let selection = TagSelection::new(&vocab, default, directives).unwrap();
        let lowered = Expr::from(&selection);
        let owned_lowered = Expr::from(selection.clone());

        let samples: &[&[Tag]] = &[
            &[],
            &[OS],
            &[LINUX],
            &[SIBLING],
            &[EBPF],
            &[OTHER],
            &[LINUX, SIBLING],
            &[EBPF, OTHER],
        ];
        let attached: Vec<TagSet> = samples
            .iter()
            .map(|raw| TagSet::new(&vocab, raw.iter().copied()).unwrap())
            .collect();
        let lowered_verdicts: Vec<bool> =
            attached.iter().map(|tags| lowered.matches(tags)).collect();
        let selection_verdicts: Vec<bool> = attached
            .iter()
            .map(|tags| selection.matches(tags))
            .collect();
        assert_eq!(owned_lowered, lowered);
        assert_eq!(lowered_verdicts, selection_verdicts);
    }

    #[test]
    fn conflicting_tag_directives_are_rejected_in_tag_order() {
        let vocab = vocabulary();
        let [os, linux] = ["os", "linux"].map(|label| vocab.id(label).unwrap());
        let conflicts = TagSelection::new(
            &vocab,
            Verdict::Include,
            [
                (os, Verdict::Include),
                (linux, Verdict::Include),
                (os, Verdict::Exclude),
                (linux, Verdict::Exclude),
            ],
        )
        .unwrap_err();
        let mut expected = vec![TagConflict { tag: os }, TagConflict { tag: linux }];
        expected.sort_by_key(|conflict| conflict.tag);
        assert_eq!(conflicts, expected);
    }

    #[rstest]
    #[case::include_to_exclude(
        Verdict::Exclude,
        &[("linux", Verdict::Include), ("other", Verdict::Include)],
        &[("linux", Verdict::Exclude)],
        &[(&[LINUX][..], false), (&[OTHER][..], true), (&[SIBLING][..], false)],
    )]
    #[case::exclude_to_include_under_default_exclude(
        Verdict::Exclude,
        &[("linux", Verdict::Exclude), ("other", Verdict::Include)],
        &[("linux", Verdict::Include), ("linux", Verdict::Include)],
        &[(&[LINUX][..], true), (&[OTHER][..], true), (&[SIBLING][..], false)],
    )]
    #[case::broader_override_keeps_narrower_directive(
        Verdict::Include,
        &[("os", Verdict::Include), ("linux", Verdict::Include)],
        &[("os", Verdict::Exclude)],
        &[(&[LINUX][..], true), (&[SIBLING][..], false), (&[OTHER][..], true)],
    )]
    #[case::adds_new_include_tag_absent_from_base(
        Verdict::Exclude,
        &[("other", Verdict::Include)],
        &[("linux", Verdict::Include)],
        &[(&[LINUX][..], true), (&[OTHER][..], true), (&[SIBLING][..], false)],
    )]
    #[case::adds_new_narrower_exclude_tag_absent_from_base(
        Verdict::Include,
        &[("os", Verdict::Include)],
        &[("linux", Verdict::Exclude)],
        &[(&[LINUX][..], false), (&[SIBLING][..], true), (&[OTHER][..], true)],
    )]
    fn override_with_replaces_only_overridden_tags_and_matches_to_expr(
        #[case] default: Verdict,
        #[case] base_directives: &[(&str, Verdict)],
        #[case] override_directives: &[(&str, Verdict)],
        #[case] expected: &[(&[Tag], bool)],
    ) {
        let vocab = vocabulary();
        let resolve = |pairs: &[(&str, Verdict)]| {
            pairs
                .iter()
                .map(|&(label, verdict)| (vocab.id(label).unwrap(), verdict))
                .collect::<Vec<_>>()
        };
        let base = TagSelection::new(&vocab, default, resolve(base_directives)).unwrap();
        let overridden = base.override_with(resolve(override_directives)).unwrap();
        let lowered = overridden.to_expr();
        let actual: Vec<(bool, bool)> = expected
            .iter()
            .map(|&(tags, _)| {
                let set = TagSet::new(&vocab, tags.iter().copied()).unwrap();
                (overridden.matches(&set), lowered.matches(&set))
            })
            .collect();
        let expected_pairs: Vec<(bool, bool)> =
            expected.iter().map(|&(_, want)| (want, want)).collect();
        assert_eq!(actual, expected_pairs);
        assert_eq!(overridden.default_verdict(), default);
    }

    #[test]
    fn override_with_keeps_base_unmutated() {
        let vocab = vocabulary();
        let linux = vocab.id("linux").unwrap();
        let base =
            TagSelection::new(&vocab, Verdict::Exclude, [(linux, Verdict::Include)]).unwrap();
        let linux_tags = TagSet::new(&vocab, [LINUX]).unwrap();

        let overridden = base.override_with([(linux, Verdict::Exclude)]).unwrap();

        assert!(base.matches(&linux_tags));
        assert!(!overridden.matches(&linux_tags));
    }

    #[test]
    fn override_with_rejects_intra_layer_conflicts_in_tag_order() {
        let vocab = vocabulary();
        let [os, linux] = ["os", "linux"].map(|label| vocab.id(label).unwrap());
        let base =
            TagSelection::new(&vocab, Verdict::Exclude, [(linux, Verdict::Include)]).unwrap();

        let conflicts = base
            .override_with([
                (os, Verdict::Include),
                (linux, Verdict::Include),
                (os, Verdict::Exclude),
                (linux, Verdict::Exclude),
            ])
            .unwrap_err();
        let mut expected = vec![TagConflict { tag: os }, TagConflict { tag: linux }];
        expected.sort_by_key(|conflict| conflict.tag);
        assert_eq!(conflicts, expected);
    }

    #[rstest]
    #[case::shared_in_later_block(&[105], &[105, 115], None, true)]
    #[case::masked_skipped_bit(&[105], &[105, 115], Some(105), false)]
    #[case::skipped_in_other_block_keeps_hit(&[105], &[105, 115], Some(1), true)]
    #[case::pairwise_without_common_triple(&[1], &[105], None, false)]
    fn intersects_both_except_handles_multi_block_bitsets(
        #[case] first_bits: &[usize],
        #[case] second_bits: &[usize],
        #[case] skipped: Option<usize>,
        #[case] expected: bool,
    ) {
        let make = |indices: &[usize]| {
            let mut set = FixedBitSet::with_capacity(130);
            for &index in indices {
                set.insert(index);
            }
            set
        };
        assert_eq!(
            intersects_both_except(
                &make(&[1, 105]),
                &make(first_bits),
                &make(second_bits),
                skipped
            ),
            expected
        );
    }
}
