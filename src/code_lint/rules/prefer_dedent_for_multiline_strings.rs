//! Enforces wrapping multiline string literals in a dedent helper (`textwrap.dedent`, `indoc!`, etc.).

use crate::code_lint::ast::{self, ParsedFile};
use crate::code_lint::rule::{CodeDetector, RuleTarget};
use crate::core::{Detector, FilterListDefaults};
use crate::diagnostic::{Diagnostic, RuleName, ViolationTemplate, violation_template};
use crate::rule_documentation::{ConfigShape, Reference, RuleDoc};
use crate::rule_taxonomy::{Classification, Consensus, ImpactedQuality, Precision, Topic};
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
                "rule_test",
                "crate::rule_test",
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

impl PreferDedentForMultilineStrings {
    /// The rule's declared facets (ADR 007).
    pub(crate) const CLASSIFICATION: Classification = Classification {
        topics: &[Topic::LITERALS],
        precision: Precision::Exact,
        consensus: Consensus::Opinionated,
        impacted_quality: ImpactedQuality::Reliability,
    };

    /// The rule's user-facing doc.
    pub(crate) const DOC: RuleDoc = RuleDoc {
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
        configuration: &[ConfigShape::AllowList],
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
    };
}

impl Detector for PreferDedentForMultilineStrings {
    fn name(&self) -> RuleName {
        RuleName("prefer-dedent-for-multiline-strings")
    }

    fn supported_languages(&self) -> &'static [SupportLang] {
        &[SupportLang::Python, SupportLang::Rust]
    }

    fn violation_template(&self) -> &'static ViolationTemplate {
        &TEMPLATE
    }
}

impl CodeDetector for PreferDedentForMultilineStrings {
    fn target(&self) -> RuleTarget {
        RuleTarget::All
    }

    fn check_file(
        &self,
        path: &Path,
        file: &ParsedFile,
        config: &crate::core::Config,
    ) -> Vec<Diagnostic> {
        let allowed = self.effective_allowed_set(file.lang(), config, &DEFAULT_ALLOWED_WRAPPERS);
        ast::find_unwrapped_multiline_strings(file, |full_path, terminal| {
            allowed.contains(full_path) || allowed.contains(terminal)
        })
        .iter()
        .map(|node| self.diagnostic_at_node(path, node, &[]))
        .collect()
    }
}

#[cfg(test)]
crate::test_utils::rule_test!(
    PreferDedentForMultilineStrings,
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
