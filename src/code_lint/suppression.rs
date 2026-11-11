//! Inline and file-level suppression comment hygiene.

architecture_component!(CodeLintSuppression);

use crate::code_lint::ast::{self, ParsedFile};
use crate::code_lint::semantic::comments::{
    CommentIndex, OmniDirectivePrefix, is_line_prefix_whitespace, is_line_suffix_whitespace,
    is_standalone_comment_span, may_contain_suppression_directive, parse_omni_directive_prefix,
    split_python_directive_segments, starts_with_tool_directive, strip_comment_delimiters,
};
use crate::config::Config;
use crate::diagnostic::{
    Diagnostic, Language, LineColumn, RuleName, SourceLocation, SourceSpan, ViolationTemplate,
    violation_template,
};
use crate::rule_declaration::{
    Classification, Consensus, Declaration, ImpactedQuality, Precision, Reference, RuleDoc,
    RuleOptions, Topic,
};
use std::collections::{HashMap, HashSet};
use std::path::Path;

const MISSING_REASON_TEMPLATE: ViolationTemplate = violation_template! {
    summary: "Suppression directive gives no reason.",
    rationale: "A suppression without a reason cannot be reviewed: nobody can later tell whether it is still justified.",
    suggestion: "Add `-- <reason>` after the directive, or a comment above it, stating why the finding is acceptable here.",
};

/// Flags suppression directives missing a non-empty explanation reason.
const MISSING_SUPPRESSION_REASON: Declaration = Declaration {
    name: RuleName("missing-suppression-reason"),
    template: &MISSING_REASON_TEMPLATE,
    languages: &[Language::Python, Language::Rust],
    options: RuleOptions::none(),
    classification: Classification {
        topics: &[Topic::SUPPRESSION_DIRECTIVES],
        precision: Precision::Exact,
        consensus: Consensus::Opinionated,
        impacted_quality: ImpactedQuality::Maintainability,
    },
    doc: RuleDoc {
        summary: "Requires every suppression directive to give a reason.",
        what_it_does: indoc::indoc! {r"
            Flags suppression directives that give no reason, either as a non-empty `-- <reason>`
            at the end of the directive or as a substantive comment block directly above its line.
            For example, `# omni:ignore [nested-function]` is flagged, while
            `# omni:ignore [nested-function] -- required for fixture` is not.

            Checked directives are `omni:ignore` and `omni:disable-file` in Python and Rust, plus
            the Python third-party directives `noqa`, `type: ignore`, `pyright: ignore`,
            `ty: ignore`, `pyrefly: ignore`, `ruff: noqa`, `ruff: ignore`, `ruff: disable`,
            `ruff: file-ignore`, `flake8: noqa`, `pylint: disable` and `pylint: skip-file`, at the
            start of the comment or after a later `#` in it, as in `# see below  # noqa: E501`.
            Bare `noqa` matches in any case, as Ruff and flake8 accept `# NOQA`; the others only
            in lowercase. One `-- <reason>` covers every directive chained in the same
            comment, as in `# noqa: F401  # type: ignore[attr-defined] -- <reason>`. Rust
            `#[allow]` attributes are left to Clippy's `allow_attributes_without_reason`."},
        why_is_this_bad: indoc::indoc! {r"
            A suppression switches a check off, and only its author knows why. Without a reason, a
            reviewer cannot tell a deliberate exception from a shortcut, and a later reader cannot
            tell whether the suppression is still needed. The reason keeps the decision reviewable,
            and makes stale suppressions easy to spot and remove."},
        known_problems: Some(indoc::indoc! {r"
            Any substantive comment block directly above the directive's line counts as its
            reason, including an unrelated one such as a license header above a file-level
            directive. Text before the directive in the same comment, as in
            `# URL cannot wrap  # noqa: E501`, does not count as a reason. Two rare spellings that
            a tool honors are not checked: `# FLAKE8: NOQA` (flake8), and a `pylint:` pragma after
            text without its own `#`, as in `# note pylint: disable=...` (pylint)."}),
        references: &[
            Reference {
                title: "Ruff: Error suppression",
                url: "https://docs.astral.sh/ruff/linter/#error-suppression",
            },
            Reference {
                title: "Clippy: allow_attributes_without_reason",
                url: "https://rust-lang.github.io/rust-clippy/master/index.html#allow_attributes_without_reason",
            },
        ],
        examples: &[],
    },
};

const UNUSED_SUPPRESSION_TEMPLATE: ViolationTemplate = violation_template! {
    summary: "Suppression directive for `{rule}` matches no finding.",
    rationale: "A suppression that silences nothing is a dead comment; it misleads the reader into thinking the rule still fires here.",
    suggestion: "Remove `{rule}` from the suppression directive.",
};

/// Flags suppression directives when no violation occurred for the specified rule.
const UNUSED_SUPPRESSION: Declaration = Declaration {
    name: RuleName("unused-suppression"),
    template: &UNUSED_SUPPRESSION_TEMPLATE,
    languages: &[Language::Python, Language::Rust],
    options: RuleOptions::none(),
    classification: Classification {
        topics: &[Topic::SUPPRESSION_DIRECTIVES],
        precision: Precision::Exact,
        consensus: Consensus::Unopinionated,
        impacted_quality: ImpactedQuality::Reliability,
    },
    doc: RuleDoc {
        summary: "Flags suppression directives that suppress no finding.",
        what_it_does: indoc::indoc! {r"
            Flags each rule named in an `omni:ignore` or `omni:disable-file` directive that reports
            no finding in the directive's scope: its own line, the declaration below it, or the
            whole file. Rules disabled in the configuration or not run on the file are not checked,
            and unknown or missing rule names are left to `unknown-suppression-rule` and
            `blanket-suppression`."},
        why_is_this_bad: indoc::indoc! {r"
            A suppression that matches nothing is stale: the code was fixed or moved, or the
            directive sits on the wrong line. It misleads readers into thinking the line breaks a
            rule, and it keeps silencing that rule there, so if the problem comes back it is not
            reported.

            Remove the rule from the directive, or the whole directive when no rule is left."},
        known_problems: Some(indoc::indoc! {r"
            A directive on its own line covers contiguous decorators, attributes, and comments below
            it and the first declaration line after them, nothing further. A finding further down a
            multi-line statement is not suppressed, and the directive is reported as unused."}),
        references: &[Reference {
            title: "Ruff: unused-noqa (RUF100)",
            url: "https://docs.astral.sh/ruff/rules/unused-noqa/",
        }],
        examples: &[],
    },
};

const UNKNOWN_SUPPRESSION_TEMPLATE: ViolationTemplate = violation_template! {
    summary: "Suppression directive names `{rule}`, which is unknown or cannot be suppressed inline.",
    rationale: "The name is not a rule Omni knows, or it is a suppression audit, which inline directives cannot silence; either way the directive does nothing.",
    suggestion: "Verify the rule name with `--list-rules`, or disable suppression audits in `.omnicoach.toml`.",
};

/// Flags suppression directives targeting unknown or non-suppressible rules.
const UNKNOWN_SUPPRESSION_RULE: Declaration = Declaration {
    name: RuleName("unknown-suppression-rule"),
    template: &UNKNOWN_SUPPRESSION_TEMPLATE,
    languages: &[Language::Python, Language::Rust],
    options: RuleOptions::none(),
    classification: Classification {
        topics: &[Topic::SUPPRESSION_DIRECTIVES],
        precision: Precision::Exact,
        consensus: Consensus::Unopinionated,
        impacted_quality: ImpactedQuality::Reliability,
    },
    doc: RuleDoc {
        summary: "Flags suppression directives that name an unknown or unsuppressible rule.",
        what_it_does: indoc::indoc! {r"
            Flags each rule name in an `omni:ignore` or `omni:disable-file` directive that is not a
            code rule known to Omni, such as the typo `[nested-functions]`. The suppression audits
            themselves, such as `unused-suppression`, cannot be suppressed by a directive, so
            naming one is flagged too."},
        why_is_this_bad: indoc::indoc! {r"
            A misspelled or obsolete rule name suppresses nothing, but reads as if it did. The
            finding it was meant to hide is still reported, or a reader wrongly assumes a check is
            off. Names of renamed or removed rules pile up as noise.

            Fix the name (`--list-rules` prints every rule name), or remove it from the directive."},
        known_problems: None,
        references: &[],
        examples: &[],
    },
};

const BLANKET_SUPPRESSION_TEMPLATE: ViolationTemplate = violation_template! {
    summary: "Suppression directive names no rule.",
    rationale: "A directive without bracketed rule names suppresses nothing, yet it reads as if it silenced every check on the line or in the file.",
    suggestion: "Specify the rule names in brackets, as in `[rule-name] -- reason`.",
};

/// Flags blanket suppression directives that omit explicit rule names.
const BLANKET_SUPPRESSION: Declaration = Declaration {
    name: RuleName("blanket-suppression"),
    template: &BLANKET_SUPPRESSION_TEMPLATE,
    languages: &[Language::Python, Language::Rust],
    options: RuleOptions::none(),
    classification: Classification {
        topics: &[Topic::SUPPRESSION_DIRECTIVES],
        precision: Precision::Exact,
        consensus: Consensus::Unopinionated,
        impacted_quality: ImpactedQuality::Reliability,
    },
    doc: RuleDoc {
        summary: "Flags suppression directives that name no rule.",
        what_it_does: indoc::indoc! {r"
            Flags `omni:ignore` and `omni:disable-file` directives that name no rule. For example,
            `# omni:ignore -- legacy code` is flagged, while
            `# omni:ignore [nested-function] -- legacy code` is not."},
        why_is_this_bad: indoc::indoc! {r"
            Omni has no catch-all suppression: a directive silences only the rules it names. A
            directive without names does nothing, yet reads as if it switched off every check on the
            line or in the file, which misleads the reader.

            Name the rules to suppress in brackets, followed by a reason: `[rule-name] -- reason`."},
        known_problems: None,
        references: &[Reference {
            title: "Ruff: blanket-noqa (PGH004)",
            url: "https://docs.astral.sh/ruff/rules/blanket-noqa/",
        }],
        examples: &[],
    },
};

/// The suppression audits, evaluated by `SuppressionTracker::audit` rather than per file
/// like code rules.
pub const SUPPRESSION_AUDITS: &[Declaration] = &[
    MISSING_SUPPRESSION_REASON,
    UNUSED_SUPPRESSION,
    UNKNOWN_SUPPRESSION_RULE,
    BLANKET_SUPPRESSION,
];

/// Tracker responsible for collecting directives, filtering diagnostics, and auditing hygiene.
#[derive(Debug, Default)]
pub struct SuppressionTracker {
    /// Collected `omni:` / `omnicoach:` suppression directives in the file.
    directives: Vec<ParsedDirective>,
    /// Comment sites holding at least one unexplained omni or Python tool suppression directive.
    unexplained_directives: Vec<DirectiveSite>,
}

impl SuppressionTracker {
    /// Parses suppression directives from the parsed file and its content.
    #[must_use]
    pub fn from_file(file: &ParsedFile, content: &str) -> Self {
        // Skip the comment-node traversal when no suppression directive can exist.
        if !may_contain_suppression_directive(content, file.lang()) {
            return Self::default();
        }

        let comment_index = CommentIndex::from_file(file);
        let comment_nodes = ast::collect_comment_nodes(file);
        let mut skippable_end_lines = ast::decorator_and_attribute_end_lines(file);
        for comment_node in &comment_nodes {
            let span = comment_node.span();
            if is_standalone_comment_span(content, span) {
                let start_line = comment_node.start_line();
                let end_line = comment_node.end_line();
                skippable_end_lines
                    .entry(start_line)
                    .and_modify(|existing| *existing = (*existing).max(end_line))
                    .or_insert(end_line);
            }
        }
        let mut tracker = Self::default();
        for comment_node in comment_nodes {
            let text = comment_node.text();
            let site = DirectiveSite {
                span: comment_node.span(),
                end_line: comment_node.end_line(),
                coord: comment_node.start_coordinate(),
            };
            tracker.collect_comment(
                file.lang(),
                &text,
                site,
                content,
                &skippable_end_lines,
                &comment_index,
            );
        }
        tracker
    }

    /// Filters diagnostics against active directives, marking matched rules as used.
    #[must_use]
    pub(super) fn filter_diagnostics(&mut self, diagnostics: Vec<Diagnostic>) -> Vec<Diagnostic> {
        if self.directives.is_empty() {
            return diagnostics;
        }

        let mut retained = Vec::new();

        for diagnostic in diagnostics {
            let diagnostic_line = diagnostic.location.line;
            let rule_name = diagnostic.rule_name.0;

            let mut suppressed = false;

            for directive in &mut self.directives {
                if directive_matches_line(&directive.placement, diagnostic_line)
                    && directive.target_rules.iter().any(|rule| rule == rule_name)
                {
                    suppressed = true;
                    if let Some(count) = directive.matched_count.get_mut(rule_name) {
                        *count += 1;
                    }
                }
            }

            if !suppressed {
                retained.push(diagnostic);
            }
        }

        retained
    }

    /// Audits all parsed directives and emits suppression diagnostics according to configuration.
    ///
    /// `suppressible_rules` holds the names of every registered non-suppression code rule, while
    /// `evaluated_rules` holds the subset that actually ran on `path`; both are supplied by the
    /// caller so this module stays independent of the rule registry.
    #[must_use]
    pub(super) fn audit(
        &self,
        path: &Path,
        config: &Config,
        suppressible_rules: &HashSet<&'static str>,
        evaluated_rules: &HashSet<&'static str>,
    ) -> Vec<Diagnostic> {
        let mut diagnostics = Vec::new();
        for directive in &self.directives {
            audit_single_directive(
                directive,
                path,
                config,
                suppressible_rules,
                evaluated_rules,
                &mut diagnostics,
            );
        }
        if config.is_rule_enabled_for_path(MISSING_SUPPRESSION_REASON.name, path) {
            diagnostics.extend(self.unexplained_directives.iter().map(|site| {
                MISSING_SUPPRESSION_REASON
                    .render_diagnostic(&[], SourceLocation::file_span(path, site.span, site.coord))
            }));
        }
        diagnostics
    }

    /// Collects all omni and third-party suppression directives from a single comment node.
    fn collect_comment(
        &mut self,
        language: Language,
        text: &str,
        site: DirectiveSite,
        content: &str,
        skippable_end_lines: &HashMap<usize, usize>,
        comment_index: &CommentIndex<'_>,
    ) {
        let Some(stripped) = strip_comment_delimiters(text) else {
            return;
        };
        let segments = if language == Language::Python {
            split_python_directive_segments(stripped)
        } else {
            vec![stripped]
        };
        let tail_has_reason = segments
            .last()
            .is_some_and(|last| segment_has_reason(last, language));
        let mut has_unexplained_segment = false;

        for segment in &segments {
            let trimmed = segment.trim_start();
            if let Some(OmniDirectivePrefix { is_file, remainder }) =
                parse_omni_directive_prefix(trimmed)
            {
                let BracketedRules {
                    rules: target_rules,
                    rest: after_rules,
                } = parse_bracketed_rules(remainder.trim_start());
                let is_blanket = target_rules.is_empty();
                has_unexplained_segment |= !(has_dash_reason(after_rules) || tail_has_reason);
                let placement =
                    resolve_directive_placement(is_file, content, skippable_end_lines, site);
                let matched_count = target_rules.iter().map(|rule| (rule.clone(), 0)).collect();
                self.directives.push(ParsedDirective {
                    placement,
                    span: site.span,
                    coord: site.coord,
                    target_rules,
                    is_blanket,
                    matched_count,
                });
            } else if language == Language::Python
                && starts_with_tool_directive(trimmed)
                && !(has_dash_reason_anywhere(trimmed) || tail_has_reason)
            {
                has_unexplained_segment = true;
            }
        }

        if has_unexplained_segment && !comment_index.has_prose_explanation_above(site.coord.line) {
            self.unexplained_directives.push(site);
        }
    }
}

/// Where a suppression directive comment sits in the source file.
#[derive(Debug, Clone, Copy)]
struct DirectiveSite {
    /// The byte span of the comment.
    span: SourceSpan,
    /// The 1-indexed line where the comment ends.
    end_line: usize,
    /// The 1-indexed line and column where the comment starts.
    coord: LineColumn,
}

fn segment_has_reason(segment: &str, language: Language) -> bool {
    let trimmed = segment.trim_start();
    if let Some(OmniDirectivePrefix { remainder, .. }) = parse_omni_directive_prefix(trimmed) {
        let BracketedRules { rest, .. } = parse_bracketed_rules(remainder.trim_start());
        return has_dash_reason(rest);
    }
    language == Language::Python
        && starts_with_tool_directive(trimmed)
        && has_dash_reason_anywhere(trimmed)
}

fn has_dash_reason(after_rules: &str) -> bool {
    after_rules
        .trim_start()
        .strip_prefix("--")
        .is_some_and(|reason| !reason.trim().is_empty())
}

fn has_dash_reason_anywhere(segment: &str) -> bool {
    segment
        .split_once("--")
        .is_some_and(|(_, reason)| !reason.trim().is_empty())
}

/// The bracketed rule names `[rule-a, rule-b]` of a directive body.
struct BracketedRules<'a> {
    /// The rule names, empty for a blanket directive (no list, an unclosed list or an empty one).
    rules: Vec<String>,
    /// The directive body after the closing `]`, or the whole body without a closed list.
    rest: &'a str,
}

/// Parses bracketed rule names `[rule-a, rule-b]` from the directive body.
fn parse_bracketed_rules(remainder_trimmed: &str) -> BracketedRules<'_> {
    if !remainder_trimmed.starts_with('[') {
        return BracketedRules {
            rules: Vec::new(),
            rest: remainder_trimmed,
        };
    }
    remainder_trimmed.find(']').map_or_else(
        || BracketedRules {
            rules: Vec::new(),
            rest: remainder_trimmed,
        },
        |close_idx| {
            let raw_rules = &remainder_trimmed[1..close_idx];
            let rules: Vec<String> = raw_rules
                .split(',')
                .map(|segment| segment.trim().to_string())
                .filter(|segment| !segment.is_empty())
                .collect();
            BracketedRules {
                rules,
                rest: &remainder_trimmed[close_idx + 1..],
            }
        },
    )
}

/// A parsed omni suppression directive comment.
#[derive(Debug, Clone)]
struct ParsedDirective {
    /// The placement scope of the directive.
    placement: DirectivePlacement,
    /// The byte span of the comment in the source file.
    span: SourceSpan,
    /// The 1-indexed line and column coordinate where the directive comment resides.
    coord: LineColumn,
    /// Rule names targeted by the directive (e.g. `["single-letter-name"]`).
    target_rules: Vec<String>,
    /// Whether the directive omitted bracketed rule names (`[...]`).
    is_blanket: bool,
    /// Counter of violations matched and suppressed for each target rule.
    matched_count: HashMap<String, usize>,
}

/// The placement scope of a parsed suppression directive.
#[derive(Debug, Clone, PartialEq, Eq)]
enum DirectivePlacement {
    /// Directive placed on the same line as code (suppresses violations on `line`).
    SameLine {
        /// The 1-indexed line number where the directive is placed.
        line: usize,
    },
    /// Directive placed on its own standalone line preceding code (suppresses violations on `target_line..=end_target_line`).
    PrecedingLine {
        /// The 1-indexed line number immediately following the directive.
        target_line: usize,
        /// The 1-indexed line number of the declaration after any contiguous attributes/decorators (`#[...]`, `@...`).
        end_target_line: usize,
    },
    /// Directive applying to the entire file.
    File,
}

/// Resolves whether a directive applies to the whole file, the same line, or the following declaration line(s).
fn resolve_directive_placement(
    is_file: bool,
    content: &str,
    skippable_end_lines: &HashMap<usize, usize>,
    site: DirectiveSite,
) -> DirectivePlacement {
    if is_file {
        return DirectivePlacement::File;
    }
    if !is_line_prefix_whitespace(content, site.span.start) {
        return DirectivePlacement::SameLine {
            line: site.coord.line,
        };
    }
    if !is_line_suffix_whitespace(content, site.span.end) {
        return DirectivePlacement::SameLine {
            line: site.end_line,
        };
    }
    DirectivePlacement::PrecedingLine {
        target_line: site.end_line + 1,
        end_target_line: compute_effective_target_line(skippable_end_lines, site.end_line),
    }
}

/// Computes the declaration line after skipping contiguous attributes, decorators, or standalone
/// comments.
///
/// `skippable_end_lines` maps the first line of each decorator, attribute, or standalone comment
/// to its last line, so continuation lines of multiline items and block comments are skipped too.
fn compute_effective_target_line(
    skippable_end_lines: &HashMap<usize, usize>,
    raw_line: usize,
) -> usize {
    let mut current_line = raw_line + 1;
    while let Some(&end_line) = skippable_end_lines.get(&current_line) {
        current_line = end_line + 1;
    }
    current_line
}

/// Returns true if `placement` covers `diagnostic_line`.
fn directive_matches_line(placement: &DirectivePlacement, diagnostic_line: usize) -> bool {
    match *placement {
        DirectivePlacement::File => true,
        DirectivePlacement::SameLine { line } => line == diagnostic_line,
        DirectivePlacement::PrecedingLine {
            target_line,
            end_target_line,
        } => (target_line..=end_target_line).contains(&diagnostic_line),
    }
}

/// Template placeholder for the rule named in a directive.
const RULE_PLACEHOLDER: &str = "rule";

/// Audits a single `ParsedDirective` for blanket usage, missing reason, unknown rule names, and unused targets.
fn audit_single_directive(
    directive: &ParsedDirective,
    path: &Path,
    config: &Config,
    suppressible_rules: &HashSet<&'static str>,
    evaluated_rules: &HashSet<&'static str>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let location = SourceLocation::file_span(path, directive.span, directive.coord);

    if directive.is_blanket && config.is_rule_enabled_for_path(BLANKET_SUPPRESSION.name, path) {
        diagnostics.push(BLANKET_SUPPRESSION.render_diagnostic(&[], location.clone()));
    }

    if config.is_rule_enabled_for_path(UNKNOWN_SUPPRESSION_RULE.name, path) {
        for target_rule in &directive.target_rules {
            if !suppressible_rules.contains(target_rule.as_str()) {
                diagnostics.push(
                    UNKNOWN_SUPPRESSION_RULE
                        .render_diagnostic(&[(RULE_PLACEHOLDER, target_rule)], location.clone()),
                );
            }
        }
    }

    if !directive.is_blanket && config.is_rule_enabled_for_path(UNUSED_SUPPRESSION.name, path) {
        for target_rule in &directive.target_rules {
            if evaluated_rules.contains(target_rule.as_str())
                && directive
                    .matched_count
                    .get(target_rule)
                    .is_none_or(|&count| count == 0)
            {
                diagnostics.push(
                    UNUSED_SUPPRESSION
                        .render_diagnostic(&[(RULE_PLACEHOLDER, target_rule)], location.clone()),
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagnostic::Language;
    use rstest::rstest;

    #[rstest]
    #[case::omni("let a = 1; // omni:ignore [single-letter-name] -- math variable", 1)]
    #[case::omnicoach(
        "let a = 1; // omnicoach:ignore [single-letter-name] -- math variable",
        1
    )]
    #[case::no_space_before_brackets(
        "let a = 1; // omni:ignore[single-letter-name] -- math variable",
        1
    )]
    #[case::second_line(
        "let x = 1;\nlet a = 1; // omni:ignore [single-letter-name] -- math variable",
        2
    )]
    #[case::leading_block_comment_before_code(
        "/* omni:ignore [single-letter-name] -- math variable */ let a = 1;",
        1
    )]
    #[case::multiline_leading_block_comment_before_code(
        "/* omni:ignore [single-letter-name]\n   -- math variable */ let a = 1;",
        2
    )]
    fn test_parse_valid_inline_directive_same_line(
        #[case] content: &str,
        #[case] expected_line: usize,
    ) {
        let file = ParsedFile::new(content, Language::Rust);
        let tracker = SuppressionTracker::from_file(&file, content);
        let directive = &tracker.directives[0];

        assert_eq!(tracker.directives.len(), 1);
        assert_eq!(directive.target_rules, vec!["single-letter-name"]);
        assert!(tracker.unexplained_directives.is_empty());
        assert_eq!(
            directive.placement,
            DirectivePlacement::SameLine {
                line: expected_line
            }
        );
    }

    #[test]
    fn test_non_delimiter_after_omni_prefix_is_ignored() {
        let content = "// omni:ignored by compiler\nlet a = 1;";
        let file = ParsedFile::new(content, Language::Rust);
        let tracker = SuppressionTracker::from_file(&file, content);

        assert!(tracker.directives.is_empty());
    }

    #[rstest]
    #[case::direct_declaration(
        "# omni:ignore [nested-function] -- required for fixture\ndef inner(): pass",
        Language::Python,
        2,
        2
    )]
    #[case::skips_python_decorator_and_comment(
        indoc::indoc! {r"
            # omni:ignore [nested-function] -- required for fixture
            @decorator
            # note
            def inner(): pass"},
        Language::Python,
        2,
        4
    )]
    #[case::skips_multiline_python_decorators_and_comment(
        indoc::indoc! {r#"
            # omni:ignore [nested-function] -- required for fixture
            @decorator_one(
                "a",
                "b",
            )
            # note
            @decorator_two(
                "c",
            )
            def inner(): pass"#},
        Language::Python,
        2,
        10
    )]
    #[case::skips_rust_attribute_and_comment(
        indoc::indoc! {r"
            // omni:ignore [single-letter-name] -- math
            #[allow(dead_code)]
            // note
            fn f(a: i32) {}"},
        Language::Rust,
        2,
        4
    )]
    #[case::skips_multiline_rust_attributes_and_comment(
        indoc::indoc! {r"
            // omni:ignore [single-letter-name] -- math
            #[cfg_attr(
                test,
                allow(dead_code)
            )]
            // note
            #[derive(
                Debug
            )]
            fn f(a: i32) {}"},
        Language::Rust,
        2,
        10
    )]
    #[case::skips_multiline_rust_block_comment(
        indoc::indoc! {r"
            // omni:ignore [single-letter-name] -- math
            /* multiline
               block comment */
            #[allow(dead_code)]
            fn f(a: i32) {}"},
        Language::Rust,
        2,
        5
    )]
    #[case::standalone_multiline_block_directive_targets_line_after_comment_end(
        indoc::indoc! {r"
            /* omni:ignore [single-letter-name]
               -- math */
            #[allow(dead_code)]
            fn f(a: i32) {}"},
        Language::Rust,
        3,
        4
    )]
    fn test_parse_valid_inline_directive_preceding_line(
        #[case] content: &str,
        #[case] lang: Language,
        #[case] expected_target_line: usize,
        #[case] expected_end_target_line: usize,
    ) {
        let file = ParsedFile::new(content, lang);
        let tracker = SuppressionTracker::from_file(&file, content);
        let directive = &tracker.directives[0];

        assert_eq!(tracker.directives.len(), 1);
        assert!(!directive.is_blanket);
        assert_eq!(
            directive.placement,
            DirectivePlacement::PrecedingLine {
                target_line: expected_target_line,
                end_target_line: expected_end_target_line,
            }
        );
    }

    #[rstest]
    #[case::omni(
        "# omni:disable-file [nested-function, single-letter-name] -- legacy generated file\ndef foo(): pass"
    )]
    #[case::omnicoach(
        "# omnicoach:disable-file [nested-function, single-letter-name] -- legacy generated file\ndef foo(): pass"
    )]
    fn test_parse_file_level_directive(#[case] content: &str) {
        let file = ParsedFile::new(content, Language::Python);
        let tracker = SuppressionTracker::from_file(&file, content);
        let directive = &tracker.directives[0];

        assert_eq!(tracker.directives.len(), 1);
        assert_eq!(
            directive.target_rules,
            vec!["nested-function", "single-letter-name"]
        );
        assert!(tracker.unexplained_directives.is_empty());
        assert_eq!(directive.placement, DirectivePlacement::File);
    }

    #[rstest]
    #[case::missing_brackets("let a = 1; // omni:ignore -- missing brackets")]
    #[case::no_space_before_separator("let a = 1; // omni:ignore-- missing brackets")]
    #[case::empty_brackets("let a = 1; // omni:ignore [] -- empty brackets")]
    #[case::unclosed_bracket("let a = 1; // omni:ignore [single-letter-name -- unclosed bracket")]
    fn test_blanket_directive_detected(#[case] content: &str) {
        let file = ParsedFile::new(content, Language::Rust);
        let tracker = SuppressionTracker::from_file(&file, content);

        assert_eq!(tracker.directives.len(), 1);
        assert!(tracker.directives[0].is_blanket);
    }

    #[rstest]
    #[case::same_line_matches(DirectivePlacement::SameLine { line: 3 }, 3, true)]
    #[case::same_line_misses_other_line(DirectivePlacement::SameLine { line: 3 }, 4, false)]
    #[case::preceding_matches_start(
        DirectivePlacement::PrecedingLine {
            target_line: 2,
            end_target_line: 4,
        },
        2,
        true
    )]
    #[case::preceding_matches_end(
        DirectivePlacement::PrecedingLine {
            target_line: 2,
            end_target_line: 4,
        },
        4,
        true
    )]
    #[case::preceding_misses_before(
        DirectivePlacement::PrecedingLine {
            target_line: 2,
            end_target_line: 4,
        },
        1,
        false
    )]
    #[case::preceding_misses_after(
        DirectivePlacement::PrecedingLine {
            target_line: 2,
            end_target_line: 4,
        },
        5,
        false
    )]
    #[case::file_matches_any_line(DirectivePlacement::File, 42, true)]
    fn test_directive_matches_line(
        #[case] placement: DirectivePlacement,
        #[case] diagnostic_line: usize,
        #[case] expected: bool,
    ) {
        assert_eq!(
            directive_matches_line(&placement, diagnostic_line),
            expected
        );
    }

    fn has_unexplained_python_directive(comment: &str) -> bool {
        let file = ParsedFile::new(comment, Language::Python);
        !SuppressionTracker::from_file(&file, comment)
            .unexplained_directives
            .is_empty()
    }

    #[rstest]
    #[case::noqa("# noqa: F401")]
    #[case::bare_noqa("# noqa")]
    #[case::type_ignore("# type: ignore[attr-defined]")]
    #[case::type_ignore_without_space("# type:ignore")]
    #[case::pyright("# pyright: ignore[reportAttributeAccessIssue]")]
    #[case::ty("# ty: ignore[unresolved-attribute]")]
    #[case::pyrefly_file("# pyrefly: ignore-errors")]
    #[case::ruff_noqa("# ruff: noqa: F401")]
    #[case::ruff_disable("# ruff: disable[E501]")]
    #[case::ruff_file_ignore("# ruff: file-ignore[E501]")]
    #[case::flake8("# flake8: noqa")]
    #[case::pylint_disable_next("# pylint: disable-next=protected-access")]
    #[case::pylint_skip_file("# pylint: skip-file")]
    #[case::empty_reason("# type: ignore[attr-defined] --   ")]
    #[case::chained_without_reason("# noqa: F401  # type: ignore[attr-defined]")]
    #[case::noqa_uppercase("# NOQA: E501")]
    #[case::noqa_mixed_case("# Noqa")]
    #[case::noqa_mid_comment("# see below  # noqa: E501")]
    #[case::type_ignore_mid_comment("# see below  # type: ignore[attr-defined]")]
    #[case::pylint_mid_comment("# see below  # pylint: disable=unused-import")]
    fn test_unexplained_tool_directive_detected(#[case] comment: &str) {
        assert!(has_unexplained_python_directive(comment));
    }

    #[rstest]
    #[case::inline_reason("# type: ignore[attr-defined] -- set by the plugin loader")]
    #[case::chained_shared_reason("# noqa: F401  # type: ignore[attr-defined] -- re-exported")]
    #[case::mid_comment_reason("# see below  # noqa: E501 -- the URL cannot be wrapped")]
    #[case::url_anchor_fragment("# See https://docs.astral.sh/ruff/linter/#noqa for details")]
    #[case::reenable("# pylint: enable=protected-access")]
    #[case::type_comment("# type: int")]
    #[case::keyword_prefix("# noquality checks below")]
    #[case::keyword_suffix("# type: ignored on purpose")]
    #[case::type_ignore_uppercase("# TYPE: IGNORE")]
    #[case::ruff_noqa_uppercase("# RUFF: NOQA")]
    #[case::pylint_uppercase("# PYLINT: DISABLE=unused-import")]
    #[case::keyword_in_prose("# we disable caching here")]
    fn test_explained_or_non_tool_directive_not_detected(#[case] comment: &str) {
        assert!(!has_unexplained_python_directive(comment));
    }

    #[rstest]
    #[case::omni_standalone(
        indoc::indoc! {r"
            # The fixture needs a closure over the client.
            # omni:ignore [nested-function]
            def inner(): pass"},
        true
    )]
    #[case::omni_trailing(
        "# Coordinates follow the paper's notation.\na = 1  # omni:ignore [single-letter-name]",
        true
    )]
    #[case::omni_too_short_above(
        indoc::indoc! {r"
            # why
            # omni:ignore [nested-function]
            def inner(): pass"},
        false
    )]
    #[case::omni_trailing_comment_above_is_not_standalone(
        "b = 2  # The fixture needs a closure here.\na = 1  # omni:ignore [single-letter-name]",
        false
    )]
    #[case::stacked_directive_reason_does_not_excuse_next_directive(
        indoc::indoc! {r"
            # omni:ignore [nested-function] -- closure required for fixture
            # omni:ignore [single-letter-name]
            def inner(): pass"},
        false
    )]
    fn test_omni_directive_explained_above(#[case] content: &str, #[case] expected: bool) {
        let file = ParsedFile::new(content, Language::Python);
        let tracker = SuppressionTracker::from_file(&file, content);

        assert!(
            !tracker.directives.is_empty(),
            "expected at least one omni directive"
        );
        assert_eq!(tracker.unexplained_directives.is_empty(), expected);
    }

    #[rstest]
    #[case::unexplained("import os  # noqa: F401", Language::Python, 1)]
    #[case::unexplained_uppercase("import os  # NOQA: F401", Language::Python, 1)]
    #[case::explained_above(
        "# Re-exported so plugins can import it from here.\nimport os  # noqa: F401",
        Language::Python,
        0
    )]
    #[case::string_ignored("TEXT = \"# noqa: F401\"", Language::Python, 0)]
    #[case::rust_ignored("let a = 1; // noqa: F401", Language::Rust, 0)]
    #[case::chained_tool_and_omni_shared_reason(
        "value = cast(int, raw)  # type: ignore[arg-type]  # omni:ignore [type-cast] -- validated upstream",
        Language::Python,
        0
    )]
    #[case::chained_omni_and_tool_shared_reason(
        "value = cast(int, raw)  # omni:ignore [type-cast]  # type: ignore[arg-type] -- validated upstream",
        Language::Python,
        0
    )]
    #[case::chained_omni_with_reason_then_unexplained_tool(
        "value = cast(int, raw)  # omni:ignore [type-cast] -- validated upstream  # type: ignore[arg-type]",
        Language::Python,
        1
    )]
    #[case::chained_two_unexplained_omni_emits_single_site(
        "value = cast(int, raw)  # omni:ignore [type-cast]  # omni:ignore [single-letter-name]",
        Language::Python,
        1
    )]
    #[case::reason_with_hash_issue_number(
        "value = cast(int, raw)  # type: ignore[arg-type]  # omni:ignore [type-cast] -- tracks #42",
        Language::Python,
        0
    )]
    fn test_unexplained_directives_collected(
        #[case] content: &str,
        #[case] lang: Language,
        #[case] expected_count: usize,
    ) {
        let file = ParsedFile::new(content, lang);
        let tracker = SuppressionTracker::from_file(&file, content);

        assert_eq!(tracker.unexplained_directives.len(), expected_count);
    }
}
