//! Lint policy shared across rules and the test harness (`CodeLintPolicy`).
//!
//! Boundary: `code_lint::ast` (`CodeLintAst`) exposes only rule-agnostic language facts (including
//! standard-library collection taxonomy and syntax positions where the language or API requires a
//! literal); single-rule policy lives in that rule's file (`CodeLintRules`); policy shared by two
//! or more rules or by a rule and `TestUtils` lives here (`CodeLintPolicy`).
//!
//! Two tests separate a fact from policy:
//! 1. **Vocabulary**: a fact is stated and named in terms of the language or standard library
//!    (`has_imposed_signature`, `is_interface_method`, `has_stub_body`, `is_documented_elsewhere`),
//!    never in terms of rules ("exempt", "skip", "allowed").
//! 2. **Rule disagreement**: when two rules can reasonably want different answers (for example,
//!    `identical-positional-types` skips `has_imposed_signature()` while collection signature
//!    rules also skip `is_interface_method`), the grouping is policy and the pieces underneath are
//!    facts.

architecture_component!(CodeLintPolicy);

use crate::code_lint::ast::LiteralValue;
use crate::code_lint::ast::python::{
    CollectionShape, PythonCollectionType, PythonFunctionSignature, is_interface_method,
};
use crate::rule_declaration::{CountOption, LanguageDefaults, rule_options};

const MIN_STATEMENTS_KEY: &str = "min-statements";
const DEFAULT_MIN_STATEMENTS: usize = 12;
const MIN_COGNITIVE_COMPLEXITY_KEY: &str = "min-cognitive-complexity";
const DEFAULT_MIN_COGNITIVE_COMPLEXITY: usize = 6;
const STATEMENT_COUNT_METRIC: &str = "statement count";
const COGNITIVE_COMPLEXITY_METRIC: &str = "cognitive complexity";

rule_options! {
    /// Declared threshold options for the Python function-complexity gate shared by docstring
    /// rules (`min-statements` and `min-cognitive-complexity`).
    pub struct ComplexityGateOptions =>
    /// Resolved statement-count and cognitive-complexity thresholds for the Python complexity gate.
    pub struct ComplexityGate {
        /// Minimum statement count; `0` disables this trigger.
        pub(super) min_statements: CountOption,
        /// Minimum cognitive complexity; `0` disables this trigger.
        pub(super) min_cognitive_complexity: CountOption,
    }
}

impl ComplexityGateOptions {
    /// Declares the `min-statements` [`CountOption`] (default `12`) with `doc`.
    #[must_use]
    pub(super) const fn min_statements(doc: &'static str) -> CountOption {
        CountOption {
            key: MIN_STATEMENTS_KEY,
            doc,
            default: LanguageDefaults::new(DEFAULT_MIN_STATEMENTS, &[]),
        }
    }

    /// Declares the `min-cognitive-complexity` [`CountOption`] (default `6`) with `doc`.
    #[must_use]
    pub(super) const fn min_cognitive_complexity(doc: &'static str) -> CountOption {
        CountOption {
            key: MIN_COGNITIVE_COMPLEXITY_KEY,
            doc,
            default: LanguageDefaults::new(DEFAULT_MIN_COGNITIVE_COMPLEXITY, &[]),
        }
    }
}

/// A complexity threshold reached by a Python function, paired with the function's measured value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ComplexityMetric {
    /// The function's statement count ([`PythonFunctionSignature::statement_count`]).
    StatementCount(usize),
    /// The function's cognitive complexity ([`PythonFunctionSignature::cognitive_complexity`]).
    CognitiveComplexity(usize),
}

impl ComplexityMetric {
    /// The metric's display name (`"statement count"` or `"cognitive complexity"`).
    #[must_use]
    pub(super) const fn name(self) -> &'static str {
        match self {
            Self::StatementCount(_) => STATEMENT_COUNT_METRIC,
            Self::CognitiveComplexity(_) => COGNITIVE_COMPLEXITY_METRIC,
        }
    }

    /// The function's measured value for the metric.
    #[must_use]
    pub(super) const fn value(self) -> usize {
        match self {
            Self::StatementCount(value) | Self::CognitiveComplexity(value) => value,
        }
    }
}

impl ComplexityGate {
    /// Returns the first complexity threshold reached by `signature` (statement count first, then
    /// cognitive complexity), or `None` if neither threshold is reached. A threshold of `0`
    /// disables that trigger.
    #[must_use]
    pub(super) fn triggered_metric(
        self,
        signature: &PythonFunctionSignature<'_>,
    ) -> Option<ComplexityMetric> {
        let statements = signature.statement_count();
        if self.min_statements > 0 && statements >= self.min_statements {
            return Some(ComplexityMetric::StatementCount(statements));
        }
        let complexity = signature.cognitive_complexity();
        (self.min_cognitive_complexity > 0 && complexity >= self.min_cognitive_complexity)
            .then_some(ComplexityMetric::CognitiveComplexity(complexity))
    }

    /// Returns true if `signature` reaches `min_statements` or `min_cognitive_complexity` (`0`
    /// disables a trigger).
    #[must_use]
    pub(super) fn is_nontrivial(self, signature: &PythonFunctionSignature<'_>) -> bool {
        self.triggered_metric(signature).is_some()
    }
}

/// Returns true if the function of `signature` is exempt from signature annotation rules.
///
/// Its signature is imposed ([`PythonFunctionSignature::has_imposed_signature`]) or it is a
/// method of an interface class ([`is_interface_method`]).
#[must_use]
pub(super) fn is_exempt_from_signature_rules(signature: &PythonFunctionSignature<'_>) -> bool {
    signature.has_imposed_signature() || is_interface_method(signature)
}

/// Returns true if the function of `signature` is exempt from body-usage parameter rules.
///
/// It is exempt from signature rules ([`is_exempt_from_signature_rules`]) or its body is a stub
/// ([`PythonFunctionSignature::has_stub_body`]).
#[must_use]
pub(super) fn is_exempt_from_body_usage_rules(signature: &PythonFunctionSignature<'_>) -> bool {
    is_exempt_from_signature_rules(signature) || signature.has_stub_body()
}

/// Returns true if `name` starts with the word `Fake` after any leading `_`: `Fake` alone or
/// followed by anything but a lowercase letter, so `Faker` and `Fakeable` do not match.
#[must_use]
pub(super) fn is_fake_class_name(name: &str) -> bool {
    name.trim_start_matches('_')
        .strip_prefix("Fake")
        .is_some_and(|rest| !rest.starts_with(|character: char| character.is_ascii_lowercase()))
}

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
pub(super) fn read_only_collection_replacements(
    collection_types: &[PythonCollectionType],
) -> String {
    replacements_by_shape(collection_types, |shape| match shape {
        CollectionShape::Mapping => "collections.abc.Mapping",
        CollectionShape::Set => "collections.abc.Set",
        CollectionShape::Sequence | CollectionShape::Iterable => "collections.abc.Sequence",
    })
}

/// The `replacement` of each shape in `collection_types`, deduplicated in order and joined with
/// `", "`.
#[must_use]
pub(super) fn replacements_by_shape(
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
    use crate::code_lint::ast::ParsedFile;
    use crate::code_lint::ast::python::{CollectionKind, extract_function_signatures};
    use crate::diagnostic::Language;

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

    #[rstest::rstest]
    #[case::statements_first(
        ComplexityGate { min_statements: 2, min_cognitive_complexity: 1 },
        Some(("statement count", 3))
    )]
    #[case::statements_exact_boundary(
        ComplexityGate { min_statements: 3, min_cognitive_complexity: 5 },
        Some(("statement count", 3))
    )]
    #[case::complexity_when_statements_below(
        ComplexityGate { min_statements: 10, min_cognitive_complexity: 1 },
        Some(("cognitive complexity", 1))
    )]
    #[case::zero_disables_trigger(
        ComplexityGate { min_statements: 0, min_cognitive_complexity: 0 },
        None
    )]
    #[case::below_both(
        ComplexityGate { min_statements: 10, min_cognitive_complexity: 5 },
        None
    )]
    fn complexity_gate_orders_thresholds(
        #[case] gate: ComplexityGate,
        #[case] expected: Option<(&str, usize)>,
    ) {
        let source = indoc::indoc! {"
            def f(x):
                if x:
                    return 1
                return 0
        "};
        let file = ParsedFile::new(source, Language::Python);
        let signature = &extract_function_signatures(&file)[0];
        assert_eq!(
            gate.triggered_metric(signature)
                .map(|metric| (metric.name(), metric.value())),
            expected
        );
        assert_eq!(gate.is_nontrivial(signature), expected.is_some());
    }

    #[rstest::rstest]
    #[case::list(
        vec![collection_type("list", CollectionKind::ConcreteMutable, CollectionShape::Sequence)],
        "collections.abc.Sequence"
    )]
    #[case::deque(
        vec![collection_type("collections.deque", CollectionKind::ConcreteMutable, CollectionShape::Sequence)],
        "collections.abc.Sequence"
    )]
    #[case::typing_dict(
        vec![collection_type("typing.Dict", CollectionKind::ConcreteMutable, CollectionShape::Mapping)],
        "collections.abc.Mapping"
    )]
    #[case::counter(
        vec![collection_type("collections.Counter", CollectionKind::ConcreteMutable, CollectionShape::Mapping)],
        "collections.abc.Mapping"
    )]
    #[case::mutable_set(
        vec![collection_type("MutableSet", CollectionKind::AbstractMutable, CollectionShape::Set)],
        "collections.abc.Set"
    )]
    #[case::dedup_and_order(
        vec![
            collection_type("dict", CollectionKind::ConcreteMutable, CollectionShape::Mapping),
            collection_type("list", CollectionKind::ConcreteMutable, CollectionShape::Sequence),
            collection_type("OrderedDict", CollectionKind::ConcreteMutable, CollectionShape::Mapping),
        ],
        "collections.abc.Mapping, collections.abc.Sequence"
    )]
    fn test_read_only_collection_replacements(
        #[case] collection_types: Vec<PythonCollectionType>,
        #[case] expected: &str,
    ) {
        assert_eq!(
            read_only_collection_replacements(&collection_types),
            expected,
            "{collection_types:?}"
        );
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
