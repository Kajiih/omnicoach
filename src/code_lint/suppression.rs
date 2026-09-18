//! Inline and file-level suppression comment hygiene.

use crate::code_lint::{AstNode, CodeRule, SourceDoc};
use crate::core::{Config, Rule};
use crate::diagnostic::{
    Diagnostic, LineColumn, RuleName, SourceLocation, SourceSpan, ViolationMessage,
};
use crate::rules::Tag;
use ast_grep_core::AstGrep;
use ast_grep_language::SupportLang;
use std::collections::{HashMap, HashSet};
use std::path::Path;

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
            let start_pos = comment_node.start_pos();
            let coord = LineColumn {
                line: start_pos.line() + 1,
                column: start_pos.column(&comment_node) + 1,
            };

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
        let raw_line = coord.line;

        // Strip comment prefix: '#' or '//' or '/*'
        let trimmed = text.trim();
        let stripped = if let Some(body) = trimmed.strip_prefix("//") {
            body.trim_start()
        } else if let Some(body) = trimmed.strip_prefix('#') {
            body.trim_start()
        } else if let Some(body) = trimmed.strip_prefix("/*") {
            body.trim_start().trim_end_matches("*/").trim_end()
        } else {
            return None;
        };

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

        // Parse bracketed rules and remainder
        let remainder_trimmed = remainder.trim_start();
        let (target_rules, is_blanket, after_rules) = if remainder_trimmed.starts_with('[') {
            remainder_trimmed.find(']').map_or_else(
                || (Vec::new(), true, remainder_trimmed),
                |close_idx| {
                    let raw_rules = &remainder_trimmed[1..close_idx];
                    let rules: Vec<String> = raw_rules
                        .split(',')
                        .map(|segment| segment.trim().to_string())
                        .filter(|segment| !segment.is_empty())
                        .collect();
                    let blanket = rules.is_empty();
                    (rules, blanket, &remainder_trimmed[close_idx + 1..])
                },
            )
        } else {
            (Vec::new(), true, remainder_trimmed)
        };

        // Parse reason after '--'
        let reason = after_rules
            .trim_start()
            .strip_prefix("--")
            .map(str::trim)
            .filter(|reason_text| !reason_text.is_empty())
            .map(ToString::to_string);

        // Determine placement: file vs same-line vs preceding-line
        let placement = if is_file {
            DirectivePlacement::File
        } else {
            // Check if there is non-whitespace preceding this comment on the same line
            let line_start_offset = content[..span.start].rfind('\n').map_or(0, |idx| idx + 1);
            let prefix_on_line = &content[line_start_offset..span.start];
            let is_standalone = prefix_on_line.trim().is_empty();

            if is_standalone {
                DirectivePlacement::PrecedingLine {
                    target_line: raw_line + 1,
                    end_target_line: compute_effective_target_line(content, raw_line),
                }
            } else {
                DirectivePlacement::SameLine { line: raw_line }
            }
        };

        let mut matched_count = HashMap::new();
        for rule in &target_rules {
            matched_count.insert(rule.clone(), 0);
        }

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
                let matches_line = match directive.placement {
                    DirectivePlacement::File => true,
                    DirectivePlacement::SameLine { line } => line == diagnostic_line,
                    DirectivePlacement::PrecedingLine { target_line, end_target_line } => {
                        (target_line..=end_target_line).contains(&diagnostic_line)
                    }
                };

                if matches_line && directive.target_rules.iter().any(|rule| rule == rule_name) {
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
        let mut diagnostics = Vec::new();

        let missing_reason_rule = MissingSuppressionReason;
        let unused_rule = UnusedSuppression;
        let unknown_rule = UnknownSuppressionRule;
        let blanket_rule = BlanketSuppression;

        let check_missing_reason = config.is_rule_enabled_for_path(&missing_reason_rule, path);
        let check_unused = config.is_rule_enabled_for_path(&unused_rule, path);
        let check_unknown = config.is_rule_enabled_for_path(&unknown_rule, path);
        let check_blanket = config.is_rule_enabled_for_path(&blanket_rule, path);

        // Build set of all known suppressible rule names (code rules excluding suppression tags)
        let mut suppressible_rules: HashSet<&'static str> = HashSet::new();
        for rule in crate::rules::CODE_RULES {
            if !rule.tags().contains(&Tag::Suppression) {
                suppressible_rules.insert(rule.name().0);
            }
        }

        for directive in &self.directives {
            let location = SourceLocation::file_span(path, directive.span, directive.coord);

            // Blanket suppression
            if directive.is_blanket && check_blanket {
                diagnostics.push(Diagnostic::new(
                    blanket_rule.name(),
                    ViolationMessage {
                        summary: "Blanket suppression directives without rule names are banned.".to_string(),
                        rationale: "Directives must explicitly target rule names in brackets (e.g. `[rule-name]`) to prevent unintended rule suppression.".to_string(),
                        suggestion: "Specify the explicit rule names in brackets, e.g. `[rule-name] -- reason`.".to_string(),
                    },
                    location.clone(),
                ));
            }

            // Missing or empty explanation reason
            if check_missing_reason && directive.reason.is_none() {
                diagnostics.push(Diagnostic::new(
                    missing_reason_rule.name(),
                    ViolationMessage {
                        summary: "Suppression directive is missing an explanation reason.".to_string(),
                        rationale: "Suppression directives must include an explanation via '-- <reason>' to ensure code review accountability.".to_string(),
                        suggestion: "Add '-- <reason>' after the rule names explaining why this suppression is necessary.".to_string(),
                    },
                    location.clone(),
                ));
            }

            // Unknown or non-suppressible rule
            if check_unknown {
                for target_rule in &directive.target_rules {
                    if !suppressible_rules.contains(target_rule.as_str()) {
                        diagnostics.push(Diagnostic::new(
                            unknown_rule.name(),
                            ViolationMessage {
                                summary: format!("Unknown rule `{target_rule}` in suppression directive."),
                                rationale: "The specified rule is not registered as a suppressible rule in Omni.".to_string(),
                                suggestion: "Verify the rule name spelling or check if the rule is registered.".to_string(),
                            },
                            location.clone(),
                        ));
                    }
                }
            }

            // Unused suppression
            if check_unused && !directive.is_blanket {
                for target_rule in &directive.target_rules {
                    // Only flag known rules as unused (avoid redundant dual flagging with unknown rule)
                    if suppressible_rules.contains(target_rule.as_str())
                        && directive.matched_count.get(target_rule).copied().unwrap_or(0) == 0
                    {
                        diagnostics.push(Diagnostic::new(
                            unused_rule.name(),
                            ViolationMessage {
                                summary: format!("Suppression directive for rule `{target_rule}` is unused."),
                                rationale: "No violation occurred for this rule; obsolete suppressions cause dead comments and confusion.".to_string(),
                                suggestion: format!("Remove `{target_rule}` from the suppression directive."),
                            },
                            location.clone(),
                        ));
                    }
                }
            }
        }

        diagnostics
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
        assert_eq!(directive.placement, DirectivePlacement::SameLine { line: 1 });
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
            DirectivePlacement::PrecedingLine { target_line: 2, end_target_line: 2 }
        );
    }

    #[test]
    fn test_parse_file_level_directive() {
        let content =
            "# omni:disable-file [flat-scope-enforced, single-letter-variable-name] -- legacy generated file\ndef foo(): pass";
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
        assert!(diags.iter().any(|diag| diag.rule_name.0 == "missing-suppression-reason"));
    }

    #[test]
    fn test_empty_reason_flagged() {
        let content = "a = 1  # omni:ignore [single-letter-variable-name] --    ";
        let config = Config::default();
        let diags = crate::code_lint::lint_file(Path::new("math.py"), content, &config);
        assert!(diags.iter().any(|diag| diag.rule_name.0 == "missing-suppression-reason"));
    }

    #[test]
    fn test_unknown_rule_flagged() {
        let content = "a = 1  # omni:ignore [non-existent-rule] -- reason";
        let config = Config::default();
        let diags = crate::code_lint::lint_file(Path::new("math.py"), content, &config);
        assert!(diags.iter().any(|diag| diag.rule_name.0 == "unknown-suppression-rule"));
    }

    #[test]
    fn test_blanket_suppression_flagged() {
        let content = "a = 1  # omni:ignore -- missing rule names";
        let config = Config::default();
        let diags = crate::code_lint::lint_file(Path::new("math.py"), content, &config);
        assert!(diags.iter().any(|diag| diag.rule_name.0 == "blanket-suppression"));
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
            diags.iter().any(|diag| diag.rule_name.0 == "unknown-suppression-rule"),
            "Expected unknown-suppression-rule for non-code rule in code directive, got: {diags:?}"
        );
    }
}
