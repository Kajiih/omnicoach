//! Enforces wrapping multiline string literals in a dedent helper (`textwrap.dedent`, `indoc!`, etc.).

use crate::code_lint::{CodeRule, RuleTarget, ast_python, ast_rust};
use crate::core::{FilterListDefaults, Rule, RuleName};
use crate::diagnostic::{Diagnostic, ViolationTemplate, violation_template};
use crate::rules::Tag;
use ast_grep_core::AstGrep;
use ast_grep_language::SupportLang;
use std::path::Path;

/// Static defaults for allowed multiline string wrapper functions/macros.
const DEFAULT_ALLOWED_WRAPPERS: FilterListDefaults = FilterListDefaults {
    base: &[],
    extend: &[
        (SupportLang::Python, &["cleandoc", "inspect.cleandoc"]),
        (
            SupportLang::Rust,
            &[
                "indoc",
                "indoc::indoc",
                "formatdoc",
                "indoc::formatdoc",
                "writedoc",
                "indoc::writedoc",
                "printdoc",
                "indoc::printdoc",
                "eprintdoc",
                "indoc::eprintdoc",
            ],
        ),
    ],
    exempt: &[],
};

const TEMPLATE: ViolationTemplate = violation_template! {
    summary: "Multiline string literal is not wrapped in a dedent helper.",
    rationale: "Indented multiline string literals include the enclosing block's leading spaces and initial newline in the runtime value (corrupting indentation-sensitive text and shifting line/column coordinates), while flushing lines to column 0 breaks the visual indentation hierarchy of the surrounding code.",
    suggestion: {
        base: "Wrap the multiline string in `inspect.cleandoc(\"\"\"...\"\"\")` (Python) or `indoc::indoc! {r\"...\"}` (Rust), or write a single-line string literal if the value has only one line.",
        Python => "Wrap the multiline string in `inspect.cleandoc(\"\"\"...\"\"\")`, or write a single-line string literal if the value has only one line.",
        Rust => "Wrap the multiline string in `indoc::indoc! {r\"...\"}` (or `indoc::formatdoc! {r\"...\"}` when interpolating), or write a single-line string literal if the value has only one line.",
    },
};

/// Rule enforcing that multiline string literals are wrapped in a dedent helper.
pub struct PreferDedentForMultilineStrings;

impl Rule for PreferDedentForMultilineStrings {
    fn name(&self) -> RuleName {
        RuleName("prefer-dedent-for-multiline-strings")
    }

    fn tags(&self) -> &'static [Tag] {
        &[Tag::Style]
    }

    fn supported_languages(&self) -> &'static [SupportLang] {
        &[SupportLang::Python, SupportLang::Rust]
    }

    fn violation_template(&self) -> &'static ViolationTemplate {
        &TEMPLATE
    }
}

impl CodeRule for PreferDedentForMultilineStrings {
    fn target(&self) -> RuleTarget {
        RuleTarget::All
    }

    fn check_file(
        &self,
        path: &Path,
        grep: &AstGrep<crate::code_lint::SourceDoc>,
        config: &crate::core::Config,
    ) -> Vec<Diagnostic> {
        let allowed = self.effective_allowed_set(*grep.lang(), config, &DEFAULT_ALLOWED_WRAPPERS);
        let nodes = match *grep.lang() {
            SupportLang::Python => {
                ast_python::collect_undented_multiline_strings(&grep.root(), &allowed)
            }
            SupportLang::Rust => {
                ast_rust::collect_undented_multiline_strings(&grep.root(), &allowed)
            }
            _ => Vec::new(),
        };
        nodes
            .into_iter()
            .map(|node| self.diagnostic_at_node(path, &node, &[]))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::Config;
    use crate::test_utils::{assert_code_rule_snapshot, assert_code_rule_snapshot_with_config};
    use rstest::rstest;

    #[test]
    fn test_python_multiline_strings_snapshot() {
        let rule = PreferDedentForMultilineStrings;
        let source = indoc::indoc! {r#"
            """Module docstring is exempt."""

            import inspect
            import textwrap

            GLOBAL_BAD = """
                select *
                from users
            """

            def render_query(user_id: int) -> str:
                """Function docstring is exempt."""
                bad_local = """
                    line 1
                    line 2
                """
                bad_dedent = textwrap.dedent("""
                    line 1
                    line 2
                """).strip()
                good_cleandoc = inspect.cleandoc("""
                    line 1
                    line 2
                """)
                good_implicit_concat = (
                    "line 1\n"
                    "line 2\n"
                )
                good_backslash = "hello \
                    world"
                good_fstring_expr = f"value: {max(
                    1,
                    2,
                )}"
                return bad_local
        "#};

        insta::assert_snapshot!(assert_code_rule_snapshot(&rule, source, "service.py"), @"
        [prefer-dedent-for-multiline-strings] Line 6, Col 14: Multiline string literal is not wrapped in a dedent helper.
        [prefer-dedent-for-multiline-strings] Line 13, Col 17: Multiline string literal is not wrapped in a dedent helper.
        [prefer-dedent-for-multiline-strings] Line 17, Col 34: Multiline string literal is not wrapped in a dedent helper.
        ");
    }

    #[test]
    fn test_rust_multiline_strings_snapshot() {
        let rule = PreferDedentForMultilineStrings;
        let source = indoc::indoc! {r#"
            const BAD_SQL: &str = "
                SELECT id
                FROM accounts
            ";

            fn build_fixture() {
                let bad_raw = r"
                    alpha
                    beta
                ";

                let good_indoc = indoc::indoc! {r"
                    alpha
                    beta
                "};

                let good_backslash = "hello \
                    world";

                insta::assert_snapshot!(good_indoc, @"
                    alpha
                    beta
                ");
            }
        "#};

        insta::assert_snapshot!(assert_code_rule_snapshot(&rule, source, "lib.rs"), @"
        [prefer-dedent-for-multiline-strings] Line 1, Col 23: Multiline string literal is not wrapped in a dedent helper.
        [prefer-dedent-for-multiline-strings] Line 7, Col 19: Multiline string literal is not wrapped in a dedent helper.
        ");
    }

    #[rstest]
    #[case::custom_extend_allowed(
        indoc::indoc! {r#"
            [rules.prefer-dedent-for-multiline-strings]
            extend_allowed = ["custom_dedent"]
        "#},
        indoc::indoc! {r#"
            x = custom_dedent("""
                hello
                world
            """)
        "#},
        true
    )]
    #[case::custom_banned_revokes_default(
        indoc::indoc! {r#"
            [rules.prefer-dedent-for-multiline-strings]
            banned = ["cleandoc", "inspect.cleandoc"]
        "#},
        indoc::indoc! {r#"
            x = inspect.cleandoc("""
                hello
                world
            """)
        "#},
        false
    )]
    fn test_config_allowlist_customization(
        #[case] config_toml: &str,
        #[case] source: &str,
        #[case] expect_clean: bool,
    ) {
        let rule = PreferDedentForMultilineStrings;
        let config: Config = toml::from_str(config_toml).unwrap();
        let snapshot = assert_code_rule_snapshot_with_config(&rule, source, "service.py", &config);
        assert_eq!(snapshot.is_empty(), expect_clean);
    }
}
