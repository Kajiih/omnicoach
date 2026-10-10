//! Enforces wrapping multiline string literals in a dedent helper (`inspect.cleandoc`, `indoc!`, etc.).

use crate::code_lint::ast::{self, ParsedFile};
use crate::code_lint::contract::{CodeRule, RuleTarget};
use crate::diagnostic::{Diagnostic, Language, RuleName, ViolationTemplate, violation_template};
use crate::rule_declaration::{
    Classification, Consensus, Declaration, Example, FilterListDefaults, ImpactedQuality, ListKind,
    ListOption, Precision, Reference, RuleDoc, RuleOptions, Topic,
};
use std::collections::HashSet;
use std::path::Path;

const ALLOWED: ListOption = ListOption {
    kind: ListKind::Allow,
    doc: "Functions and macros accepted as dedenting a multiline string.",
    default: FilterListDefaults {
        base: &[],
        extend: &[
            (Language::Python, &["cleandoc", "inspect.cleandoc"]),
            (
                Language::Rust,
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
        Python => "Wrap the literal in `inspect.cleandoc(\"\"\"...\"\"\")`, or write a single-line literal if the value has one line.",
        Rust => "Wrap the literal in `indoc::indoc! {r\"...\"}` (`indoc::formatdoc!` when interpolating), or write a single-line literal if the value has one line.",
    },
};

/// The rule's declaration.
pub const RULE: CodeRule<ListOption> = CodeRule {
    declaration: Declaration {
        name: RuleName("bare-multiline-string"),
        template: &TEMPLATE,
        languages: &[Language::Python, Language::Rust],
        options: RuleOptions::code_rule(ALLOWED),
        classification: Classification {
            topics: &[Topic::LITERALS],
            precision: Precision::Exact,
            consensus: Consensus::Opinionated,
            impacted_quality: ImpactedQuality::Reliability,
        },
        doc: RuleDoc {
            summary: "Flags multiline string literals that are not wrapped in a dedent helper.",
            what_it_does: indoc::indoc! {r#"
                Flags string literals that span several lines and contain a real line break,
                unless a dedent helper such as `inspect.cleandoc` or `indoc::indoc!` wraps them.
                Python docstrings are not flagged, nor are Rust `#[doc = ...]` attributes and
                `insta` inline snapshots (`@"..."`)."#},
            why_is_this_bad: indoc::indoc! {r#"
                A multiline literal keeps the source indentation and the line break after the
                opening quote in its value. Indented to match the code, the text carries extra
                spaces that break indentation-sensitive content (YAML, Markdown, expected output)
                and shift line and column numbers. Moved to column 0 to avoid that, it breaks the
                visual structure of the surrounding code.

                Indent the literal with the code and wrap it in a helper that strips the common
                indentation: `inspect.cleandoc("""...""")` in Python, `indoc::indoc!` (or
                `formatdoc!` to interpolate) in Rust. Write a single-line literal when the value has
                one line."#},
            known_problems: Some(indoc::indoc! {r"
                The allow list matches a call or macro by its written path or its last segment,
                without resolving imports: any `x.cleandoc(...)` is accepted, and an aliased
                helper such as `from inspect import cleandoc as dedent` is not."}),
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
            examples: &[
                Example {
                    language: Language::Python,
                    flagged: indoc::indoc! {r#"
                        ACTIVE_USERS_QUERY = """
                            SELECT id, email
                            FROM users
                        """
                    "#},
                    flagged_span: indoc::indoc! {r#"
                        """
                            SELECT id, email
                            FROM users
                        """
                    "#},
                    fixed: indoc::indoc! {r#"
                        import inspect

                        ACTIVE_USERS_QUERY = inspect.cleandoc("""
                            SELECT id, email
                            FROM users
                        """)
                    "#},
                },
                Example {
                    language: Language::Rust,
                    flagged: indoc::indoc! {r#"
                        const ACTIVE_USERS_QUERY: &str = r"
                            SELECT id, email
                            FROM users
                        ";
                    "#},
                    flagged_span: indoc::indoc! {r#"
                        r"
                            SELECT id, email
                            FROM users
                        "
                    "#},
                    fixed: indoc::indoc! {r#"
                        const ACTIVE_USERS_QUERY: &str = indoc::indoc! {r"
                            SELECT id, email
                            FROM users
                        "};
                    "#},
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
