//! Inline and file-level suppression comment hygiene.

architecture_component!(CodeLintSuppression);

use crate::code_lint::ast::{self, ParsedFile};
use crate::code_lint::semantic::comments::{CommentIndex, strip_comment_delimiters};
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
            Python comments starting with a third-party directive: `noqa`, `type: ignore`,
            `pyright: ignore`, `ty: ignore`, `pyrefly: ignore`, `ruff: noqa`, `ruff: ignore`,
            `ruff: disable`, `ruff: file-ignore`, `flake8: noqa`, `pylint: disable` and
            `pylint: skip-file`. One `-- <reason>` covers every directive chained in the same
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
            directive. A third-party directive is recognized only at the start of a comment and in
            lowercase, so `# see below  # noqa: E501` and `# NOQA: E501` are not checked."}),
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

/// The suppression audits, evaluated by [`SuppressionTracker::audit`] rather than per file
/// like code rules.
pub const SUPPRESSION_AUDITS: &[Declaration] = &[
    MISSING_SUPPRESSION_REASON,
    UNUSED_SUPPRESSION,
    UNKNOWN_SUPPRESSION_RULE,
    BLANKET_SUPPRESSION,
];

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
    /// Whether a reason is given after `--` or in the comment block directly above.
    is_explained: bool,
    /// Whether the directive omitted bracketed rule names (`[...]`).
    is_blanket: bool,
    /// Counter of violations matched and suppressed for each target rule.
    matched_count: HashMap<String, usize>,
}

/// Where a suppression directive comment sits in the source file.
#[derive(Debug, Clone, Copy)]
struct DirectiveSite {
    /// The byte span of the comment.
    span: SourceSpan,
    /// The 1-indexed line and column where the comment starts.
    coord: LineColumn,
}

/// Tracker responsible for collecting directives, filtering diagnostics, and auditing hygiene.
#[derive(Debug, Default)]
pub struct SuppressionTracker {
    /// Collected suppression directives in the file.
    directives: Vec<ParsedDirective>,
    /// Third-party Python suppression directives (`# noqa`, `# type: ignore`, ...) giving no reason.
    unexplained_tool_directives: Vec<DirectiveSite>,
}

const DIRECTIVE_NAMESPACES: &[&str] = &["omnicoach:", "omni:"];

/// Substrings of which every third-party Python suppression directive contains at least one.
const TOOL_DIRECTIVE_MARKERS: &[&str] = &["noqa", "ignore", "disable", "skip-file"];

/// Returns `true` if `content` may contain a suppression directive: an `omni:` or `omnicoach:`
/// one in any language, or a third-party one in Python.
#[must_use]
pub fn contains_directive_prefix(content: &str, lang: Language) -> bool {
    DIRECTIVE_NAMESPACES
        .iter()
        .any(|namespace| content.contains(namespace))
        || (lang == Language::Python
            && TOOL_DIRECTIVE_MARKERS
                .iter()
                .any(|marker| content.contains(marker)))
}

impl SuppressionTracker {
    /// Parses suppression directives from the parsed file and its content.
    #[must_use]
    pub fn from_file(file: &ParsedFile, content: &str) -> Self {
        // Skip the comment-node traversal when no suppression directive can exist.
        if !contains_directive_prefix(content, file.lang()) {
            return Self::default();
        }

        let comment_index = CommentIndex::from_file(file);
        let decorator_end_lines = ast::statements::decorator_and_attribute_end_lines(file);
        let mut tracker = Self::default();
        for comment_node in ast::collect_comment_nodes(file) {
            let text = comment_node.text();
            let site = DirectiveSite {
                span: comment_node.span(),
                coord: comment_node.start_coordinate(),
            };
            let is_explained_above = || comment_index.has_explanation_above(site.coord.line);
            if let Some(directive) = Self::parse_comment_text(
                &text,
                site,
                content,
                &decorator_end_lines,
                is_explained_above,
            ) {
                tracker.directives.push(directive);
            } else if file.lang() == Language::Python
                && is_unexplained_tool_directive(&text)
                && !is_explained_above()
            {
                tracker.unexplained_tool_directives.push(site);
            }
        }
        tracker
    }

    /// Filters diagnostics against active directives, marking matched rules as used.
    #[must_use]
    pub fn filter_diagnostics(&mut self, diagnostics: Vec<Diagnostic>) -> Vec<Diagnostic> {
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
    pub fn audit(
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
            diagnostics.extend(self.unexplained_tool_directives.iter().map(|site| {
                MISSING_SUPPRESSION_REASON
                    .render_diagnostic(&[], SourceLocation::file_span(path, site.span, site.coord))
            }));
        }
        diagnostics
    }

    /// Helper to parse a single comment's text into a `ParsedDirective` if it is an omni directive.
    ///
    /// `is_explained_above` reports whether the comment block above the directive gives a reason.
    fn parse_comment_text(
        text: &str,
        site: DirectiveSite,
        content: &str,
        decorator_end_lines: &HashMap<usize, usize>,
        is_explained_above: impl FnOnce() -> bool,
    ) -> Option<ParsedDirective> {
        let stripped = strip_comment_delimiters(text)?;
        let DirectivePrefix { is_file, remainder } = parse_directive_prefix(stripped)?;
        let BracketedRules {
            rules: target_rules,
            rest: after_rules,
        } = parse_bracketed_rules(remainder.trim_start());
        let is_blanket = target_rules.is_empty();

        let has_inline_reason = after_rules
            .trim_start()
            .strip_prefix("--")
            .is_some_and(|reason| !reason.trim().is_empty());

        let placement = resolve_directive_placement(
            is_file,
            content,
            decorator_end_lines,
            site.span,
            site.coord.line,
        );
        let matched_count = target_rules.iter().map(|rule| (rule.clone(), 0)).collect();

        Some(ParsedDirective {
            placement,
            span: site.span,
            coord: site.coord,
            target_rules,
            is_explained: has_inline_reason || is_explained_above(),
            is_blanket,
            matched_count,
        })
    }
}

/// A third-party Python suppression directive, written `# <tool>: <keyword>...`.
struct ToolDirective {
    /// The tool namespace before the colon, such as `type` or `pylint`.
    tool: &'static str,
    /// The keywords after the colon that suppress findings, such as `ignore` or `disable`.
    keywords: &'static [&'static str],
}

/// Third-party Python suppression directives besides bare `# noqa`.
const TOOL_DIRECTIVES: &[ToolDirective] = &[
    ToolDirective {
        tool: "type",
        keywords: &["ignore"],
    },
    ToolDirective {
        tool: "pyright",
        keywords: &["ignore"],
    },
    ToolDirective {
        tool: "ty",
        keywords: &["ignore"],
    },
    ToolDirective {
        tool: "pyrefly",
        keywords: &["ignore"],
    },
    ToolDirective {
        tool: "ruff",
        keywords: &["noqa", "ignore", "disable", "file-ignore"],
    },
    ToolDirective {
        tool: "flake8",
        keywords: &["noqa"],
    },
    ToolDirective {
        tool: "pylint",
        keywords: &["disable", "skip-file"],
    },
];

/// Returns true if a Python comment starts with a third-party suppression directive and has no
/// non-empty `-- <reason>`.
///
/// The reason may follow any directive chained in the same comment, as in
/// `# noqa: F401  # type: ignore[attr-defined] -- <reason>`.
fn is_unexplained_tool_directive(text: &str) -> bool {
    let Some(body) = strip_comment_delimiters(text) else {
        return false;
    };
    let is_directive = starts_with_keyword(body, "noqa")
        || body.split_once(':').is_some_and(|(tool, rest)| {
            TOOL_DIRECTIVES.iter().any(|directive| {
                directive.tool == tool
                    && directive
                        .keywords
                        .iter()
                        .any(|keyword| starts_with_keyword(rest.trim_start(), keyword))
            })
        });
    is_directive
        && body
            .split_once("--")
            .is_none_or(|(_, reason)| reason.trim().is_empty())
}

/// Returns true if `text` starts with `keyword` as a whole word, so `ignore` matches
/// `ignore[code]` and `ignore-errors` but not `ignored`.
fn starts_with_keyword(text: &str, keyword: &str) -> bool {
    text.strip_prefix(keyword).is_some_and(|rest| {
        rest.chars()
            .next()
            .is_none_or(|next| !next.is_alphanumeric() && next != '_')
    })
}

/// The `omni:` or `omnicoach:` prefix of a suppression directive.
struct DirectivePrefix<'a> {
    /// True for `disable-file`, false for `ignore`.
    is_file: bool,
    /// The directive text after the prefix.
    remainder: &'a str,
}

/// Parses the `omni:` or `omnicoach:` (`disable-file` / `ignore`) prefix and validates boundary delimiters.
fn parse_directive_prefix(stripped: &str) -> Option<DirectivePrefix<'_>> {
    let body = DIRECTIVE_NAMESPACES
        .iter()
        .find_map(|namespace| stripped.strip_prefix(namespace))?;
    let (is_file, remainder) = if let Some(rest) = body.strip_prefix("disable-file") {
        (true, rest)
    } else {
        (false, body.strip_prefix("ignore")?)
    };

    // Require boundary delimiter (whitespace, '[', or '-') after directive prefix
    // so ordinary comments like `# omni:ignored by compiler` are not treated as directives.
    if !remainder.is_empty()
        && !remainder.starts_with(|c: char| c.is_whitespace() || c == '[' || c == '-')
    {
        return None;
    }

    Some(DirectivePrefix { is_file, remainder })
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

/// Resolves whether a directive applies to the whole file, the same line, or the following declaration line(s).
fn resolve_directive_placement(
    is_file: bool,
    content: &str,
    decorator_end_lines: &HashMap<usize, usize>,
    span: SourceSpan,
    raw_line: usize,
) -> DirectivePlacement {
    if is_file {
        return DirectivePlacement::File;
    }
    let line_start_offset = content[..span.start].rfind('\n').map_or(0, |idx| idx + 1);
    let prefix_on_line = &content[line_start_offset..span.start];
    if prefix_on_line.trim().is_empty() {
        DirectivePlacement::PrecedingLine {
            target_line: raw_line + 1,
            end_target_line: compute_effective_target_line(content, decorator_end_lines, raw_line),
        }
    } else {
        DirectivePlacement::SameLine { line: raw_line }
    }
}

/// Computes the declaration line after skipping contiguous attributes, decorators, or comments.
///
/// `decorator_end_lines` maps the first line of each decorator or attribute to its last line,
/// so continuation lines of a multiline one are skipped too.
fn compute_effective_target_line(
    content: &str,
    decorator_end_lines: &HashMap<usize, usize>,
    raw_line: usize,
) -> usize {
    let mut current_line = raw_line + 1;
    for (index, line_text) in content.lines().enumerate().skip(raw_line) {
        let line_number = index + 1;
        if line_number < current_line {
            continue;
        }
        let trimmed = line_text.trim();
        if let Some(&end_line) = decorator_end_lines.get(&line_number) {
            current_line = end_line + 1;
        } else if trimmed.starts_with("#[")
            || trimmed.starts_with('@')
            || trimmed.starts_with("//")
            || trimmed.starts_with('#')
        {
            current_line += 1;
        } else {
            break;
        }
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

    if !directive.is_explained
        && config.is_rule_enabled_for_path(MISSING_SUPPRESSION_REASON.name, path)
    {
        diagnostics.push(MISSING_SUPPRESSION_REASON.render_diagnostic(&[], location.clone()));
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
    #[case::omni("let a = 1; // omni:ignore [single-letter-name] -- math variable")]
    #[case::omnicoach("let a = 1; // omnicoach:ignore [single-letter-name] -- math variable")]
    fn test_parse_valid_inline_directive_same_line(#[case] content: &str) {
        let file = ParsedFile::new(content, Language::Rust);
        let tracker = SuppressionTracker::from_file(&file, content);
        let directive = &tracker.directives[0];

        assert_eq!(tracker.directives.len(), 1);
        assert_eq!(directive.target_rules, vec!["single-letter-name"]);
        assert!(directive.is_explained);
        assert_eq!(
            directive.placement,
            DirectivePlacement::SameLine { line: 1 }
        );
    }

    #[rstest]
    #[case::direct_declaration(
        "# omni:ignore [nested-function] -- required for fixture\ndef inner(): pass",
        Language::Python,
        2,
        2
    )]
    #[case::skips_python_decorator_and_comment(
        "# omni:ignore [nested-function] -- required for fixture\n@decorator\n# note\ndef inner(): pass",
        Language::Python,
        2,
        4
    )]
    #[case::skips_multiline_python_decorators_and_comment(
        "# omni:ignore [nested-function] -- required for fixture\n@decorator_one(\n    \"a\",\n    \"b\",\n)\n# note\n@decorator_two(\n    \"c\",\n)\ndef inner(): pass",
        Language::Python,
        2,
        10
    )]
    #[case::skips_rust_attribute_and_comment(
        "// omni:ignore [single-letter-name] -- math\n#[allow(dead_code)]\n// note\nfn f(a: i32) {}",
        Language::Rust,
        2,
        4
    )]
    #[case::skips_multiline_rust_attributes_and_comment(
        "// omni:ignore [single-letter-name] -- math\n#[cfg_attr(\n    test,\n    allow(dead_code)\n)]\n// note\n#[derive(\n    Debug\n)]\nfn f(a: i32) {}",
        Language::Rust,
        2,
        10
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
        assert!(directive.is_explained);
        assert_eq!(directive.placement, DirectivePlacement::File);
    }

    #[rstest]
    #[case::missing_brackets("let a = 1; // omni:ignore -- missing brackets")]
    #[case::empty_brackets("let a = 1; // omni:ignore [] -- empty brackets")]
    #[case::unclosed_bracket("let a = 1; // omni:ignore [single-letter-name -- unclosed bracket")]
    fn test_blanket_directive_detected(#[case] content: &str) {
        let file = ParsedFile::new(content, Language::Rust);
        let tracker = SuppressionTracker::from_file(&file, content);

        assert_eq!(tracker.directives.len(), 1);
        assert!(tracker.directives[0].is_blanket);
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
    fn test_unexplained_tool_directive_detected(#[case] comment: &str) {
        assert!(is_unexplained_tool_directive(comment));
    }

    #[rstest]
    #[case::inline_reason("# type: ignore[attr-defined] -- set by the plugin loader")]
    #[case::chained_shared_reason("# noqa: F401  # type: ignore[attr-defined] -- re-exported")]
    #[case::reenable("# pylint: enable=protected-access")]
    #[case::type_comment("# type: int")]
    #[case::keyword_prefix("# noquality checks below")]
    #[case::keyword_suffix("# type: ignored on purpose")]
    #[case::directive_mid_comment("# see below  # noqa: E501")]
    #[case::omni_directive("# omni:ignore [nested-function]")]
    fn test_explained_or_non_tool_directive_not_detected(#[case] comment: &str) {
        assert!(!is_unexplained_tool_directive(comment));
    }

    #[rstest]
    #[case::omni_standalone(
        "# The fixture needs a closure over the client.\n# omni:ignore [nested-function]\ndef inner(): pass",
        true
    )]
    #[case::omni_trailing(
        "# Coordinates follow the paper's notation.\na = 1  # omni:ignore [single-letter-name]",
        true
    )]
    #[case::omni_too_short_above(
        "# why\n# omni:ignore [nested-function]\ndef inner(): pass",
        false
    )]
    #[case::omni_trailing_comment_above_is_not_standalone(
        "b = 2  # The fixture needs a closure here.\na = 1  # omni:ignore [single-letter-name]",
        false
    )]
    fn test_omni_directive_explained_above(#[case] content: &str, #[case] expected: bool) {
        let file = ParsedFile::new(content, Language::Python);
        let tracker = SuppressionTracker::from_file(&file, content);

        assert_eq!(tracker.directives.len(), 1);
        assert_eq!(tracker.directives[0].is_explained, expected);
    }

    #[rstest]
    #[case::unexplained("import os  # noqa: F401", Language::Python, 1)]
    #[case::explained_above(
        "# Re-exported so plugins can import it from here.\nimport os  # noqa: F401",
        Language::Python,
        0
    )]
    #[case::string_ignored("TEXT = \"# noqa: F401\"", Language::Python, 0)]
    #[case::rust_ignored("let a = 1; // noqa: F401", Language::Rust, 0)]
    fn test_unexplained_tool_directives_collected(
        #[case] content: &str,
        #[case] lang: Language,
        #[case] expected_count: usize,
    ) {
        let file = ParsedFile::new(content, lang);
        let tracker = SuppressionTracker::from_file(&file, content);

        assert_eq!(tracker.unexplained_tool_directives.len(), expected_count);
    }
}
