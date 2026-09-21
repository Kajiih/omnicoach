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
        let root = grep.root();
        root.dfs()
            .filter(|node| match *grep.lang() {
                SupportLang::Python => {
                    ast_python::is_multiline_string_literal(node)
                        && !ast_python::is_docstring(node)
                        && !ast_python::is_enclosed_in_call(node, |full_path, terminal| {
                            allowed.contains(full_path) || allowed.contains(terminal)
                        })
                }
                SupportLang::Rust => {
                    ast_rust::is_multiline_string_literal(node)
                        && !ast_rust::is_insta_inline_snapshot(node)
                        && !ast_rust::is_enclosed_in_doc_attribute(node)
                        && !ast_rust::is_enclosed_in_macro(node, |full_path, terminal| {
                            allowed.contains(full_path) || allowed.contains(terminal)
                        })
                }
                _ => false,
            })
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

    #[rstest]
    #[case::module_docstring_exempt(
        indoc::indoc! {r#"
            """Module docstring
            spanning multiple lines."""
            x = 1
        "#},
        false
    )]
    #[case::function_docstring_exempt(
        indoc::indoc! {r#"
            def render():
                """Function docstring
                spanning multiple lines."""
                return 1
        "#},
        false
    )]
    #[case::global_raw_multiline_flagged(
        indoc::indoc! {r#"
            GLOBAL_BAD = """
                select *
                from users
            """
        "#},
        true
    )]
    #[case::local_raw_multiline_flagged(
        indoc::indoc! {r#"
            def render():
                bad_local = """
                    line 1
                    line 2
                """
                return bad_local
        "#},
        true
    )]
    #[case::textwrap_dedent_flagged_by_default(
        indoc::indoc! {r#"
            import textwrap
            x = textwrap.dedent("""
                line 1
                line 2
            """).strip()
        "#},
        true
    )]
    #[case::inspect_cleandoc_allowed(
        indoc::indoc! {r#"
            import inspect
            x = inspect.cleandoc("""
                line 1
                line 2
            """)
        "#},
        false
    )]
    #[case::implicit_adjacent_concat_allowed(
        indoc::indoc! {r#"
            x = (
                "line 1\n"
                "line 2\n"
            )
        "#},
        false
    )]
    #[case::backslash_continuation_allowed(
        indoc::indoc! {r#"
            x = "hello \
                world"
        "#},
        false
    )]
    #[case::fstring_multiline_interpolation_allowed(
        indoc::indoc! {r#"
            x = f"value: {max(
                1,
                2,
            )}"
        "#},
        false
    )]
    fn test_python_multiline_strings(#[case] source: &str, #[case] expect_flagged: bool) {
        let snapshot =
            assert_code_rule_snapshot(&PreferDedentForMultilineStrings, source, "service.py");
        assert_eq!(
            !snapshot.is_empty(),
            expect_flagged,
            "unexpected result: {snapshot}"
        );
    }

    #[rstest]
    #[case::const_multiline_flagged(
        indoc::indoc! {r#"
            const BAD_SQL: &str = "
                SELECT id
                FROM accounts
            ";
        "#},
        true
    )]
    #[case::local_raw_multiline_flagged(
        indoc::indoc! {r#"
            fn build() {
                let bad_raw = r"
                    alpha
                    beta
                ";
            }
        "#},
        true
    )]
    #[case::indoc_macro_allowed(
        indoc::indoc! {r#"
            fn build() {
                let good = indoc::indoc! {r"
                    alpha
                    beta
                "};
            }
        "#},
        false
    )]
    #[case::backslash_continuation_allowed(
        indoc::indoc! {r#"
            fn build() {
                let good = "hello \
                    world";
            }
        "#},
        false
    )]
    #[case::insta_inline_snapshot_exempt(
        indoc::indoc! {r#"
            fn test_snap() {
                insta::assert_snapshot!(val, @"
                    alpha
                    beta
                ");
            }
        "#},
        false
    )]
    fn test_rust_multiline_strings(#[case] source: &str, #[case] expect_flagged: bool) {
        let snapshot =
            assert_code_rule_snapshot(&PreferDedentForMultilineStrings, source, "lib.rs");
        assert_eq!(
            !snapshot.is_empty(),
            expect_flagged,
            "unexpected result: {snapshot}"
        );
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
