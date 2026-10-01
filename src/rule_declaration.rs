//! Rule declaration: the single item each rule exposes.
//!
//! Pure, constant data bundling a rule's detector with its classification, doc and options.

architecture_component!(RuleDeclaration);

use crate::core::RuleOptions;
use crate::rule_documentation::RuleDoc;
use crate::rule_taxonomy::Classification;

/// A rule: the detector that finds violations, its classification, its doc and its options.
/// A rule cannot be registered without all four.
#[derive(Clone, Copy)]
pub struct Rule<DetectorType: ?Sized + 'static> {
    /// The detector that finds violations.
    pub detector: &'static DetectorType,
    /// Its classification.
    pub classification: Classification,
    /// Its user-facing doc.
    pub doc: RuleDoc,
    /// Everything it accepts under `[rules.<name>]`.
    pub options: RuleOptions,
}
