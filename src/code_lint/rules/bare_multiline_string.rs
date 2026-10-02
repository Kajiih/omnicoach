//! Enforces wrapping multiline string literals in a dedent helper (`inspect.cleandoc`, `indoc!`, etc.).

use crate::code_lint::ast::{self, ParsedFile};
use crate::code_lint::rule::{CodeRule, RuleTarget};
use crate::diagnostic::{Diagnostic, RuleName, ViolationTemplate, violation_template};
use crate::rule_declaration::{
    Classification, Consensus, Declaration, FilterListDefaults, ImpactedQuality, ListKind,
    ListOption, Precision, Reference, RuleDoc, RuleOptions, Topic,
};
use ast_grep_language::SupportLang;
use std::collections::HashSet;
use std::path::Path;

const ALLOWED: ListOption = ListOption {
    kind: ListKind::Allow,
    doc: "Functions and macros accepted as dedenting a multiline string.",
    default: FilterListDefaults {
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
        remove: &[],
    },
};

const TEMPLATE: ViolationTemplate = violation_template! {
    summary: "Multiline string literal is not wrapped in a dedent helper.",
    rationale: "An indented multiline literal carries the block's leading spaces and first newline into the runtime value, which corrupts indentation-sensitive text and shifts line and column coordinates; flushing it to column 0 breaks the indentation of the surrounding code instead.",
    suggestion: {
        base: "Wrap the literal in a dedent helper, or write a single-line literal if the value has one line.",
        Python => "Wrap the literal in `inspect.cleandoc(\"\"\"...\"\"\")`, or write a single-line literal if the value has one line.",
        Rust => "Wrap the literal in `indoc::indoc! {r\"...\"}` (`indoc::formatdoc!` when interpolating), or write a single-line literal if the value has one line.",
    },
};

/// The rule's declaration.
pub const RULE: CodeRule<ListOption> = CodeRule {
    declaration: Declaration {
        name: RuleName("bare-multiline-string"),
        template: &TEMPLATE,
        languages: &[SupportLang::Python, SupportLang::Rust],
        options: RuleOptions::code_rule(ALLOWED),
        classification: Classification {
            topics: &[Topic::LITERALS],
            precision: Precision::Exact,
            consensus: Consensus::Opinionated,
            impacted_quality: ImpactedQuality::Reliability,
        },
        doc: RuleDoc {
            summary: "Flags multiline string literals that are not wrapped in a dedent helper.",
            what_it_does: "Flags string literals that span several lines and contain a real \
                           line break. In Python these are triple-quoted strings; strings \
                           used as a statement on their own, such as docstrings, are not \
                           flagged. A string anywhere inside a call to `inspect.cleandoc` is \
                           allowed; `textwrap.dedent` is not allowed by default and must be \
                           added to the allow list. In Rust, normal, raw, byte and C string \
                           literals are checked; a normal string whose line breaks are all \
                           `\\` continuations is not flagged. Strings inside the `indoc` \
                           macros (`indoc!`, `formatdoc!`, `writedoc!`, `printdoc!`, \
                           `eprintdoc!`), `#[doc = ...]` attributes and `insta` inline \
                           snapshots (`@\"...\"`) are allowed. Test files are checked too.",
            why_is_this_bad: "A multiline literal keeps the source indentation and the line \
                              break after the opening quote in its value. Indented to match \
                              the code, the text carries extra spaces that break \
                              indentation-sensitive content (YAML, Markdown, expected output) \
                              and shift line and column numbers. Moved to column 0 to avoid \
                              that, it breaks the visual structure of the surrounding code.\n\n\
                              Indent the literal with the code and wrap it in a helper that \
                              strips the common indentation: `inspect.cleandoc(\"\"\"...\"\"\")` \
                              in Python, `indoc::indoc!` (or `formatdoc!` to interpolate) in \
                              Rust. Write a single-line literal when the value has one line.",
            references: &[
                Reference {
                    title: "Python docs: inspect.cleandoc",
                    url: "https://docs.python.org/3/library/inspect.html#inspect.cleandoc",
                },
                Reference {
                    title: "indoc crate documentation",
                    url: "https://docs.rs/indoc",
                },
            ],
        },
    },
    target: RuleTarget::All,
    check: check_file,
};

fn check_file(
    rule: &CodeRule<ListOption>,
    path: &Path,
    file: &ParsedFile,
    allowed: &HashSet<String>,
) -> Vec<Diagnostic> {
    ast::find_unwrapped_multiline_strings(file, |full_path, terminal| {
        allowed.contains(full_path) || allowed.contains(terminal)
    })
    .iter()
    .map(|node| rule.diagnostic_at_node(path, node, &[]))
    .collect()
}

#[cfg(test)]
crate::test_utils::rule_test!(
    RULE,
    {
        Python => {
            pass: [
                module_docstring_exempt => r#"
                    """Module docstring
                    spanning multiple lines."""
                    x = 1
                "#,
                function_docstring_exempt => r#"
                    def render():
                        """Function docstring
                        spanning multiple lines."""
                        return 1
                "#,
                inspect_cleandoc_allowed => r#"
                    import inspect
                    x = inspect.cleandoc("""
                        line 1
                        line 2
                    """)
                "#,
                implicit_adjacent_concat_allowed => r#"
                    x = (
                        "line 1\n"
                        "line 2\n"
                    )
                "#,
                backslash_continuation_allowed => r#"
                    x = "hello \
                        world"
                "#,
                fstring_multiline_interpolation_allowed => r#"
                    x = f"value: {max(
                        1,
                        2,
                    )}"
                "#,
            ],
            fail: [
                global_un_dedented_multiline => r#"
                    GLOBAL_BAD = """
                        select *
                        from users
                    """
                "# => r#"
                    """
                        select *
                        from users
                    """
                "#,
                local_un_dedented_multiline => r#"
                    def render():
                        bad_local = """
                            line 1
                            line 2
                        """
                        return bad_local
                "# => r#"
                    """
                        line 1
                        line 2
                    """
                "#,
                flushed_column_zero_inside_indented_function => r#"
def render():
    bad = """line 1
line 2
"""
    return bad
"# => r#"
"""line 1
line 2
"""
"#,
                textwrap_dedent_flagged_by_default => r#"
                    import textwrap
                    x = textwrap.dedent("""
                        line 1
                        line 2
                    """).strip()
                "# => r#"
                    """
                        line 1
                        line 2
                    """
                "#,
            ],
        },
        Rust => {
            pass: [
                indoc_macro_allowed => r#"
                    fn build() {
                        let good = indoc::indoc! {r"
                            alpha
                            beta
                        "};
                    }
                "#,
                backslash_continuation_allowed => r#"
                    fn build() {
                        let good = "hello \
                            world";
                    }
                "#,
                insta_inline_snapshot_exempt => r#"
                    fn test_snap() {
                        insta::assert_snapshot!(val, @"
                            alpha
                            beta
                        ");
                    }
                "#,
                doc_attribute_exempt => r#"
                    #[doc = "line 1
                    line 2"]
                    fn documented() {}
                "#,
            ],
            fail: [
                const_multiline_flagged => r#"
                    const BAD_SQL: &str = "
                        SELECT id
                        FROM accounts
                    ";
                "# => r#"
                    "
                        SELECT id
                        FROM accounts
                    "
                "#,
                local_raw_multiline_flagged => r#"
                    fn build() {
                        let bad_raw = r"
                            alpha
                            beta
                        ";
                    }
                "# => r#"
                    r"
                        alpha
                        beta
                    "
                "#,
            ],
        },
    }
);
