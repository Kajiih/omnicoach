//! Declarations of generic rules targeting multiple languages.

use crate::code_lint::CodeRule;
use crate::core::{FilterListDefaults, Rule, RuleName};
use crate::diagnostic::{Diagnostic, ViolationTemplate, violation_template};
use crate::rules::Tag;
use ast_grep_core::AstGrep;
use ast_grep_language::SupportLang;
use std::path::Path;

/// Static defaults for single-letter variable names.
const DEFAULT_ALLOWED: FilterListDefaults = FilterListDefaults {
    base: &["i", "j", "x", "f"],
    extend: &[(SupportLang::Rust, &["c"])],
    exempt: &[],
};

const TEMPLATE: ViolationTemplate = violation_template! {
    summary: "Variable name `{name}` is too short (single-letter).",
    rationale: "Single-letter variable names are not descriptive and make code harder to read and maintain.",
    suggestion: "Choose a more descriptive name that reflects the variable's purpose.",
};

/// Rule that bans single-letter variable names.
pub struct SingleLetterVariableName;

impl Rule for SingleLetterVariableName {
    fn name(&self) -> RuleName {
        RuleName("single-letter-variable-name")
    }

    fn tags(&self) -> &'static [Tag] {
        &[Tag::Style, Tag::Naming]
    }

    fn supported_languages(&self) -> &'static [SupportLang] {
        &[SupportLang::Python, SupportLang::Rust]
    }

    fn violation_template(&self) -> &'static ViolationTemplate {
        &TEMPLATE
    }
}
impl CodeRule for SingleLetterVariableName {
    fn check_file(
        &self,
        path: &Path,
        grep: &AstGrep<crate::code_lint::SourceDoc>,
        config: &crate::core::Config,
    ) -> Vec<Diagnostic> {
        let effective_allowed = self.effective_allowed_set(*grep.lang(), config, &DEFAULT_ALLOWED);

        let mut diagnostics = Vec::new();
        for node in crate::code_lint::collect_renameable_bindings(grep) {
            let name = node.text();
            if name.len() == 1 && name != "_" && !effective_allowed.contains(&*name) {
                diagnostics.push(self.diagnostic_at_node(path, &node, &[("name", &name)]));
            }
        }
        diagnostics
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::{assert_code_rule_snapshot, assert_code_rule_snapshot_with_config};
    use rstest::rstest;

    #[rstest]
    #[case::let_binding(
        "fn main() { let a = 1; }",
        "[single-letter-variable-name] Line 1, Col 17: Variable name `a` is too short (single-letter)."
    )]
    #[case::let_reference(
        "fn main() { let a = b; }",
        "[single-letter-variable-name] Line 1, Col 17: Variable name `a` is too short (single-letter)."
    )]
    #[case::mutable_binding(
        "fn main() { let mut b = 2; }",
        "[single-letter-variable-name] Line 1, Col 21: Variable name `b` is too short (single-letter)."
    )]
    #[case::tuple_destructuring(
        "fn main() { let (c, d) = (1, 2); }",
        "[single-letter-variable-name] Line 1, Col 21: Variable name `d` is too short (single-letter)."
    )]
    #[case::struct_destructuring_explicit(
        "fn main() { let Point { x: e, y: _ } = p; }",
        "[single-letter-variable-name] Line 1, Col 28: Variable name `e` is too short (single-letter)."
    )]
    #[case::struct_destructuring_shorthand(
        "fn main() { let Point { f, g } = p; }",
        "[single-letter-variable-name] Line 1, Col 28: Variable name `g` is too short (single-letter)."
    )]
    #[case::loop_target(
        "fn main() { for h in 0..10 {} }",
        "[single-letter-variable-name] Line 1, Col 17: Variable name `h` is too short (single-letter)."
    )]
    #[case::closure_parameter_allowed("fn main() { let f = |x: i32| x + 1; }", "")]
    #[case::fn_parameter_allowed("fn test(i: i32, j: i32) {}", "")]
    #[case::match_pattern_variants(
        "fn main() { match val { Some(k) => {}, None => {} } }",
        "[single-letter-variable-name] Line 1, Col 30: Variable name `k` is too short (single-letter)."
    )]
    #[case::if_let_and_while_let(
        "fn main() { if let Some(x) = y {} while let Some(z) = y {} }",
        "[single-letter-variable-name] Line 1, Col 50: Variable name `z` is too short (single-letter)."
    )]
    #[case::match_pattern_guard(
        "fn main() { match val { Some(z) if z > 0 => {} } }",
        "[single-letter-variable-name] Line 1, Col 30: Variable name `z` is too short (single-letter)."
    )]
    #[case::wildcard_ignored("fn main() { let _ = 1; }", "")]
    fn test_rust_bindings(#[case] source: &str, #[case] expected: &str) {
        let output = assert_code_rule_snapshot(&SingleLetterVariableName, source, "test.rs");
        assert_eq!(output.trim(), expected);
    }

    #[rstest]
    #[case::parameter_annotations(
        "def foo(i: int = 1, b: int = 1): pass",
        "[single-letter-variable-name] Line 1, Col 21: Variable name `b` is too short (single-letter)."
    )]
    #[case::assignment(
        "c = 2",
        "[single-letter-variable-name] Line 1, Col 1: Variable name `c` is too short (single-letter)."
    )]
    #[case::multi_assignment(
        "d, e = 3, 4",
        "[single-letter-variable-name] Line 1, Col 1: Variable name `d` is too short (single-letter).\n[single-letter-variable-name] Line 1, Col 4: Variable name `e` is too short (single-letter)."
    )]
    #[case::comprehension(
        "[y for y in range(10)]",
        "[single-letter-variable-name] Line 1, Col 8: Variable name `y` is too short (single-letter)."
    )]
    #[case::exception_alias(
        "try:\n    pass\nexcept Exception as g:\n    pass",
        "[single-letter-variable-name] Line 3, Col 21: Variable name `g` is too short (single-letter)."
    )]
    #[case::walrus_expression(
        "(v := 1)",
        "[single-letter-variable-name] Line 1, Col 2: Variable name `v` is too short (single-letter)."
    )]
    fn test_python_bindings(#[case] source: &str, #[case] expected: &str) {
        let output = assert_code_rule_snapshot(&SingleLetterVariableName, source, "test.py");
        assert_eq!(output.trim(), expected);
    }

    #[test]
    fn test_configuration_override() {
        let rule = SingleLetterVariableName;
        let config_toml = indoc::indoc! {r#"
            [rules.single-letter-variable-name]
            allowed = ["y"]
            banned = ["i"]
        "#};
        let config: crate::core::Config = toml::from_str(config_toml).unwrap();

        let source = "fn main() { let i = 1; let y = 2; }";
        let output = assert_code_rule_snapshot_with_config(&rule, source, "test.rs", &config);
        assert!(output.contains("Variable name `i` is too short"));
        assert!(!output.contains("Variable name `y`"));
    }
}
