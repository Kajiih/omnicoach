//! Inline and file-level suppression comment hygiene.

use crate::code_lint::{AstNode, CodeRule, SourceDoc};
use crate::core::{Config, Rule, RuleName};
use crate::diagnostic::{
    Diagnostic, LineColumn, SourceLocation, SourceSpan, ViolationTemplate, violation_template,
};
use crate::rules::Tag;
use ast_grep_core::AstGrep;
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

impl Rule for MissingSuppressionReason {
    fn name(&self) -> RuleName {
        RuleName("missing-suppression-reason")
    }

    fn tags(&self) -> &'static [Tag] {
        &[Tag::Suppression]
    }

    fn supported_languages(&self) -> &'static [SupportLang] {
        &[SupportLang::Python, SupportLang::Rust]
    }

    fn violation_template(&self) -> &'static ViolationTemplate {
        &MISSING_REASON_TEMPLATE
    }
}

impl CodeRule for MissingSuppressionReason {
    fn check_file(
        &self,
        _path: &Path,
        _grep: &AstGrep<SourceDoc>,
        _config: &Config,
    ) -> Vec<Diagnostic> {
        // Evaluated during the suppression tracker audit pass
        Vec::new()
    }
}

const UNUSED_SUPPRESSION_TEMPLATE: ViolationTemplate = violation_template! {
    summary: "Suppression directive for rule `{target_rule}` is unused.",
    rationale: "No violation occurred for this rule; obsolete suppressions cause dead comments and confusion.",
    suggestion: "Remove `{target_rule}` from the suppression directive.",
};

/// Flags suppression directives when no violation occurred for the specified rule.
pub struct UnusedSuppression;

impl Rule for UnusedSuppression {
    fn name(&self) -> RuleName {
        RuleName("unused-suppression")
    }

    fn tags(&self) -> &'static [Tag] {
        &[Tag::Suppression]
    }

    fn supported_languages(&self) -> &'static [SupportLang] {
        &[SupportLang::Python, SupportLang::Rust]
    }

    fn violation_template(&self) -> &'static ViolationTemplate {
        &UNUSED_SUPPRESSION_TEMPLATE
    }
}

impl CodeRule for UnusedSuppression {
    fn check_file(
        &self,
        _path: &Path,
        _grep: &AstGrep<SourceDoc>,
        _config: &Config,
    ) -> Vec<Diagnostic> {
        // Evaluated during the suppression tracker audit pass
        Vec::new()
    }
}

const UNKNOWN_SUPPRESSION_TEMPLATE: ViolationTemplate = violation_template! {
    summary: "Unknown rule `{target_rule}` in suppression directive.",
    rationale: "The specified rule is not registered as a suppressible rule in Omni.",
    suggestion: "Verify the rule name spelling or check if the rule is registered.",
};

/// Flags suppression directives targeting unknown or non-suppressible rules.
pub struct UnknownSuppressionRule;

impl Rule for UnknownSuppressionRule {
    fn name(&self) -> RuleName {
        RuleName("unknown-suppression-rule")
    }

    fn tags(&self) -> &'static [Tag] {
        &[Tag::Suppression]
    }

    fn supported_languages(&self) -> &'static [SupportLang] {
        &[SupportLang::Python, SupportLang::Rust]
    }

    fn violation_template(&self) -> &'static ViolationTemplate {
        &UNKNOWN_SUPPRESSION_TEMPLATE
    }
}

impl CodeRule for UnknownSuppressionRule {
    fn check_file(
        &self,
        _path: &Path,
        _grep: &AstGrep<SourceDoc>,
        _config: &Config,
    ) -> Vec<Diagnostic> {
        // Evaluated during the suppression tracker audit pass
        Vec::new()
    }
}

const BLANKET_SUPPRESSION_TEMPLATE: ViolationTemplate = violation_template! {
    summary: "Blanket suppression directives without rule names are banned.",
    rationale: "Directives must explicitly target rule names in brackets (e.g. `[rule-name]`) to prevent unintended rule suppression.",
    suggestion: "Specify the explicit rule names in brackets, e.g. `[rule-name] -- reason`.",
};

/// Flags blanket suppression directives that omit explicit rule names.
pub struct BlanketSuppression;

impl Rule for BlanketSuppression {
    fn name(&self) -> RuleName {
        RuleName("blanket-suppression")
    }

    fn tags(&self) -> &'static [Tag] {
        &[Tag::Suppression]
    }

    fn supported_languages(&self) -> &'static [SupportLang] {
        &[SupportLang::Python, SupportLang::Rust]
    }

    fn violation_template(&self) -> &'static ViolationTemplate {
        &BLANKET_SUPPRESSION_TEMPLATE
    }
}

impl CodeRule for BlanketSuppression {
    fn check_file(
        &self,
        _path: &Path,
        _grep: &AstGrep<SourceDoc>,
        _config: &Config,
    ) -> Vec<Diagnostic> {
        // Evaluated during the suppression tracker audit pass
        Vec::new()
    }
}

/// The placement scope of a parsed suppression directive.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DirectivePlacement {
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
pub struct ParsedDirective {
    /// The placement scope of the directive.
    pub placement: DirectivePlacement,
    /// The byte span of the comment in the source file.
    pub span: SourceSpan,
    /// The 1-indexed line and column coordinate where the directive comment resides.
    pub coord: LineColumn,
    /// Rule names targeted by the directive (e.g. `["single-letter-variable-name"]`).
    pub target_rules: Vec<String>,
    /// Optional explanatory reason provided after `--`.
    pub reason: Option<String>,
    /// Whether the directive omitted bracketed rule names (`[...]`).
    pub is_blanket: bool,
    /// Counter of violations matched and suppressed for each target rule.
    pub matched_count: HashMap<String, usize>,
}

/// Tracker responsible for collecting directives, filtering diagnostics, and auditing hygiene.
#[derive(Debug, Default)]
pub struct SuppressionTracker {
    /// Collected suppression directives in the file.
    pub directives: Vec<ParsedDirective>,
}

fn collect_comments<'a>(node: &AstNode<'a>, comments: &mut Vec<AstNode<'a>>) {
    let kind = node.kind();
    if kind == "comment" || kind == "line_comment" || kind == "block_comment" {
        comments.push(node.clone());
        return;
    }
    for child in node.children() {
        collect_comments(&child, comments);
    }
}

impl SuppressionTracker {
    /// Parses suppression directives from the AST and file content.
    #[must_use]
    pub fn from_ast(grep: &AstGrep<SourceDoc>, content: &str) -> Self {
        let mut comment_nodes = Vec::new();
        collect_comments(&grep.root(), &mut comment_nodes);

        let mut directives = Vec::new();

        for comment_node in comment_nodes {
            let text = comment_node.text();
            let span = SourceSpan::from_range(comment_node.range());
            let coord = LineColumn::from_node(&comment_node);

            if let Some(directive) = Self::parse_comment_text(&text, span, coord, content) {
                directives.push(directive);
            }
        }

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
    #[must_use]
    pub fn audit(&self, path: &Path, config: &Config) -> Vec<Diagnostic> {
        let suppressible_rules: HashSet<&'static str> = crate::rules::CODE_RULES
            .iter()
            .filter(|rule| !rule.tags().contains(&Tag::Suppression))
            .map(|rule| rule.name().0)
            .collect();

        let mut diagnostics = Vec::new();
        for directive in &self.directives {
            audit_single_directive(
                directive,
                path,
                config,
                &suppressible_rules,
                &mut diagnostics,
            );
        }
        diagnostics
    }
}

/// Strips leading and trailing comment delimiters (`//`, `#`, `/* ... */`).
fn strip_comment_delimiters(text: &str) -> Option<&str> {
    let trimmed = text.trim();
    trimmed
        .strip_prefix("//")
        .or_else(|| trimmed.strip_prefix('#'))
        .map(str::trim_start)
        .or_else(|| {
            trimmed
                .strip_prefix("/*")
                .map(|body| body.trim_start().trim_end_matches("*/").trim_end())
        })
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

    if directive.is_blanket && config.is_rule_enabled_for_path(&BlanketSuppression, path) {
        diagnostics.push(BlanketSuppression.render_diagnostic(&[], location.clone()));
    }

    if directive.reason.is_none()
        && config.is_rule_enabled_for_path(&MissingSuppressionReason, path)
    {
        diagnostics.push(MissingSuppressionReason.render_diagnostic(&[], location.clone()));
    }

    if config.is_rule_enabled_for_path(&UnknownSuppressionRule, path) {
        for target_rule in &directive.target_rules {
            if !suppressible_rules.contains(target_rule.as_str()) {
                diagnostics.push(
                    UnknownSuppressionRule
                        .render_diagnostic(&[("target_rule", target_rule)], location.clone()),
                );
            }
        }
    }

    if !directive.is_blanket && config.is_rule_enabled_for_path(&UnusedSuppression, path) {
        for target_rule in &directive.target_rules {
            if suppressible_rules.contains(target_rule.as_str())
                && directive
                    .matched_count
                    .get(target_rule)
                    .copied()
                    .unwrap_or(0)
                    == 0
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
        let grep = AstGrep::new(content, SupportLang::Rust);
        let tracker = SuppressionTracker::from_ast(&grep, content);

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
        let grep = AstGrep::new(content, SupportLang::Python);
        let tracker = SuppressionTracker::from_ast(&grep, content);

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
        let grep = AstGrep::new(content, SupportLang::Python);
        let tracker = SuppressionTracker::from_ast(&grep, content);

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
        let grep = AstGrep::new(content, SupportLang::Rust);
        let tracker = SuppressionTracker::from_ast(&grep, content);

        assert_eq!(tracker.directives.len(), 1);
        assert!(tracker.directives[0].is_blanket);
    }

    #[test]
    fn test_valid_inline_suppression_silences_violation() {
        let content = "a = 1  # omni:ignore [single-letter-variable-name] -- math variable";
        let config = Config::default();
        let diags = crate::code_lint::lint_file(Path::new("math.py"), content, &config);
        assert!(diags.is_empty(), "Expected 0 diagnostics, got: {diags:?}");
    }

    #[test]
    fn test_valid_preceding_line_suppression_silences_violation() {
        let content = "# omni:ignore [single-letter-variable-name] -- math variable\na = 1";
        let config = Config::default();
        let diags = crate::code_lint::lint_file(Path::new("math.py"), content, &config);
        assert!(diags.is_empty(), "Expected 0 diagnostics, got: {diags:?}");
    }

    #[test]
    fn test_unused_suppression_flagged() {
        let content =
            "clean_name = 1  # omni:ignore [single-letter-variable-name] -- math variable";
        let config = Config::default();
        let diags = crate::code_lint::lint_file(Path::new("clean.py"), content, &config);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].rule_name.0, "unused-suppression");
    }

    #[test]
    fn test_missing_reason_flagged() {
        let content = "a = 1  # omni:ignore [single-letter-variable-name]";
        let config = Config::default();
        let diags = crate::code_lint::lint_file(Path::new("math.py"), content, &config);
        assert!(
            diags
                .iter()
                .any(|diag| diag.rule_name.0 == "missing-suppression-reason")
        );
    }

    #[test]
    fn test_empty_reason_flagged() {
        let content = "a = 1  # omni:ignore [single-letter-variable-name] --    ";
        let config = Config::default();
        let diags = crate::code_lint::lint_file(Path::new("math.py"), content, &config);
        assert!(
            diags
                .iter()
                .any(|diag| diag.rule_name.0 == "missing-suppression-reason")
        );
    }

    #[test]
    fn test_unknown_rule_flagged() {
        let content = "a = 1  # omni:ignore [non-existent-rule] -- reason";
        let config = Config::default();
        let diags = crate::code_lint::lint_file(Path::new("math.py"), content, &config);
        assert!(
            diags
                .iter()
                .any(|diag| diag.rule_name.0 == "unknown-suppression-rule")
        );
    }

    #[test]
    fn test_blanket_suppression_flagged() {
        let content = "a = 1  # omni:ignore -- missing rule names";
        let config = Config::default();
        let diags = crate::code_lint::lint_file(Path::new("math.py"), content, &config);
        assert!(
            diags
                .iter()
                .any(|diag| diag.rule_name.0 == "blanket-suppression")
        );
    }

    #[test]
    fn test_file_level_suppression_targets_specific_rule() {
        let content = indoc::indoc! {r"
            # omni:disable-file [flat-scope-enforced] -- legacy nested functions
            def outer():
                def inner():
                    a = 1
        "};
        let config = Config::default();
        let diags = crate::code_lint::lint_file(Path::new("src/module.py"), content, &config);

        // flat-scope-enforced should be suppressed, but single-letter-variable-name should be reported!
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].rule_name.0, "single-letter-variable-name");
    }

    #[test]
    fn test_file_level_unused_suppression_flagged() {
        let content = indoc::indoc! {r"
            # omni:disable-file [no-logging-in-except] -- unused file disable
            def clean():
                pass
        "};
        let config = Config::default();
        let diags = crate::code_lint::lint_file(Path::new("src/module.py"), content, &config);

        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].rule_name.0, "unused-suppression");
    }

    #[test]
    fn test_suppressing_supp_in_config() {
        let content = "clean_name = 1  # omni:ignore [single-letter-variable-name] -- intentional dormant suppression";
        let toml_content = r#"
            ignore = ["unused-suppression"]
        "#;
        let config: Config = toml::from_str(toml_content).unwrap();
        let diags = crate::code_lint::lint_file(Path::new("src/template.py"), content, &config);

        assert!(diags.is_empty(), "Expected 0 diagnostics, got: {diags:?}");
    }

    #[test]
    fn test_string_literal_does_not_trigger_suppression() {
        let content =
            r##"sample_text = "# omni:ignore [single-letter-variable-name] -- not a comment""##;
        let config = Config::default();
        let diags = crate::code_lint::lint_file(Path::new("src/test_case.py"), content, &config);

        // Does not trigger unused-suppression since it's a string literal, not a comment
        assert!(diags.is_empty(), "Expected 0 diagnostics, got: {diags:?}");
    }

    #[test]
    fn test_comment_prefix_word_boundary() {
        // Comments containing 'omni:ignored' should not be treated as omni:ignore directives
        let content = "a = 1  # omni:ignored by other tool";
        let config = Config::default();
        let diags = crate::code_lint::lint_file(Path::new("src/test.py"), content, &config);

        // Should flag single-letter-variable-name violation, and NOT flag blanket-suppression
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].rule_name.0, "single-letter-variable-name");
    }

    #[test]
    fn test_command_rule_in_code_flagged_as_unknown() {
        // no-edits-on-described-commits is a command rule and cannot be suppressed in code files
        let content = "a = 1  # omni:ignore [no-edits-on-described-commits] -- invalid code rule";
        let config = Config::default();
        let diags = crate::code_lint::lint_file(Path::new("src/test.py"), content, &config);

        assert!(
            diags
                .iter()
                .any(|diag| diag.rule_name.0 == "unknown-suppression-rule"),
            "Expected unknown-suppression-rule for non-code rule in code directive, got: {diags:?}"
        );
    }
}
