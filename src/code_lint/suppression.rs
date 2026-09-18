//! Inline and file-level suppression comment hygiene (`SUPP` family).

use crate::code_lint::{AstNode, CodeRule, SourceDoc};
use crate::core::{Config, Rule};
use crate::diagnostic::{
    Diagnostic, LocationContext, RuleCode, RuleName, SourceLocation, SourceSpan, ViolationMessage,
};
use crate::rules::Tag;
use ast_grep_core::AstGrep;
use ast_grep_language::SupportLang;
use std::collections::{HashMap, HashSet};
use std::path::Path;

/// SUPP-001: Flags suppression directives missing a non-empty explanation reason.
pub struct MissingSuppressionReason;

impl Rule for MissingSuppressionReason {
    fn code(&self) -> RuleCode {
        RuleCode("SUPP-001")
    }

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

/// SUPP-002: Flags suppression directives when no violation occurred for the specified rule.
pub struct UnusedSuppression;

impl Rule for UnusedSuppression {
    fn code(&self) -> RuleCode {
        RuleCode("SUPP-002")
    }

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

/// SUPP-003: Flags suppression directives targeting unknown or non-suppressible rule codes.
pub struct UnknownSuppressionCode;

impl Rule for UnknownSuppressionCode {
    fn code(&self) -> RuleCode {
        RuleCode("SUPP-003")
    }

    fn name(&self) -> RuleName {
        RuleName("unknown-suppression-code")
    }

    fn tags(&self) -> &'static [Tag] {
        &[Tag::Suppression]
    }

    fn supported_languages(&self) -> &'static [SupportLang] {
        &[SupportLang::Python, SupportLang::Rust]
    }
}

impl CodeRule for UnknownSuppressionCode {
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

/// SUPP-004: Flags blanket suppression directives that omit explicit rule codes.
pub struct BlanketSuppression;

impl Rule for BlanketSuppression {
    fn code(&self) -> RuleCode {
        RuleCode("SUPP-004")
    }

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
    /// The 1-indexed line number where the directive comment resides.
    pub raw_line: usize,
    /// Rule codes targeted by the directive (e.g. `["NAME-001"]`).
    pub target_codes: Vec<String>,
    /// Optional explanatory reason provided after `--`.
    pub reason: Option<String>,
    /// Whether the directive omitted bracketed rule codes (`[...]`).
    pub is_blanket: bool,
    /// Counter of violations matched and suppressed for each target code.
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

        let line_index = crate::diagnostic::LineIndex::new(content);
        let mut directives = Vec::new();

        for comment_node in comment_nodes {
            let text = comment_node.text();
            let range = comment_node.range();
            let span = SourceSpan { start: range.start, end: range.end };

            if let Some(directive) = Self::parse_comment_text(&text, span, content, &line_index) {
                directives.push(directive);
            }
        }

        Self { directives }
    }

    /// Helper to parse a single comment's text into a `ParsedDirective` if it is an omni directive.
    fn parse_comment_text(
        text: &str,
        span: SourceSpan,
        content: &str,
        line_index: &crate::diagnostic::LineIndex,
    ) -> Option<ParsedDirective> {
        let raw_line = line_index.lookup(span.start).line;

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

        // Parse bracketed codes and remainder
        let remainder_trimmed = remainder.trim_start();
        let (target_codes, is_blanket, after_codes) = if remainder_trimmed.starts_with('[') {
            remainder_trimmed.find(']').map_or_else(
                || (Vec::new(), true, remainder_trimmed),
                |close_idx| {
                    let raw_codes = &remainder_trimmed[1..close_idx];
                    let codes: Vec<String> = raw_codes
                        .split(',')
                        .map(|segment| segment.trim().to_string())
                        .filter(|segment| !segment.is_empty())
                        .collect();
                    let blanket = codes.is_empty();
                    (codes, blanket, &remainder_trimmed[close_idx + 1..])
                },
            )
        } else {
            (Vec::new(), true, remainder_trimmed)
        };

        // Parse reason after '--'
        let reason = after_codes
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
        for code in &target_codes {
            matched_count.insert(code.clone(), 0);
        }

        Some(ParsedDirective {
            placement,
            span,
            raw_line,
            target_codes,
            reason,
            is_blanket,
            matched_count,
        })
    }

    /// Filters diagnostics against active directives, marking matched codes as used.
    #[must_use]
    pub fn filter_diagnostics(
        &mut self,
        diagnostics: Vec<Diagnostic>,
        content: &str,
    ) -> Vec<Diagnostic> {
        if self.directives.is_empty() {
            return diagnostics;
        }

        let line_index = crate::diagnostic::LineIndex::new(content);
        let mut retained = Vec::new();

        for diagnostic in diagnostics {
            let diagnostic_line = line_index.lookup(diagnostic.location.span.start).line;
            let rule_code = diagnostic.rule_code.0;

            let mut suppressed = false;

            for directive in &mut self.directives {
                let matches_line = match directive.placement {
                    DirectivePlacement::File => true,
                    DirectivePlacement::SameLine { line } => line == diagnostic_line,
                    DirectivePlacement::PrecedingLine { target_line, end_target_line } => {
                        (target_line..=end_target_line).contains(&diagnostic_line)
                    }
                };

                if matches_line && directive.target_codes.iter().any(|c| c == rule_code) {
                    suppressed = true;
                    if let Some(count) = directive.matched_count.get_mut(rule_code) {
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

    /// Audits all parsed directives and emits `SUPP-*` diagnostics according to configuration.
    #[must_use]
    pub fn audit(&self, path: &Path, _content: &str, config: &Config) -> Vec<Diagnostic> {
        let mut diagnostics = Vec::new();

        let missing_reason_rule = MissingSuppressionReason;
        let unused_rule = UnusedSuppression;
        let unknown_code_rule = UnknownSuppressionCode;
        let blanket_rule = BlanketSuppression;

        let check_missing_reason = config.is_rule_enabled_for_path(&missing_reason_rule, path);
        let check_unused = config.is_rule_enabled_for_path(&unused_rule, path);
        let check_unknown = config.is_rule_enabled_for_path(&unknown_code_rule, path);
        let check_blanket = config.is_rule_enabled_for_path(&blanket_rule, path);

        // Build set of all known suppressible rule codes (code rules excluding SUPP)
        let mut suppressible_codes: HashSet<&'static str> = HashSet::new();
        for rule in crate::rules::CODE_RULES {
            if !rule.tags().contains(&Tag::Suppression) {
                suppressible_codes.insert(rule.code().0);
            }
        }

        let context = LocationContext::File(path.to_path_buf());

        for directive in &self.directives {
            let location = SourceLocation { context: context.clone(), span: directive.span };

            // SUPP-004: Blanket suppression
            if directive.is_blanket && check_blanket {
                diagnostics.push(Diagnostic::new(
                    blanket_rule.code(),
                    blanket_rule.name(),
                    ViolationMessage {
                        summary: "Blanket suppression directives without rule codes are banned.".to_string(),
                        rationale: "Directives must explicitly target rule codes in brackets (e.g. `[CODE]`) to prevent unintended rule suppression.".to_string(),
                        suggestion: "Specify the explicit rule codes in brackets, e.g. `[RULE-CODE] -- reason`.".to_string(),
                    },
                    location.clone(),
                ));
            }

            // SUPP-001: Missing or empty explanation reason
            if check_missing_reason && directive.reason.is_none() {
                diagnostics.push(Diagnostic::new(
                    missing_reason_rule.code(),
                    missing_reason_rule.name(),
                    ViolationMessage {
                        summary: "Suppression directive is missing an explanation reason.".to_string(),
                        rationale: "Suppression directives must include an explanation via '-- <reason>' to ensure code review accountability.".to_string(),
                        suggestion: "Add '-- <reason>' after the rule codes explaining why this suppression is necessary.".to_string(),
                    },
                    location.clone(),
                ));
            }

            // SUPP-003: Unknown or non-suppressible rule code
            if check_unknown {
                for code in &directive.target_codes {
                    if !suppressible_codes.contains(code.as_str()) {
                        diagnostics.push(Diagnostic::new(
                            unknown_code_rule.code(),
                            unknown_code_rule.name(),
                            ViolationMessage {
                                summary: format!("Unknown rule code `{code}` in suppression directive."),
                                rationale: "The specified rule code is not registered as a suppressible rule in Omni.".to_string(),
                                suggestion: "Verify the rule code spelling or check if the rule is registered.".to_string(),
                            },
                            location.clone(),
                        ));
                    }
                }
            }

            // SUPP-002: Unused suppression
            if check_unused && !directive.is_blanket {
                for code in &directive.target_codes {
                    // Only flag known codes as unused (avoid redundant dual flagging with 003)
                    if suppressible_codes.contains(code.as_str())
                        && directive.matched_count.get(code).copied().unwrap_or(0) == 0
                    {
                        diagnostics.push(Diagnostic::new(
                            unused_rule.code(),
                            unused_rule.name(),
                            ViolationMessage {
                                summary: format!("Suppression directive for rule `{code}` is unused."),
                                rationale: "No violation occurred for this rule; obsolete suppressions cause dead comments and confusion.".to_string(),
                                suggestion: format!("Remove `{code}` from the suppression directive."),
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
        let content = "let a = 1; // omni:ignore [NAME-001] -- math variable";
        let grep = AstGrep::new(content, SupportLang::Rust);
        let tracker = SuppressionTracker::from_ast(&grep, content);

        assert_eq!(tracker.directives.len(), 1);
        let directive = &tracker.directives[0];
        assert_eq!(directive.target_codes, vec!["NAME-001"]);
        assert_eq!(
            (directive.reason.as_deref(), directive.is_blanket),
            (Some("math variable"), false)
        );
        assert_eq!(directive.placement, DirectivePlacement::SameLine { line: 1 });
    }

    #[test]
    fn test_parse_valid_inline_directive_preceding_line() {
        let content = "# omni:ignore [SCOPE-001] -- required for fixture\ndef inner(): pass";
        let grep = AstGrep::new(content, SupportLang::Python);
        let tracker = SuppressionTracker::from_ast(&grep, content);

        assert_eq!(tracker.directives.len(), 1);
        let directive = &tracker.directives[0];
        assert_eq!(directive.target_codes, vec!["SCOPE-001"]);
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
            "# omni:disable-file [SCOPE-001, NAME-001] -- legacy generated file\ndef foo(): pass";
        let grep = AstGrep::new(content, SupportLang::Python);
        let tracker = SuppressionTracker::from_ast(&grep, content);

        assert_eq!(tracker.directives.len(), 1);
        let directive = &tracker.directives[0];
        assert_eq!(directive.target_codes, vec!["SCOPE-001", "NAME-001"]);
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
        let content = "a = 1  # omni:ignore [NAME-001] -- math variable";
        let config = Config::default();
        let diags = crate::code_lint::lint_file(Path::new("math.py"), content, &config);
        assert!(diags.is_empty(), "Expected 0 diagnostics, got: {diags:?}");
    }

    #[test]
    fn test_valid_preceding_line_suppression_silences_violation() {
        let content = "# omni:ignore [NAME-001] -- math variable\na = 1";
        let config = Config::default();
        let diags = crate::code_lint::lint_file(Path::new("math.py"), content, &config);
        assert!(diags.is_empty(), "Expected 0 diagnostics, got: {diags:?}");
    }

    #[test]
    fn test_unused_suppression_flagged() {
        let content = "clean_name = 1  # omni:ignore [NAME-001] -- math variable";
        let config = Config::default();
        let diags = crate::code_lint::lint_file(Path::new("clean.py"), content, &config);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].rule_code.0, "SUPP-002");
    }

    #[test]
    fn test_missing_reason_flagged() {
        let content = "a = 1  # omni:ignore [NAME-001]";
        let config = Config::default();
        let diags = crate::code_lint::lint_file(Path::new("math.py"), content, &config);
        assert!(diags.iter().any(|d| d.rule_code.0 == "SUPP-001"));
    }

    #[test]
    fn test_empty_reason_flagged() {
        let content = "a = 1  # omni:ignore [NAME-001] --    ";
        let config = Config::default();
        let diags = crate::code_lint::lint_file(Path::new("math.py"), content, &config);
        assert!(diags.iter().any(|d| d.rule_code.0 == "SUPP-001"));
    }

    #[test]
    fn test_unknown_rule_code_flagged() {
        let content = "a = 1  # omni:ignore [NON-EXISTENT-999] -- reason";
        let config = Config::default();
        let diags = crate::code_lint::lint_file(Path::new("math.py"), content, &config);
        assert!(diags.iter().any(|d| d.rule_code.0 == "SUPP-003"));
    }

    #[test]
    fn test_blanket_suppression_flagged() {
        let content = "a = 1  # omni:ignore -- missing rule codes";
        let config = Config::default();
        let diags = crate::code_lint::lint_file(Path::new("math.py"), content, &config);
        assert!(diags.iter().any(|d| d.rule_code.0 == "SUPP-004"));
    }

    #[test]
    fn test_file_level_suppression_targets_specific_rule() {
        let content = indoc::indoc! {r"
            # omni:disable-file [SCOPE-001] -- legacy nested functions
            def outer():
                def inner():
                    a = 1
        "};
        let config = Config::default();
        let diags = crate::code_lint::lint_file(Path::new("src/module.py"), content, &config);

        // SCOPE-001 should be suppressed, but NAME-001 should be reported!
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].rule_code.0, "NAME-001");
    }

    #[test]
    fn test_file_level_unused_suppression_flagged() {
        let content = indoc::indoc! {r"
            # omni:disable-file [LOG-001] -- unused file disable
            def clean():
                pass
        "};
        let config = Config::default();
        let diags = crate::code_lint::lint_file(Path::new("src/module.py"), content, &config);

        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].rule_code.0, "SUPP-002");
    }

    #[test]
    fn test_suppressing_supp_in_config() {
        let content = "clean_name = 1  # omni:ignore [NAME-001] -- intentional dormant suppression";
        let toml_content = r#"
            ignore = ["SUPP-002"]
        "#;
        let config: Config = toml::from_str(toml_content).unwrap();
        let diags = crate::code_lint::lint_file(Path::new("src/template.py"), content, &config);

        assert!(diags.is_empty(), "Expected 0 diagnostics, got: {diags:?}");
    }

    #[test]
    fn test_string_literal_does_not_trigger_suppression() {
        let content = r##"sample_text = "# omni:ignore [NAME-001] -- not a comment""##;
        let config = Config::default();
        let diags = crate::code_lint::lint_file(Path::new("src/test_case.py"), content, &config);

        // Does not trigger SUPP-002 for unused suppression since it's a string literal, not a comment
        assert!(diags.is_empty(), "Expected 0 diagnostics, got: {diags:?}");
    }

    #[test]
    fn test_comment_prefix_word_boundary() {
        // Comments containing 'omni:ignored' should not be treated as omni:ignore directives
        let content = "a = 1  # omni:ignored by other tool";
        let config = Config::default();
        let diags = crate::code_lint::lint_file(Path::new("src/test.py"), content, &config);

        // Should flag NAME-001 violation, and NOT flag SUPP-004 (blanket suppression)
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].rule_code.0, "NAME-001");
    }

    #[test]
    fn test_command_rule_in_code_flagged_as_unknown() {
        // JJ-001 is a command rule and cannot be suppressed in code files
        let content = "a = 1  # omni:ignore [JJ-001] -- invalid code rule";
        let config = Config::default();
        let diags = crate::code_lint::lint_file(Path::new("src/test.py"), content, &config);

        assert!(
            diags.iter().any(|d| d.rule_code.0 == "SUPP-003"),
            "Expected SUPP-003 for non-code rule in code directive, got: {diags:?}"
        );
    }
}
