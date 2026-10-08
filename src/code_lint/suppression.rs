//! Inline and file-level suppression comment hygiene.

architecture_component!(CodeLintSuppression);

use crate::code_lint::ast::{self, ParsedFile};
use crate::code_lint::semantic::comments::strip_comment_delimiters;
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
    summary: "Suppression directive has no `-- reason`.",
    rationale: "A suppression without a reason cannot be reviewed: nobody can later tell whether it is still justified.",
    suggestion: "Add `-- <reason>` after the rule names, stating why the finding is acceptable here.",
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
            Flags `omni:ignore` and `omni:disable-file` directives that do not end with
            `-- <reason>`, or whose reason is empty. For example, `# omni:ignore [nested-function]`
            is flagged, while `# omni:ignore [nested-function] -- required for fixture` is not."},
        why_is_this_bad: indoc::indoc! {r"
            A suppression switches a check off, and only its author knows why. Without a reason, a
            reviewer cannot tell a deliberate exception from a shortcut, and a later reader cannot
            tell whether the suppression is still needed. The reason keeps the decision reviewable,
            and makes stale suppressions easy to spot and remove."},
        references: &[Reference {
            title: "Ruff: Error suppression",
            url: "https://docs.astral.sh/ruff/linter/#error-suppression",
        }],
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
            Flags each rule named in an `omni:ignore` or `omni:disable-file` directive that produced
            no finding in the directive's scope: its own line for a trailing directive, the next
            line for a directive on its own line (extended over any decorators, attributes or
            comments to the first line of the declaration they belong to), or the whole file. A rule
            that is disabled in the configuration, or does not run on that file, is not checked.
            Unknown rule names and directives without rule names are left to
            `unknown-suppression-rule` and `blanket-suppression`."},
        why_is_this_bad: indoc::indoc! {r"
            A suppression that matches nothing is stale: the code was fixed or moved, or the
            directive sits on the wrong line. It misleads readers into thinking the line breaks a
            rule, and it keeps silencing that rule there, so if the problem comes back it is not
            reported.

            Remove the rule from the directive, or the whole directive when no rule is left."},
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
    suggestion: "Verify the rule name with `--list-rules`, or disable suppression audits in `.omnilint.toml`.",
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
            themselves (`missing-suppression-reason`, `unused-suppression`,
            `unknown-suppression-rule` and `blanket-suppression`) cannot be suppressed by a
            directive, so naming one is flagged too."},
        why_is_this_bad: indoc::indoc! {r"
            A misspelled or obsolete rule name suppresses nothing, but reads as if it did. The
            finding it was meant to hide is still reported, or a reader wrongly assumes a check is
            off. Names of renamed or removed rules pile up as noise.

            Fix the name (`--list-rules` prints every rule name), or remove it from the directive."},
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
            Flags `omni:ignore` and `omni:disable-file` directives with no bracketed rule list, an
            empty list `[]`, or an unclosed `[`. For example, `# omni:ignore -- legacy code` is
            flagged, while `# omni:ignore [nested-function] -- legacy code` is not. Such a directive
            suppresses no finding."},
        why_is_this_bad: indoc::indoc! {r"
            Omni has no catch-all suppression: a directive silences only the rules it names. A
            directive without names does nothing, yet reads as if it switched off every check on the
            line or in the file, which misleads the reader.

            Name the rules to suppress in brackets, followed by a reason: `[rule-name] -- reason`."},
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

/// Computes the declaration line after skipping contiguous attributes, decorators, or comments.
fn compute_effective_target_line(content: &str, raw_line: usize) -> usize {
    let mut current_line = raw_line + 1;
    for line_text in content.lines().skip(raw_line) {
        let trimmed = line_text.trim();
        if trimmed.starts_with("#[")
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
    /// Optional explanatory reason provided after `--`.
    reason: Option<String>,
    /// Whether the directive omitted bracketed rule names (`[...]`).
    is_blanket: bool,
    /// Counter of violations matched and suppressed for each target rule.
    matched_count: HashMap<String, usize>,
}

/// Tracker responsible for collecting directives, filtering diagnostics, and auditing hygiene.
#[derive(Debug, Default)]
pub struct SuppressionTracker {
    /// Collected suppression directives in the file.
    directives: Vec<ParsedDirective>,
}

impl SuppressionTracker {
    /// Parses suppression directives from the parsed file and its content.
    #[must_use]
    pub fn from_file(file: &ParsedFile, content: &str) -> Self {
        // Every directive contains `omni:`; skip the comment-node traversal when none can exist.
        if !content.contains("omni:") {
            return Self::default();
        }

        let directives = ast::collect_comment_nodes(file)
            .into_iter()
            .filter_map(|comment_node| {
                let text = comment_node.text();
                let span = comment_node.span();
                let coord = comment_node.start_coordinate();
                Self::parse_comment_text(&text, span, coord, content)
            })
            .collect();

        Self { directives }
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
        diagnostics
    }

    /// Helper to parse a single comment's text into a `ParsedDirective` if it is an omni directive.
    fn parse_comment_text(
        text: &str,
        span: SourceSpan,
        coord: LineColumn,
        content: &str,
    ) -> Option<ParsedDirective> {
        let stripped = strip_comment_delimiters(text)?;
        let (is_file, remainder) = parse_directive_prefix(stripped)?;
        let (target_rules, is_blanket, after_rules) = parse_bracketed_rules(remainder.trim_start());

        let reason = after_rules
            .trim_start()
            .strip_prefix("--")
            .map(str::trim)
            .filter(|reason_text| !reason_text.is_empty())
            .map(ToString::to_string);

        let placement = resolve_directive_placement(is_file, content, span, coord.line);
        let matched_count = target_rules.iter().map(|rule| (rule.clone(), 0)).collect();

        Some(ParsedDirective {
            placement,
            span,
            coord,
            target_rules,
            reason,
            is_blanket,
            matched_count,
        })
    }
}

/// Parses the `omni:disable-file` or `omni:ignore` prefix and validates boundary delimiters.
fn parse_directive_prefix(stripped: &str) -> Option<(bool, &str)> {
    let (is_file, remainder) = if let Some(rest) = stripped.strip_prefix("omni:disable-file") {
        (true, rest)
    } else {
        (false, stripped.strip_prefix("omni:ignore")?)
    };

    // Require boundary delimiter (whitespace, '[', or '-') after directive prefix
    // so ordinary comments like `# omni:ignored by compiler` are not treated as directives.
    if !remainder.is_empty()
        && !remainder.starts_with(|c: char| c.is_whitespace() || c == '[' || c == '-')
    {
        return None;
    }

    Some((is_file, remainder))
}

/// Parses bracketed rule names `[rule-a, rule-b]` from the directive body.
fn parse_bracketed_rules(remainder_trimmed: &str) -> (Vec<String>, bool, &str) {
    if !remainder_trimmed.starts_with('[') {
        return (Vec::new(), true, remainder_trimmed);
    }
    remainder_trimmed.find(']').map_or_else(
        || (Vec::new(), true, remainder_trimmed),
        |close_idx| {
            let raw_rules = &remainder_trimmed[1..close_idx];
            let rules: Vec<String> = raw_rules
                .split(',')
                .map(|segment| segment.trim().to_string())
                .filter(|segment| !segment.is_empty())
                .collect();
            let is_blanket = rules.is_empty();
            (rules, is_blanket, &remainder_trimmed[close_idx + 1..])
        },
    )
}

/// Resolves whether a directive applies to the whole file, the same line, or the following declaration line(s).
fn resolve_directive_placement(
    is_file: bool,
    content: &str,
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
            end_target_line: compute_effective_target_line(content, raw_line),
        }
    } else {
        DirectivePlacement::SameLine { line: raw_line }
    }
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

    if directive.reason.is_none()
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

    #[test]
    fn test_parse_valid_inline_directive_same_line() {
        let content = "let a = 1; // omni:ignore [single-letter-name] -- math variable";
        let file = ParsedFile::new(content, Language::Rust);
        let tracker = SuppressionTracker::from_file(&file, content);

        assert_eq!(tracker.directives.len(), 1);
        let directive = &tracker.directives[0];
        assert_eq!(directive.target_rules, vec!["single-letter-name"]);
        assert_eq!(
            (directive.reason.as_deref(), directive.is_blanket),
            (Some("math variable"), false)
        );
        assert_eq!(
            directive.placement,
            DirectivePlacement::SameLine { line: 1 }
        );
    }

    #[test]
    fn test_parse_valid_inline_directive_preceding_line() {
        let content = "# omni:ignore [nested-function] -- required for fixture\ndef inner(): pass";
        let file = ParsedFile::new(content, Language::Python);
        let tracker = SuppressionTracker::from_file(&file, content);

        assert_eq!(tracker.directives.len(), 1);
        let directive = &tracker.directives[0];
        assert_eq!(directive.target_rules, vec!["nested-function"]);
        assert_eq!(
            (directive.reason.as_deref(), directive.is_blanket),
            (Some("required for fixture"), false)
        );
        assert_eq!(
            directive.placement,
            DirectivePlacement::PrecedingLine {
                target_line: 2,
                end_target_line: 2
            }
        );
    }

    #[test]
    fn test_parse_file_level_directive() {
        let content = "# omni:disable-file [nested-function, single-letter-name] -- legacy generated file\ndef foo(): pass";
        let file = ParsedFile::new(content, Language::Python);
        let tracker = SuppressionTracker::from_file(&file, content);

        assert_eq!(tracker.directives.len(), 1);
        let directive = &tracker.directives[0];
        assert_eq!(
            directive.target_rules,
            vec!["nested-function", "single-letter-name"]
        );
        assert_eq!(directive.reason.as_deref(), Some("legacy generated file"));
        assert_eq!(directive.placement, DirectivePlacement::File);
    }

    #[test]
    fn test_blanket_directive_detected() {
        let content = "let a = 1; // omni:ignore -- missing brackets";
        let file = ParsedFile::new(content, Language::Rust);
        let tracker = SuppressionTracker::from_file(&file, content);

        assert_eq!(tracker.directives.len(), 1);
        assert!(tracker.directives[0].is_blanket);
    }
}
