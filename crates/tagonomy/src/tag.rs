//! Compile-time tag and tag-group definitions.

/// How many tags from one [`TagGroup`] an item may carry, counting both explicit tags and broader
/// tags they imply.
///
/// For exclusive groups ([`Self::AtMostOne`] and [`Self::ExactlyOne`]), an item may still carry a
/// single chain of tags where each implies the next. For example, if `linux` implies `unix` inside
/// an `AtMostOne` `OS` group, carrying `linux` (which implies `unix`) is allowed, whereas carrying
/// both `linux` and `windows` is rejected.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Cardinality {
    /// Any number, including none.
    Any,
    /// Zero or one tag (a chain where each implies the next, such as `linux -> unix`, counts as
    /// one).
    AtMostOne,
    /// One or more tags.
    AtLeastOne,
    /// Exactly one tag (or one implication chain): combines [`Self::AtLeastOne`] and
    /// [`Self::AtMostOne`].
    ExactlyOne,
}

impl Cardinality {
    /// Whether an item may carry at most one tag (or implication chain) from the group.
    #[must_use]
    pub const fn is_exclusive(self) -> bool {
        matches!(self, Self::AtMostOne | Self::ExactlyOne)
    }

    /// Whether every item must carry at least one of the group's tags.
    #[must_use]
    pub const fn is_required(self) -> bool {
        matches!(self, Self::AtLeastOne | Self::ExactlyOne)
    }
}

/// A named category of related tags (such as `"OS"` or `"Speed"`) with a rule for how many tags
/// from it an item may carry.
///
/// Groups organize tags and enforce cardinality; they are never attached to items or used as
/// selectors themselves.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TagGroup {
    /// The group's display name, e.g. `Consensus`. Never a tag or selector.
    pub name: &'static str,
    /// How many of the group's tags an item may carry.
    pub cardinality: Cardinality,
}

/// A named tag that items can carry.
///
/// A tag can imply broader tags via [`Tag::implies`], transitively: if `ebpf` implies `linux` and
/// `linux` implies `unix`, then any item tagged `ebpf` automatically carries `linux` and `unix`
/// too. A tag may imply more than one broader tag (for example, `ebpf` implying both `linux` and
/// `kernel`).
///
/// Implication cycles fail to compile because tags are plain `const` values:
///
/// ```compile_fail,E0391
/// # use tagonomy::{Cardinality, Tag, TagGroup};
/// const GROUP: TagGroup = TagGroup { name: "Group", cardinality: Cardinality::Any };
/// const FIRST: Tag = Tag::new(&GROUP, "first", "First.").implies(&[SECOND]);
/// const SECOND: Tag = Tag::new(&GROUP, "second", "Second.").implies(&[FIRST]);
/// # let _ = (FIRST, SECOND);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Tag {
    /// The canonical label: what users type and what is displayed.
    pub label: &'static str,
    /// The group the tag belongs to.
    pub group: &'static TagGroup,
    /// The broader tags this tag directly implies.
    pub implies: &'static [Self],
    /// Other labels that resolve to this tag. Only the canonical label is displayed.
    pub aliases: &'static [&'static str],
    /// One-line description.
    pub description: &'static str,
    /// What belongs under the tag and what does not.
    pub scope_note: Option<&'static str>,
}

impl Tag {
    /// Creates a tag in `group` with no implied tags, aliases, or scope note.
    #[must_use]
    pub const fn new(
        group: &'static TagGroup,
        label: &'static str,
        description: &'static str,
    ) -> Self {
        Self {
            label,
            group,
            implies: &[],
            aliases: &[],
            description,
            scope_note: None,
        }
    }

    /// Sets the broader tags this tag directly implies.
    #[must_use]
    pub const fn implies(mut self, implies: &'static [Self]) -> Self {
        self.implies = implies;
        self
    }

    /// Sets alternative labels that resolve to this tag (for example, `"bpf"` for `"ebpf"`).
    #[must_use]
    pub const fn aliases(mut self, aliases: &'static [&'static str]) -> Self {
        self.aliases = aliases;
        self
    }

    /// Sets a note explaining what belongs under this tag and what does not.
    #[must_use]
    pub const fn scope_note(mut self, scope_note: &'static str) -> Self {
        self.scope_note = Some(scope_note);
        self
    }

    /// Whether the tag belongs to `group`.
    #[must_use]
    pub fn belongs_to(&self, group: &TagGroup) -> bool {
        self.group == group
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    const TOPIC: TagGroup = TagGroup {
        name: "Topic",
        cardinality: Cardinality::AtLeastOne,
    };
    const VCS: Tag = Tag::new(&TOPIC, "vcs", "Version control.")
        .aliases(&["version-control"])
        .scope_note("Generic VCS.");
    const JJ: Tag = Tag::new(&TOPIC, "jj", "Jujutsu.").implies(&[VCS]);

    #[rstest]
    #[case::identical("Topic", Cardinality::AtLeastOne, true)]
    #[case::same_name_other_cardinality("Topic", Cardinality::Any, false)]
    #[case::other_length("Consensus", Cardinality::AtLeastOne, false)]
    #[case::same_length_other_text("Topix", Cardinality::AtLeastOne, false)]
    fn belongs_to_matches_group_equality(
        #[case] name: &'static str,
        #[case] cardinality: Cardinality,
        #[case] expected: bool,
    ) {
        let group = TagGroup { name, cardinality };
        assert_eq!(JJ.belongs_to(&group), expected);
    }

    #[rstest]
    #[case::any(Cardinality::Any, false, false)]
    #[case::at_most_one(Cardinality::AtMostOne, true, false)]
    #[case::at_least_one(Cardinality::AtLeastOne, false, true)]
    #[case::exactly_one(Cardinality::ExactlyOne, true, true)]
    fn cardinality_predicates_classify_axes(
        #[case] cardinality: Cardinality,
        #[case] exclusive: bool,
        #[case] required: bool,
    ) {
        assert_eq!(cardinality.is_exclusive(), exclusive);
        assert_eq!(cardinality.is_required(), required);
    }

    #[test]
    fn builders_set_their_fields() {
        let built = Tag::new(&TOPIC, "jj", "Jujutsu.")
            .implies(&[VCS])
            .aliases(&["jujutsu"])
            .scope_note("Jujutsu only.");
        assert_eq!(
            built,
            Tag {
                label: "jj",
                group: &TOPIC,
                implies: &[VCS],
                aliases: &["jujutsu"],
                description: "Jujutsu.",
                scope_note: Some("Jujutsu only."),
            }
        );
    }
}
