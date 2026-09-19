//! Declarations of generic rules targeting multiple languages.

use crate::code_lint::CodeRule;
use crate::core::{AllowListConfig, DynamicRuleConfig, FilterListDefaults, Rule, RuleName};
use crate::diagnostic::{violation_template, Diagnostic, ViolationTemplate};
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

/// Configuration for the `SingleLetterVariableName` rule.
pub type SingleLetterVariableNameConfig = DynamicRuleConfig<AllowListConfig>;

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
// TODO: Is this fully language agnostic?
impl CodeRule for SingleLetterVariableName {
    fn check_file(
        &self,
        path: &Path,
        grep: &AstGrep<crate::code_lint::SourceDoc>,
        config: &crate::core::Config,
    ) -> Vec<Diagnostic> {
        let rule_config: SingleLetterVariableNameConfig = config.get_rule_config(self.name().0);
        let effective_allowed =
            rule_config.effective_allowed_for_lang(*grep.lang(), &DEFAULT_ALLOWED);

        let mut diagnostics = Vec::new();

        let bindings = crate::code_lint::collect_bindings(grep);
        let lang = *grep.lang();

        for node in bindings {
            if crate::code_lint::is_unaliased_import_binding(&node, lang) {
                continue;
            }
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

        // Custom allowed list: allow 'y', deny 'i'
        let config_toml = r#"
            [rules.single-letter-variable-name]
            allowed = ["y"]
        "#;
        let config: crate::core::Config = toml::from_str(config_toml).unwrap();

        let source = "fn main() { let i = 1; let y = 2; }";
        insta::assert_snapshot!(assert_code_rule_snapshot_with_config(&rule, source, "test.rs", &config), @"[single-letter-variable-name] Line 1, Col 17: Variable name `i` is too short (single-letter).");
    }

    #[test]
    fn test_language_nested_overrides() {
        let rule = SingleLetterVariableName;

        // Custom config:
        // - Global allows 'g'
        // - Rust allows 'r'
        // - Python allows 'p'
        let config_toml = r#"
            [rules.single-letter-variable-name]
            allowed = ["g"]
            
            [rules.single-letter-variable-name.rust]
            allowed = ["r"]

            [rules.single-letter-variable-name.python]
            allowed = ["p"]
        "#;
        let config: crate::core::Config = toml::from_str(config_toml).unwrap();

        // For Rust:
        // - 'r' is allowed by Rust override
        // - 'g' is NOT allowed because Rust override takes precedence
        // - 'c' is NOT allowed because we have overrides
        let source_rs = "fn main() { let r = 1; let g = 2; let c = 3; }";
        insta::assert_snapshot!(assert_code_rule_snapshot_with_config(&rule, source_rs, "test.rs", &config), @"
        [single-letter-variable-name] Line 1, Col 28: Variable name `g` is too short (single-letter).
        [single-letter-variable-name] Line 1, Col 39: Variable name `c` is too short (single-letter).
        ");

        // For Python:
        // - 'p' is allowed by Python override
        // - 'g' is NOT allowed because Python override takes precedence
        let source_py = "p = 1\ng = 2";
        insta::assert_snapshot!(assert_code_rule_snapshot_with_config(&rule, source_py, "test.py", &config), @"[single-letter-variable-name] Line 2, Col 1: Variable name `g` is too short (single-letter).");
    }

    #[test]
    fn test_language_nested_overrides_fallback() {
        let rule = SingleLetterVariableName;

        // Custom config:
        // - Global allows 'g'
        // - Rust / Python have no language-specific overrides
        let config_toml = r#"
            [rules.single-letter-variable-name]
            allowed = ["g"]
        "#;
        let config: crate::core::Config = toml::from_str(config_toml).unwrap();

        // For Rust:
        // - 'g' is allowed by global override fallback
        // - 'c' is NOT allowed
        let source_rs = "fn main() { let g = 1; let c = 2; }";
        insta::assert_snapshot!(assert_code_rule_snapshot_with_config(&rule, source_rs, "test.rs", &config), @"[single-letter-variable-name] Line 1, Col 28: Variable name `c` is too short (single-letter).");
    }

    #[test]
    fn test_extend_allowed() {
        let rule = SingleLetterVariableName;

        let config_toml = r#"
            [rules.single-letter-variable-name]
            extend_allowed = ["k"]
        "#;
        let config: crate::core::Config = toml::from_str(config_toml).unwrap();

        // 'i' is allowed by default, 'k' is allowed by extend_allowed, 'a' violates
        let source_rs = "fn main() { let i = 1; let k = 2; let a = 3; }";
        let output = assert_code_rule_snapshot_with_config(&rule, source_rs, "test.rs", &config);
        assert!(!output.contains("Variable name `i`"));
        assert!(!output.contains("Variable name `k`"));
        assert!(output.contains("Variable name `a` is too short"));
    }

    #[test]
    fn test_banned_revocation() {
        let rule = SingleLetterVariableName;

        let config_toml = r#"
            [rules.single-letter-variable-name]
            banned = ["i"]
        "#;
        let config: crate::core::Config = toml::from_str(config_toml).unwrap();

        // 'i' is revoked/banned, 'x' remains allowed by default base
        let source_rs = "fn main() { let i = 1; let x = 2; }";
        let output = assert_code_rule_snapshot_with_config(&rule, source_rs, "test.rs", &config);
        assert!(output.contains("Variable name `i` is too short"));
        assert!(!output.contains("Variable name `x`"));
    }
}
