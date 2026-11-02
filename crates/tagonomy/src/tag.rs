//! Tag definitions: the `const` tags and tag groups a consumer declares.

/// How many tags of one [`TagGroup`] an item may carry, counted on its effective tags (its
/// explicit tags and everything they imply).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Cardinality {
    /// Any number, including none.
    Any,
    /// At most one: the group's tags on an item form a chain (each implies the next), so a
    /// hierarchical group such as `unix > linux` stays exclusive against `windows`.
    AtMostOne,
    /// At least one.
    AtLeastOne,
    /// Exactly one chain: [`Self::AtLeastOne`] and [`Self::AtMostOne`].
    ExactlyOne,
}

impl Cardinality {
    /// Whether an item may carry at most one chain of the group's tags.
    pub(crate) const fn is_exclusive(self) -> bool {
        matches!(self, Self::AtMostOne | Self::ExactlyOne)
    }

    /// Whether every item must carry at least one of the group's tags.
    pub(crate) const fn is_required(self) -> bool {
        matches!(self, Self::AtLeastOne | Self::ExactlyOne)
    }
}

/// A named set of tags that answer the same question (a facet or axis), with a cardinality
/// rule. Groups are identified by name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TagGroup {
    /// The group's display name, e.g. `Precision`. Never a selector.
    pub name: &'static str,
    /// How many of the group's tags an item may carry.
    pub cardinality: Cardinality,
}

/// A tag: a named, documented predicate that items carry. Tags are identified by label.
///
/// A tag implies every tag in `implies`, transitively: an item carrying it carries them too.
/// A cycle through `implies` fails to compile:
///
/// ```compile_fail,E0391
/// # use tagonomy::{Cardinality, Tag, TagGroup};
/// const GROUP: TagGroup = TagGroup { name: "Group", cardinality: Cardinality::Any };
/// const A: Tag = Tag::new(&GROUP, "a", "A.").implies(&[B]);
/// const B: Tag = Tag::new(&GROUP, "b", "B.").implies(&[A]);
/// # let _ = (A, B);
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
    /// A root tag in `group` with no implied tags, aliases or scope note.
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

    /// Sets alternative labels that resolve to this tag.
    #[must_use]
    pub const fn aliases(mut self, aliases: &'static [&'static str]) -> Self {
        self.aliases = aliases;
        self
    }

    /// Sets the normative boundary note describing what belongs under the tag and what does not.
    #[must_use]
    pub const fn scope_note(mut self, scope_note: &'static str) -> Self {
        self.scope_note = Some(scope_note);
        self
    }

    /// Whether the tag belongs to `group`.
    #[must_use]
    pub fn belongs_to(&self, group: &TagGroup) -> bool {
        self.group.name == group.name
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

    /// `jj` belongs to `Topic`.
    #[rstest]
    #[case::same_name_other_cardinality("Topic", true)]
    #[case::other_length("Precision", false)]
    #[case::same_length_other_text("Topix", false)]
    fn a_tag_belongs_to_the_group_of_the_same_name(
        #[case] name: &'static str,
        #[case] expected: bool,
    ) {
        let group = TagGroup {
            name,
            cardinality: Cardinality::Any,
        };
        assert_eq!(JJ.belongs_to(&group), expected);
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
