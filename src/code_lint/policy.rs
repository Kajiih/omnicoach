//! Lint policy shared by several code rules: the suggestions they make from AST facts.

architecture_component!(CodeLintPolicy);

use crate::code_lint::ast::python::{CollectionShape, PythonCollectionType};

/// The read-only `collections.abc` counterparts of `collection_types`, deduplicated in order and
/// joined with `", "`.
#[must_use]
pub fn read_only_collection_replacements(collection_types: &[PythonCollectionType]) -> String {
    replacements_by_shape(collection_types, |shape| match shape {
        CollectionShape::Mapping => "collections.abc.Mapping",
        CollectionShape::Set => "collections.abc.Set",
        CollectionShape::Sequence | CollectionShape::Iterable => "collections.abc.Sequence",
    })
}

/// The `replacement` of each shape in `collection_types`, deduplicated in order and joined with
/// `", "`.
#[must_use]
pub fn replacements_by_shape(
    collection_types: &[PythonCollectionType],
    replacement: impl Fn(CollectionShape) -> &'static str,
) -> String {
    let mut replacements: Vec<&str> = Vec::new();
    for collection_type in collection_types {
        let replacement = replacement(collection_type.shape);
        if !replacements.contains(&replacement) {
            replacements.push(replacement);
        }
    }
    replacements.join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::code_lint::ast::python::CollectionKind;

    fn collection_type(
        path: &str,
        kind: CollectionKind,
        shape: CollectionShape,
    ) -> PythonCollectionType {
        PythonCollectionType {
            path: path.to_owned(),
            name: path.rsplit('.').next().unwrap_or(path).to_owned(),
            kind,
            shape,
        }
    }

    #[test]
    fn test_read_only_collection_replacements() {
        use CollectionKind::{AbstractMutable, ConcreteMutable};
        use CollectionShape::{Mapping, Sequence, Set};
        let cases = [
            (
                vec![collection_type("list", ConcreteMutable, Sequence)],
                "collections.abc.Sequence",
            ),
            (
                vec![collection_type(
                    "collections.deque",
                    ConcreteMutable,
                    Sequence,
                )],
                "collections.abc.Sequence",
            ),
            (
                vec![collection_type("typing.Dict", ConcreteMutable, Mapping)],
                "collections.abc.Mapping",
            ),
            (
                vec![collection_type(
                    "collections.Counter",
                    ConcreteMutable,
                    Mapping,
                )],
                "collections.abc.Mapping",
            ),
            (
                vec![collection_type("MutableSet", AbstractMutable, Set)],
                "collections.abc.Set",
            ),
            (
                vec![
                    collection_type("dict", ConcreteMutable, Mapping),
                    collection_type("list", ConcreteMutable, Sequence),
                    collection_type("OrderedDict", ConcreteMutable, Mapping),
                ],
                "collections.abc.Mapping, collections.abc.Sequence",
            ),
        ];
        for (collection_types, expected) in cases {
            assert_eq!(
                read_only_collection_replacements(&collection_types),
                expected,
                "{collection_types:?}"
            );
        }
    }
}
