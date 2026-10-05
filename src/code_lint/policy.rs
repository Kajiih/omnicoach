//! Lint policy shared by several consumers: the suggestions code rules make from AST facts, and
//! the literals `repeated-literal` and the `rule_test!` harness treat as not worth naming.

architecture_component!(CodeLintPolicy);

use crate::code_lint::ast::LiteralValue;
use crate::code_lint::ast::python::{CollectionShape, PythonCollectionType};

/// True for literal values not worth naming.
///
/// These are strings shorter than 2 characters or without an alphanumeric character (a `\` and
/// the character after it count as one non-alphanumeric character); integers -1, 0, 1, 2; floats
/// -1.0, 0.0, 1.0, 2.0.
#[must_use]
pub fn is_trivial_literal(value: &LiteralValue) -> bool {
    match value {
        LiteralValue::Str(content) | LiteralValue::Bytes(content) => {
            let mut units = 0_usize;
            let mut has_alphanumeric = false;
            let mut characters = content.chars();
            while let Some(character) = characters.next() {
                units += 1;
                if character == '\\' {
                    characters.next();
                } else if character.is_alphanumeric() {
                    has_alphanumeric = true;
                }
            }
            units < 2 || !has_alphanumeric
        }
        LiteralValue::Int(value) => (-1..=2).contains(value),
        LiteralValue::Float(bits) => [-1.0, 0.0, 1.0, 2.0].contains(&f64::from_bits(*bits)),
    }
}

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

    #[rstest::rstest]
    #[case::single_char(LiteralValue::Str("a".to_string()), true)]
    #[case::delimiter(LiteralValue::Str(", ".to_string()), true)]
    #[case::escaped_newline(LiteralValue::Str("\\n".to_string()), true)]
    #[case::escaped_crlf(LiteralValue::Bytes("\\r\\n".to_string()), true)]
    #[case::short_word(LiteralValue::Str("jj".to_string()), false)]
    #[case::word_with_escape(LiteralValue::Str("a\\n".to_string()), false)]
    #[case::small_integer(LiteralValue::Int(-1), true)]
    #[case::integer_two(LiteralValue::Int(2), true)]
    #[case::integer_minus_two(LiteralValue::Int(-2), false)]
    #[case::integer_three(LiteralValue::Int(3), false)]
    #[case::unit_float(LiteralValue::Float((-1.0_f64).to_bits()), true)]
    #[case::float_two(LiteralValue::Float(2.0_f64.to_bits()), true)]
    #[case::half_float(LiteralValue::Float(0.5_f64.to_bits()), false)]
    #[case::raw_regex_class(LiteralValue::Str("\\\\s+".to_string()), false)]
    fn test_is_trivial_literal(#[case] value: LiteralValue, #[case] expected: bool) {
        assert_eq!(is_trivial_literal(&value), expected);
    }
}
