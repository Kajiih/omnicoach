//! Inline and file-level suppression comment hygiene.

architecture_component!(CodeSuppressionEngine);

use crate::code_lint::ast::{self, ParsedFile};
use crate::code_lint::semantic::comments::strip_comment_delimiters;
use crate::core::{Config, Detector};
use crate::diagnostic::{
    Diagnostic, LineColumn, RuleName, SourceLocation, SourceSpan, ViolationTemplate,
    violation_template,
};
use crate::rule_documentation::{Reference, RuleDoc};
use crate::rule_taxonomy::{Classification, Consensus, ImpactedQuality, Precision, Rule, Topic};
use ast_grep_language::SupportLang;
use std::collections::{HashMap, HashSet};
use std::path::Path;

const MISSING_REASON_TEMPLATE: ViolationTemplate = violation_template! {
    summary: "Suppression directive is missing an explanation reason.",
    rationale: "Suppression directives must include an explanation via '-- <reason>' to ensure code review accountability.",
    suggestion: "Add '-- <reason>' after the rule names explaining why this suppression is necessary.",
};

/// Flags suppression directives missing a non-empty explanation reason.
pub struct MissingSuppressionReason;

impl MissingSuppressionReason {
    /// The rule's declared facets (ADR 007).
    pub(crate) const CLASSIFICATION: Classification = Classification {
        topics: &[Topic::SUPPRESSION_DIRECTIVES],
        precision: Precision::Exact,
        consensus: Consensus::Opinionated,
        impacted_quality: ImpactedQuality::Maintainability,
    };

    /// The rule's user-facing doc.
    pub(crate) const DOC: RuleDoc = RuleDoc {
        summary: "Requires every suppression directive to give a reason.",
        what_it_does: "Flags `omni:ignore` and `omni:disable-file` directives that do not \
                       end with `-- <reason>`, or whose reason is empty. For example, \
                       `# omni:ignore [flat-scope-enforced]` is flagged, while \
                       `# omni:ignore [flat-scope-enforced] -- required for fixture` is not.",
        why_is_this_bad: "A suppression switches a check off, and only its author knows \
                          why. Without a reason, a reviewer cannot tell a deliberate \
                          exception from a shortcut, and a later reader cannot tell whether \
                          the suppression is still needed. The reason keeps the decision \
                          reviewable, and makes stale suppressions easy to spot and \
                          remove.",
        configuration: &[],
        references: &[Reference {
            title: "Ruff: Error suppression",
            url: "https://docs.astral.sh/ruff/linter/#error-suppression",
        }],
    };
}

impl Detector for MissingSuppressionReason {
    fn name(&self) -> RuleName {
        RuleName("missing-suppression-reason")
    }

    fn supported_languages(&self) -> &'static [SupportLang] {
        &[SupportLang::Python, SupportLang::Rust]
    }

    fn violation_template(&self) -> &'static ViolationTemplate {
        &MISSING_REASON_TEMPLATE
    }
}

const UNUSED_SUPPRESSION_TEMPLATE: ViolationTemplate = violation_template! {
    summary: "Suppression directive for rule `{target_rule}` is unused.",
    rationale: "No violation occurred for this rule; obsolete suppressions cause dead comments and confusion.",
    suggestion: "Remove `{target_rule}` from the suppression directive.",
};

/// Flags suppression directives when no violation occurred for the specified rule.
pub struct UnusedSuppression;

impl UnusedSuppression {
    /// The rule's declared facets (ADR 007).
    pub(crate) const CLASSIFICATION: Classification = Classification {
        topics: &[Topic::SUPPRESSION_DIRECTIVES],
        precision: Precision::Exact,
        consensus: Consensus::Unopinionated,
        impacted_quality: ImpactedQuality::Reliability,
    };

    /// The rule's user-facing doc.
    pub(crate) const DOC: RuleDoc = RuleDoc::TODO;
}

impl Detector for UnusedSuppression {
    fn name(&self) -> RuleName {
        RuleName("unused-suppression")
    }

    fn supported_languages(&self) -> &'static [SupportLang] {
        &[SupportLang::Python, SupportLang::Rust]
    }

    fn violation_template(&self) -> &'static ViolationTemplate {
        &UNUSED_SUPPRESSION_TEMPLATE
    }
}

const UNKNOWN_SUPPRESSION_TEMPLATE: ViolationTemplate = violation_template! {
    summary: "Unknown rule `{target_rule}` in suppression directive.",
    rationale: "The specified rule is not registered as a suppressible rule in Omni.",
    suggestion: "Verify the rule name spelling or check if the rule is registered.",
};

/// Flags suppression directives targeting unknown or non-suppressible rules.
pub struct UnknownSuppressionRule;

impl UnknownSuppressionRule {
    /// The rule's declared facets (ADR 007).
    pub(crate) const CLASSIFICATION: Classification = Classification {
        topics: &[Topic::SUPPRESSION_DIRECTIVES],
        precision: Precision::Exact,
        consensus: Consensus::Unopinionated,
        impacted_quality: ImpactedQuality::Reliability,
    };

    /// The rule's user-facing doc.
    pub(crate) const DOC: RuleDoc = RuleDoc::TODO;
}

impl Detector for UnknownSuppressionRule {
    fn name(&self) -> RuleName {
        RuleName("unknown-suppression-rule")
    }

    fn supported_languages(&self) -> &'static [SupportLang] {
        &[SupportLang::Python, SupportLang::Rust]
    }

    fn violation_template(&self) -> &'static ViolationTemplate {
        &UNKNOWN_SUPPRESSION_TEMPLATE
    }
}

const BLANKET_SUPPRESSION_TEMPLATE: ViolationTemplate = violation_template! {
    summary: "Blanket suppression directives without rule names are banned.",
    rationale: "Directives must explicitly target rule names in brackets (e.g. `[rule-name]`) to prevent unintended rule suppression.",
    suggestion: "Specify the explicit rule names in brackets, e.g. `[rule-name] -- reason`.",
};

/// Flags blanket suppression directives that omit explicit rule names.
pub struct BlanketSuppression;

impl BlanketSuppression {
    /// The rule's declared facets (ADR 007).
    pub(crate) const CLASSIFICATION: Classification = Classification {
        topics: &[Topic::SUPPRESSION_DIRECTIVES],
        precision: Precision::Exact,
        consensus: Consensus::Unopinionated,
        impacted_quality: ImpactedQuality::Reliability,
    };

    /// The rule's user-facing doc.
    pub(crate) const DOC: RuleDoc = RuleDoc::TODO;
}

impl Detector for BlanketSuppression {
    fn name(&self) -> RuleName {
        RuleName("blanket-suppression")
    }

    fn supported_languages(&self) -> &'static [SupportLang] {
        &[SupportLang::Python, SupportLang::Rust]
    }

    fn violation_template(&self) -> &'static ViolationTemplate {
        &BLANKET_SUPPRESSION_TEMPLATE
    }
}

/// Static list of the suppression audits with their classifications, evaluated by [`SuppressionTracker::audit`] rather than
/// per file like code rules.
pub const SUPPRESSION_AUDITS: &[Rule<dyn Detector>] = &[
    Rule {
        detector: &MissingSuppressionReason,
        classification: MissingSuppressionReason::CLASSIFICATION,
        doc: MissingSuppressionReason::DOC,
    },
    Rule {
        detector: &UnusedSuppression,
        classification: UnusedSuppression::CLASSIFICATION,
        doc: UnusedSuppression::DOC,
    },
    Rule {
        detector: &UnknownSuppressionRule,
        classification: UnknownSuppressionRule::CLASSIFICATION,
        doc: UnknownSuppressionRule::DOC,
    },
    Rule {
        detector: &BlanketSuppression,
        classification: BlanketSuppression::CLASSIFICATION,
        doc: BlanketSuppression::DOC,
    },
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
    /// Rule names targeted by the directive (e.g. `["single-letter-variable-name"]`).
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
    /// `suppressible_rules` holds the names of every registered non-suppression code rule; it is
    /// supplied by the caller so this module stays independent of the rule registry.
    #[must_use]
    pub fn audit(
        &self,
        path: &Path,
        config: &Config,
        suppressible_rules: &HashSet<&'static str>,
    ) -> Vec<Diagnostic> {
        let mut diagnostics = Vec::new();
        for directive in &self.directives {
            audit_single_directive(
                directive,
                path,
                config,
                suppressible_rules,
                &mut diagnostics,
            );
        }
        diagnostics
    }
}

/// Parses the `omni:disable-file` or `omni:ignore` prefix and validates boundary delimiters.
fn parse_directive_prefix(stripped: &str) -> Option<(bool, &str)> {
    let (is_file, remainder) = if let Some(rest) = stripped.strip_prefix("omni:disable-file") {
        (true, rest)
    } else if let Some(rest) = stripped.strip_prefix("omni:ignore") {
        (false, rest)
    } else {
        return None;
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

/// Audits a single `ParsedDirective` for blanket usage, missing reason, unknown rule names, and unused targets.
fn audit_single_directive(
    directive: &ParsedDirective,
    path: &Path,
    config: &Config,
    suppressible_rules: &HashSet<&'static str>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let location = SourceLocation::file_span(path, directive.span, directive.coord);

    if directive.is_blanket && config.is_rule_enabled_for_path(BlanketSuppression.name(), path) {
        diagnostics.push(BlanketSuppression.render_diagnostic(&[], location.clone()));
    }

    if directive.reason.is_none()
        && config.is_rule_enabled_for_path(MissingSuppressionReason.name(), path)
    {
        diagnostics.push(MissingSuppressionReason.render_diagnostic(&[], location.clone()));
    }

    if config.is_rule_enabled_for_path(UnknownSuppressionRule.name(), path) {
        for target_rule in &directive.target_rules {
            if !suppressible_rules.contains(target_rule.as_str()) {
                diagnostics.push(
                    UnknownSuppressionRule
                        .render_diagnostic(&[("target_rule", target_rule)], location.clone()),
                );
            }
        }
    }

    if !directive.is_blanket && config.is_rule_enabled_for_path(UnusedSuppression.name(), path) {
        for target_rule in &directive.target_rules {
            if suppressible_rules.contains(target_rule.as_str())
                && directive
                    .matched_count
                    .get(target_rule)
                    .is_none_or(|&count| count == 0)
            {
                diagnostics.push(
                    UnusedSuppression
                        .render_diagnostic(&[("target_rule", target_rule)], location.clone()),
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ast_grep_language::SupportLang;

    #[test]
    fn test_parse_valid_inline_directive_same_line() {
        let content = "let a = 1; // omni:ignore [single-letter-variable-name] -- math variable";
        let file = ParsedFile::new(content, SupportLang::Rust);
        let tracker = SuppressionTracker::from_file(&file, content);

        assert_eq!(tracker.directives.len(), 1);
        let directive = &tracker.directives[0];
        assert_eq!(directive.target_rules, vec!["single-letter-variable-name"]);
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
        let content =
            "# omni:ignore [flat-scope-enforced] -- required for fixture\ndef inner(): pass";
        let file = ParsedFile::new(content, SupportLang::Python);
        let tracker = SuppressionTracker::from_file(&file, content);

        assert_eq!(tracker.directives.len(), 1);
        let directive = &tracker.directives[0];
        assert_eq!(directive.target_rules, vec!["flat-scope-enforced"]);
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
        let content = "# omni:disable-file [flat-scope-enforced, single-letter-variable-name] -- legacy generated file\ndef foo(): pass";
        let file = ParsedFile::new(content, SupportLang::Python);
        let tracker = SuppressionTracker::from_file(&file, content);

        assert_eq!(tracker.directives.len(), 1);
        let directive = &tracker.directives[0];
        assert_eq!(
            directive.target_rules,
            vec!["flat-scope-enforced", "single-letter-variable-name"]
        );
        assert_eq!(directive.reason.as_deref(), Some("legacy generated file"));
        assert_eq!(directive.placement, DirectivePlacement::File);
    }

    #[test]
    fn test_blanket_directive_detected() {
        let content = "let a = 1; // omni:ignore -- missing brackets";
        let file = ParsedFile::new(content, SupportLang::Rust);
        let tracker = SuppressionTracker::from_file(&file, content);

        assert_eq!(tracker.directives.len(), 1);
        assert!(tracker.directives[0].is_blanket);
    }
}
