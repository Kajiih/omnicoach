//! Test utilities and helpers for snapshot testing.

use crate::code_lint::CodeRule;
use crate::command_lint::CommandRule;
use crate::command_lint::vcs::JjClient;
use crate::core::Config;
use crate::diagnostic::Diagnostic;
use ast_grep_core::AstGrep;
use ast_grep_language::SupportLang;
use std::fmt::Write;
use std::path::Path;

/// Formats a list of diagnostics to a clean, human-readable simplified snapshot string.
#[must_use]
pub fn format_diagnostics_for_test(diagnostics: &[Diagnostic]) -> String {
    let mut sorted_diags = diagnostics.to_vec();
    sorted_diags.sort_unstable();

    let mut output = String::new();
    for diagnostic in &sorted_diags {
        let _ = writeln!(
            output,
            "[{}] Line {}, Col {}: {}",
            diagnostic.rule_name,
            diagnostic.location.line,
            diagnostic.location.column,
            diagnostic.message.summary
        );
    }
    output
}

/// Helper to execute `check_file` on a `CodeRule` and return its formatted diagnostics snapshot.
#[must_use]
pub fn assert_code_rule_snapshot(rule: &impl CodeRule, source: &str, filename: &str) -> String {
    assert_code_rule_snapshot_with_config(rule, source, filename, &Config::default())
}

/// Executes `check_file` on a `CodeRule` (including `RequireExplanation` filtering).
#[must_use]
pub fn run_code_rule(
    rule: &impl CodeRule,
    source: &str,
    filename: &str,
    config: &Config,
) -> Vec<Diagnostic> {
    let path = Path::new(filename);
    let lang = match path.extension().and_then(|extension| extension.to_str()) {
        Some("py") => SupportLang::Python,
        Some("rs") => SupportLang::Rust,
        _ => panic!("run_code_rule: unsupported extension in test file '{filename}'"),
    };
    let grep = AstGrep::new(source, lang);
    let mode = rule.enforcement_mode(lang, config);
    let mut diags = rule.check_file(path, &grep, config);
    if mode == crate::core::EnforcementMode::RequireExplanation {
        let index = crate::code_lint::comments::CommentIndex::from_ast(&grep);
        let root = grep.root();
        diags.retain(|diagnostic| {
            !index.has_explanation_for_span(
                &root,
                diagnostic.location.span,
                diagnostic.location.line,
            )
        });
    }
    diags
}

/// Helper to execute `check_file` on a `CodeRule` with custom configuration and return its formatted diagnostics snapshot.
#[must_use]
pub fn assert_code_rule_snapshot_with_config(
    rule: &impl CodeRule,
    source: &str,
    filename: &str,
    config: &Config,
) -> String {
    let diags = run_code_rule(rule, source, filename, config);
    format_diagnostics_for_test(&diags)
}

/// Helper to execute `check_command` on a `CommandRule` and return its formatted diagnostics snapshot.
#[must_use]
pub fn assert_command_rule_snapshot(
    rule: &impl CommandRule,
    command_input: &str,
    client: &dyn JjClient,
    config: &Config,
) -> String {
    let cmd = crate::command_lint::InterceptedCommand::parse_all(command_input).remove(0);
    let diags = rule.check_command(&cmd, client, config);
    format_diagnostics_for_test(&diags)
}

/// Re-export `SupportLang` for `rule_test!` macro expansion.
pub use ast_grep_language::SupportLang as MacroSupportLang;

/// Validates that the languages tested in `rule_test!` exactly match `rule.supported_languages()`.
///
/// # Panics
/// Panics if a supported language is missing or an unsupported language is included.
#[track_caller]
pub fn assert_language_completeness(rule: &impl CodeRule, tested_languages: &[SupportLang]) {
    let rule_name = rule.name().0;
    let supported = rule.supported_languages();

    for lang in tested_languages {
        assert!(
            supported.contains(lang),
            "rule_test! [{rule_name}]: {lang:?} is not in supported_languages()."
        );
    }

    for lang in supported {
        assert!(
            tested_languages.contains(lang),
            "rule_test! [{rule_name}]: Rule supports {lang:?}, but it is missing from rule_test!."
        );
    }
}

/// Asserts that a `pass` test case produces zero diagnostics.
///
/// # Panics
/// Panics if the rule emits any diagnostics on `code`.
#[track_caller]
pub fn assert_rule_pass(rule: &impl CodeRule, lang: SupportLang, case_name: &str, code: &str) {
    let rule_name = rule.name().0;
    let filename = dummy_filename(lang);
    let diags = run_code_rule(rule, code, filename, &Config::default());
    assert!(
        diags.is_empty(),
        "rule_test! [{rule_name}] ({lang:?}) PASS case '{case_name}' failed:\nExpected 0 diagnostics, got {}:\n{}\nSource:\n{code}",
        diags.len(),
        format_diagnostics_for_test(&diags),
    );
}

/// Asserts that a `fail` test case produces diagnostics whose AST spans match `expected_snippets`
/// (or `code.trim()` when `expected_snippets` is empty).
///
/// # Panics
/// Panics if diagnostic count, `rule_name`, or normalized AST span slice does not match.
#[track_caller]
pub fn assert_rule_fail(
    rule: &impl CodeRule,
    lang: SupportLang,
    case_name: &str,
    code: &str,
    expected_snippets: &[&str],
) {
    let rule_name = rule.name().0;
    let filename = dummy_filename(lang);
    let diags = run_code_rule(rule, code, filename, &Config::default());

    let expected_slices: Vec<&str> = if expected_snippets.is_empty() {
        vec![code.trim()]
    } else {
        expected_snippets
            .iter()
            .map(|snippet| snippet.trim())
            .collect()
    };

    assert_eq!(
        diags.len(),
        expected_slices.len(),
        "rule_test! [{rule_name}] ({lang:?}) FAIL case '{case_name}' diagnostic count mismatch:\nExpected {} diagnostic(s), got {}:\n{}\nSource:\n{code}",
        expected_slices.len(),
        diags.len(),
        format_diagnostics_for_test(&diags),
    );

    for (diagnostic, expected_slice) in diags.iter().zip(&expected_slices) {
        assert_eq!(
            diagnostic.rule_name.0, rule_name,
            "rule_test! [{rule_name}] ({lang:?}) FAIL case '{case_name}': diagnostic rule_name '{}' does not match rule name.",
            diagnostic.rule_name.0
        );

        let raw_slice = &code[diagnostic.location.span.start..diagnostic.location.span.end];
        let actual_slice =
            normalize_span_indentation(code, diagnostic.location.span.start, raw_slice);
        assert_eq!(
            actual_slice, *expected_slice,
            "rule_test! [{rule_name}] ({lang:?}) FAIL case '{case_name}' flagged span mismatch:\nExpected flagged slice:\n{expected_slice}\nActual flagged slice:\n{actual_slice}\n(Tip: if the flagged AST node is a sub-expression of the test code, add `=> [r#\"...\"#]` to specify the inner slice.)",
        );
    }
}

/// Strips the enclosing line's leading indentation from lines `2..N` of a multiline AST slice
/// so expected snippets written with `indoc!` match regardless of surrounding block nesting.
fn normalize_span_indentation(code: &str, span_start: usize, raw_slice: &str) -> String {
    let line_start = code[..span_start].rfind('\n').map_or(0, |pos| pos + 1);
    let base_indent = code[line_start..span_start]
        .bytes()
        .take_while(|byte| *byte == b' ')
        .count();
    if base_indent == 0 || !raw_slice.contains('\n') {
        return raw_slice.to_string();
    }
    let mut lines = raw_slice.split('\n');
    let mut normalized = String::with_capacity(raw_slice.len());
    if let Some(first) = lines.next() {
        normalized.push_str(first);
    }
    for line in lines {
        normalized.push('\n');
        let strip = line
            .bytes()
            .take(base_indent)
            .take_while(|byte| *byte == b' ')
            .count();
        normalized.push_str(&line[strip..]);
    }
    normalized
}

fn dummy_filename(lang: SupportLang) -> &'static str {
    match lang {
        SupportLang::Python => "test.py",
        SupportLang::Rust => "test.rs",
        _ => panic!(
            "rule_test!: no dummy filename mapped for {lang:?}; add one in test_utils::dummy_filename"
        ),
    }
}

/// Declarative macro generating the complete `#[cfg(test)] mod tests` suite for a [`CodeRule`].
///
/// Expands every `pass` and `fail` entry into an independent `#[rstest::rstest]` `#[case]`
/// and generates a `language_completeness` test verifying all `supported_languages()`.
#[macro_export]
macro_rules! rule_test {
    (
        $rule:expr,
        {
            $(
                $lang:ident => {
                    pass: [
                        $( $pass_name:ident => $pass_code:expr ),+ $(,)?
                    ],
                    fail: [
                        $( $fail_name:ident => $fail_code:expr $( => [ $( $snippet:expr ),+ $(,)? ] )? ),+ $(,)?
                    ] $(,)?
                }
            ),+ $(,)?
        }
    ) => {
        mod tests {
            use super::*;

            #[test]
            fn language_completeness() {
                $crate::test_utils::assert_language_completeness(
                    &$rule,
                    &[$( $crate::test_utils::MacroSupportLang::$lang ),+],
                );
            }

            #[rstest::rstest]
            $(
                $(
                    #[case::$pass_name(
                        $crate::test_utils::MacroSupportLang::$lang,
                        stringify!($pass_name),
                        indoc::indoc! { $pass_code },
                    )]
                )+
            )+
            fn pass(
                #[case] lang: $crate::test_utils::MacroSupportLang,
                #[case] case_name: &str,
                #[case] code: &str,
            ) {
                $crate::test_utils::assert_rule_pass(&$rule, lang, case_name, code);
            }

            #[rstest::rstest]
            $(
                $(
                    #[case::$fail_name(
                        $crate::test_utils::MacroSupportLang::$lang,
                        stringify!($fail_name),
                        indoc::indoc! { $fail_code },
                        &[ $( $( indoc::indoc! { $snippet } ),+ )? ],
                    )]
                )+
            )+
            fn fail(
                #[case] lang: $crate::test_utils::MacroSupportLang,
                #[case] case_name: &str,
                #[case] code: &str,
                #[case] expected_snippets: &[&str],
            ) {
                $crate::test_utils::assert_rule_fail(
                    &$rule,
                    lang,
                    case_name,
                    code,
                    expected_snippets,
                );
            }
        }
    };
}
