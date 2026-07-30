//! Test utilities and helpers for snapshot testing.

#![cfg(test)]


use crate::code_lint::CodeRule;
use crate::command_lint::CommandRule;
use crate::command_lint::vcs::JjClient;
use crate::core::Config;
use crate::diagnostic::{Diagnostic, LineIndex};
use ast_grep_core::AstGrep;
use ast_grep_language::SupportLang;
use std::path::Path;

/// Formats a list of diagnostics to a clean, human-readable simplified snapshot string.
pub fn format_diagnostics_for_test(diagnostics: &[Diagnostic], source_content: &str) -> String {
    let mut sorted_diags = diagnostics.to_vec();
    sorted_diags.sort_by_key(|diagnostic| diagnostic.location.span.start);

    let line_index = LineIndex::new(source_content);
    let mut output = String::new();
    for diagnostic in &sorted_diags {
        let line_col = line_index.lookup(diagnostic.location.span.start);
        output.push_str(&format!(
            "[{}] Line {}, Col {}: {}\n",
            diagnostic.rule_code,
            line_col.line,
            line_col.column,
            diagnostic.message.summary
        ));
    }
    output
}

/// Helper to execute check_file on a CodeRule and return its formatted diagnostics snapshot.
pub fn assert_code_rule_snapshot(
    rule: &impl CodeRule,
    source: &str,
    filename: &str,
) -> String {
    assert_code_rule_snapshot_with_config(rule, source, filename, &Config::default())
}

/// Helper to execute check_file on a CodeRule with custom configuration and return its formatted diagnostics snapshot.
pub fn assert_code_rule_snapshot_with_config(
    rule: &impl CodeRule,
    source: &str,
    filename: &str,
    config: &Config,
) -> String {
    let path = Path::new(filename);
    let lang = match path.extension().and_then(|ext| ext.to_str()) {
        Some("py") => SupportLang::Python,
        Some("rs") => SupportLang::Rust,
        _ => panic!("Unsupported extension in test file: {}", filename),
    };
    let grep = AstGrep::new(source, lang);
    let diags = rule.check_file(path, &grep, config);
    format_diagnostics_for_test(&diags, source)
}

/// Helper to execute check_command on a CommandRule and return its formatted diagnostics snapshot.
pub fn assert_command_rule_snapshot(
    rule: &impl CommandRule,
    command_input: &str,
    client: &dyn JjClient,
    config: &Config,
) -> String {
    let cmd = crate::command_lint::InterceptedCommand::parse_all(command_input).remove(0);
    let diags = rule.check_command(&cmd, client, config);
    format_diagnostics_for_test(&diags, command_input)
}
