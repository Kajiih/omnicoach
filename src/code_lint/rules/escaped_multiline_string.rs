//! Flags single-line string literals that pack multiple lines using `\n` escapes.

use crate::code_lint::ast::{self, ParsedFile};
use crate::code_lint::contract::{CodeRule, RuleTarget};
use crate::diagnostic::{Diagnostic, Language, RuleName, ViolationTemplate, violation_template};
use crate::rule_declaration::{
    Classification, Consensus, CountOption, Declaration, Example, ImpactedQuality,
    LanguageDefaults, Precision, Reference, RuleDoc, RuleOptions, Topic,
};
use std::path::Path;

const MAX_NEWLINES: CountOption = CountOption {
    key: "max-newlines",
    doc: "Maximum escaped newlines allowed between the first and last non-blank lines of a single-line string literal.",
    default: LanguageDefaults::new(1, &[]),
};

const TEMPLATE: ViolationTemplate = violation_template! {
    summary: "Single-line string literal contains {count} escaped newlines between its first and last non-blank lines; the limit is {max_newlines}.",
    rationale: "Packing multiple lines onto one line with `\\n` escapes hides the text's line structure and indentation in a horizontal string.",
    suggestion: {
        Python => "Replace the literal with a multiline block wrapped in `inspect.cleandoc(\"\"\"...\"\"\")`.",
        Rust => "Replace the literal with a multiline block in `indoc::indoc! {r\"...\"}` (`indoc::formatdoc!` when interpolating).",
    },
};

/// The rule's declaration.
pub const RULE: CodeRule<CountOption> = CodeRule {
    declaration: Declaration {
        name: RuleName("escaped-multiline-string"),
        template: &TEMPLATE,
        languages: &[Language::Python, Language::Rust],
        options: RuleOptions::code_rule(MAX_NEWLINES),
        classification: Classification {
            topics: &[Topic::LITERALS],
            precision: Precision::Heuristic,
            consensus: Consensus::Opinionated,
            impacted_quality: ImpactedQuality::Maintainability,
        },
        doc: RuleDoc {
            summary: "Flags single-line string literals that pack multiple lines using `\\n` escapes.",
            what_it_does: indoc::indoc! {r#"
                Flags single-line string, byte-string, C-string, and f-string literals whose
                decoded value contains more than `max-newlines` line breaks between its first and
                last non-blank lines. Leading and trailing line breaks are not counted, so
                `"a\nb"` and `"a\nb\n"` are treated alike. Raw string literals (`r"..."`), strings
                with at most one non-blank line (such as `"\n\n"` or `"\n## Section\n"`), and
                multiline literals that use real source line breaks are not flagged."#},
            why_is_this_bad: indoc::indoc! {r#"
                Writing a multi-line snippet, query, or expected output on one line with `\n`
                escapes collapses its vertical layout and indentation into an unreadable
                horizontal string. Edits to one line shift the rest of the literal and make diffs
                hard to review.

                Write multi-line text across real source lines inside a dedent helper:
                `inspect.cleandoc("""...""")` in Python, `indoc::indoc!` (or `indoc::formatdoc!` /
                `indoc::writedoc!` when formatting) in Rust."#},
            known_problems: None,
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
                        QUERY = "SELECT id, email\nFROM users\nWHERE active = 1"
                    "#},
                    flagged_span: r#""SELECT id, email\nFROM users\nWHERE active = 1""#,
                    fixed: indoc::indoc! {r#"
                        import inspect

                        QUERY = inspect.cleandoc("""
                            SELECT id, email
                            FROM users
                            WHERE active = 1
                        """)
                    "#},
                },
                Example {
                    language: Language::Rust,
                    flagged: indoc::indoc! {r#"
                        const QUERY: &str = "SELECT id, email\nFROM users\nWHERE active = 1";
                    "#},
                    flagged_span: r#""SELECT id, email\nFROM users\nWHERE active = 1""#,
                    fixed: indoc::indoc! {r#"
                        const QUERY: &str = indoc::indoc! {"
                            SELECT id, email
                            FROM users
                            WHERE active = 1
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
    rule: &CodeRule<CountOption>,
    path: &Path,
    file: &ParsedFile,
    max_newlines: usize,
) -> Vec<Diagnostic> {
    ast::collect_multiline_strings(file)
        .into_iter()
        .filter_map(|literal| match literal.kind {
            ast::MultilineKind::Escaped { newlines } if newlines > max_newlines => {
                let formatted_count = newlines.to_string();
                let formatted_max = max_newlines.to_string();
                Some(rule.diagnostic_at_node(
                    path,
                    &literal.node,
                    &[
                        ("count", &formatted_count),
                        ("max_newlines", &formatted_max),
                    ],
                ))
            }
            ast::MultilineKind::Escaped { .. } | ast::MultilineKind::Source { .. } => None,
        })
        .collect()
}

#[cfg(test)]
crate::test_utils::rule_test!(
    RULE,
    {
        Python => {
            pass: [
                single_escaped_newline_within_limit => r#"
                    x = "line 1\nline 2"
                "#,
                edge_newlines_around_two_lines_within_limit => r#"
                    x = "\nline 1\nline 2\n"
                "#,
                raw_string_escapes_exempt => r#"
                    pattern = r"line 1\nline 2\nline 3"
                "#,
                raw_fstring_escapes_exempt => r#"
                    pattern = rf"line 1: {x}\nline 2: {y}\nline 3: {z}"
                "#,
                single_non_blank_line_delimiter_exempt => r#"
                    header = "\n## Configuration\n"
                "#,
                blank_newline_separator_exempt => r#"
                    sep = "\n\n\n"
                "#,
                adjacent_concat_parts_each_within_limit => r#"
                    text = (
                        "line 1\n"
                        "line 2\n"
                        "line 3\n"
                    )
                "#,
                cleandoc_multiline_exempt => r#"
                    import inspect
                    text = inspect.cleandoc("""
                        line 1
                        line 2
                        line 3
                    """)
                "#,
            ],
            fail: [
                three_line_escaped_string_flagged => r#"
                    text = "line 1\nline 2\nline 3"
                "# => r#""line 1\nline 2\nline 3""#,
                three_lines_with_trailing_newline_flagged => r#"
                    text = "line 1\nline 2\nline 3\n"
                "# => r#""line 1\nline 2\nline 3\n""#,
                escaped_bytes_literal_flagged => r#"
                    payload = b"header\nbody\nfooter"
                "# => r#"b"header\nbody\nfooter""#,
                escaped_fstring_flagged => r#"
                    msg = f"id: {user_id}\nrole: {role}\nactive: {active}"
                "# => r#"f"id: {user_id}\nrole: {role}\nactive: {active}""#,
                implicitly_concatenated_fstring_literal_part_flagged => r#"
                    msg = "line 1\nline 2\nline 3" f"{suffix}"
                "# => r#""line 1\nline 2\nline 3""#,
                single_line_triple_quoted_escaped_flagged => r#"
                    msg = """line 1\nline 2\nline 3"""
                "# => r#""""line 1\nline 2\nline 3""""#,
            ],
        },
        Rust => {
            pass: [
                single_escaped_newline_within_limit => r#"
                    fn build() {
                        let text = "line 1\nline 2";
                    }
                "#,
                edge_newlines_around_two_lines_within_limit => r#"
                    fn build() {
                        let text = "\nline 1\nline 2\n";
                    }
                "#,
                raw_string_escapes_exempt => r#"
                    fn build() {
                        let pattern = r"line 1\nline 2\nline 3";
                    }
                "#,
                single_non_blank_line_delimiter_exempt => r#"
                    fn build() {
                        let header = "\n## Configuration\n";
                    }
                "#,
                blank_newline_separator_exempt => r#"
                    fn build() {
                        let sep = "\n\n\n";
                    }
                "#,
                indoc_multiline_exempt => r#"
                    fn build() {
                        let text = indoc::indoc! {"
                            line 1
                            line 2
                            line 3
                        "};
                    }
                "#,
            ],
            fail: [
                three_line_escaped_string_flagged => r#"
                    fn build() {
                        let text = "line 1\nline 2\nline 3";
                    }
                "# => r#""line 1\nline 2\nline 3""#,
                three_lines_with_trailing_newline_flagged => r#"
                    fn build() {
                        let text = "line 1\nline 2\nline 3\n";
                    }
                "# => r#""line 1\nline 2\nline 3\n""#,
                escaped_byte_string_flagged => r#"
                    fn build() {
                        let payload = b"header\nbody\nfooter";
                    }
                "# => r#"b"header\nbody\nfooter""#,
                escaped_c_string_flagged => r#"
                    fn build() {
                        let payload = c"header\nbody\nfooter";
                    }
                "# => r#"c"header\nbody\nfooter""#,
            ],
        },
    }
);
