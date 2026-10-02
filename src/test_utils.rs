//! Test utilities and helpers for snapshot testing.

architecture_component!(TestUtils);

use crate::code_lint::ast::{ParsedFile, detect_language};
use crate::code_lint::contract::CodeRule;
use crate::command_lint::contract::CommandRule;
use crate::command_lint::vcs::JjClient;
use crate::diagnostic::Diagnostic;
use crate::rule_declaration::OptionsDeclaration;
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

/// Executes the rule's `check_file` with its default options (including `RequireExplanation` filtering).
///
/// # Panics
/// Panics if `filename` does not have a recognized file extension (`.py` or `.rs`).
#[must_use]
pub fn run_code_rule<Options: OptionsDeclaration>(
    rule: &CodeRule<Options>,
    source: &str,
    filename: &str,
) -> Vec<Diagnostic> {
    let path = Path::new(filename);
    let lang = detect_language(path).unwrap_or_else(|| {
        panic!("run_code_rule: unsupported extension in test file '{filename}'")
    });
    rule.check_file(path, &ParsedFile::new(source, lang), None)
}

/// Helper to execute `check_command` on a [`CommandRule`] and return its formatted diagnostics snapshot.
#[must_use]
pub fn assert_command_rule_snapshot(
    rule: &CommandRule,
    command_input: &str,
    client: &dyn JjClient,
) -> String {
    let cmd = crate::command_lint::command::InterceptedCommand::parse_all(command_input).remove(0);
    let diags = rule.check_command(&cmd, client);
    format_diagnostics_for_test(&diags)
}

/// Validates that the languages tested in `rule_test!` exactly match the rule's declared languages.
///
/// # Panics
/// Panics if a supported language is missing or an unsupported language is included.
#[track_caller]
pub fn assert_language_completeness<Options: OptionsDeclaration>(
    rule: &CodeRule<Options>,
    tested_languages: &[SupportLang],
) {
    let rule_name = rule.declaration.name.0;
    let supported = rule.declaration.languages;

    for lang in tested_languages {
        assert!(
            supported.contains(lang),
            "rule_test! [{rule_name}]: {lang:?} is not in the rule's languages."
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
pub fn assert_rule_pass<Options: OptionsDeclaration>(
    rule: &CodeRule<Options>,
    lang: SupportLang,
    case_name: &str,
    code: &str,
) {
    let rule_name = rule.declaration.name.0;
    let filename = dummy_filename(lang);
    let diags = run_code_rule(rule, code, filename);
    assert!(
        diags.is_empty(),
        "rule_test! [{rule_name}] ({lang:?}) PASS case '{case_name}' failed:\nExpected 0 diagnostics, got {}:\n{}\nSource:\n{code}",
        diags.len(),
        format_diagnostics_for_test(&diags),
    );
}

/// Asserts that a `fail` test case produces exactly one matching diagnostic.
///
/// Asserts that the produced diagnostic AST span matches `expected_snippet` (or `code.trim()` when
/// `expected_snippet` is `None`), and that the same span is reported in both copies when `code` is
/// repeated twice in one file.
///
/// # Panics
/// Panics if diagnostic count, `rule_name`, normalized AST span slice, or repeated-occurrence
/// spans do not match.
#[track_caller]
pub fn assert_rule_fail<Options: OptionsDeclaration>(
    rule: &CodeRule<Options>,
    lang: SupportLang,
    case_name: &str,
    code: &str,
    expected_snippet: Option<&str>,
) {
    let rule_name = rule.declaration.name.0;
    let filename = dummy_filename(lang);
    let diags = run_code_rule(rule, code, filename);
    let expected_slice = expected_snippet.unwrap_or(code).trim();

    let [diagnostic] = diags.as_slice() else {
        panic!(
            "rule_test! [{rule_name}] ({lang:?}) FAIL case '{case_name}' diagnostic count mismatch:\nExpected exactly 1 diagnostic, got {}:\n{}\nSource:\n{code}",
            diags.len(),
            format_diagnostics_for_test(&diags),
        );
    };

    assert_eq!(
        diagnostic.rule_name.0, rule_name,
        "rule_test! [{rule_name}] ({lang:?}) FAIL case '{case_name}': diagnostic rule_name '{}' does not match rule name.",
        diagnostic.rule_name.0
    );

    let raw_slice = &code[diagnostic.location.span.start..diagnostic.location.span.end];
    let actual_slice = normalize_span_indentation(code, diagnostic.location.span.start, raw_slice);
    assert_eq!(
        actual_slice, expected_slice,
        "rule_test! [{rule_name}] ({lang:?}) FAIL case '{case_name}' flagged span mismatch:\nExpected flagged slice:\n{expected_slice}\nActual flagged slice:\n{actual_slice}\n(Tip: if the flagged AST node is a sub-expression of the test code, add `=> r#\"...\"#` to specify the inner slice.)",
    );

    let span = diagnostic.location.span.start..diagnostic.location.span.end;
    assert_every_occurrence_reported(rule, lang, case_name, code, span);
}

/// Repeats `code` twice in one file and asserts the rule flags the same span in both copies,
/// so a rule that stops after its first match cannot pass.
#[track_caller]
fn assert_every_occurrence_reported<Options: OptionsDeclaration>(
    rule: &CodeRule<Options>,
    lang: SupportLang,
    case_name: &str,
    code: &str,
    span: std::ops::Range<usize>,
) {
    let rule_name = rule.declaration.name.0;
    let repeated = format!("{code}\n{code}");
    let offset = code.len() + 1;
    let diags = run_code_rule(rule, &repeated, dummy_filename(lang));

    let mut actual: Vec<_> = diags
        .iter()
        .map(|diagnostic| diagnostic.location.span.start..diagnostic.location.span.end)
        .collect();
    actual.sort_unstable_by_key(|range| (range.start, range.end));
    let expected = vec![span.clone(), span.start + offset..span.end + offset];
    assert_eq!(
        actual,
        expected,
        "rule_test! [{rule_name}] ({lang:?}) FAIL case '{case_name}' did not report every occurrence when the code is repeated twice in one file:\n{}\nSource:\n{repeated}",
        format_diagnostics_for_test(&diags),
    );
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

/// Asserts each of the rule's documented examples exactly like a `rule_test!` case: the flagged
/// snippet as a `fail` case expecting its `flagged_span`, the fixed snippet as a `pass` case.
///
/// # Panics
/// Panics as [`assert_rule_fail`] and [`assert_rule_pass`] do.
#[track_caller]
pub fn assert_documented_examples<Options: OptionsDeclaration>(rule: &CodeRule<Options>) {
    for example in rule.declaration.doc.examples {
        assert_rule_fail(
            rule,
            example.language,
            "documented example",
            example.flagged,
            Some(example.flagged_span),
        );
        assert_rule_pass(rule, example.language, "documented fix", example.fixed);
    }
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
/// Every rule file ends with one invocation; bespoke `#[test]` / `#[rstest]` functions and
/// hand-written `mod tests` blocks in rule files are rejected by `tests/registry.rs`:
/// ```rust,ignore
/// #[cfg(test)]
/// crate::test_utils::rule_test!(SLEEP_IN_TESTS, {
///     Python => {
///         pass: [injected_clock => "fake_clock.sleep(10)"],
///         fail: [
///             time_sleep => "time.sleep(1)",
///             inside_a_test => "
///                 def test_retry():
///                     time.sleep(1)
///             " => "time.sleep(1)",
///         ],
///     },
///     Rust => { /* one block per declared language */ },
/// });
/// ```
///
/// # What it generates
///
/// - A `pass` / `fail` `#[rstest]` case per entry, named after it.
/// - `language_completeness`: every language in the rule's declaration has a block.
/// - `documented_examples`: the doc's [`crate::rule_declaration::Example`]s, checked by
///   [`assert_documented_examples`].
///
/// # How a `fail` case is checked
///
/// - It must produce exactly one diagnostic. Multi-node cases are not supported; known cases
///   that would need them are nested flagged constructs (a `def` inside a nested `def` in
///   `nested-function`), rules reporting every occurrence inside one node, and per-file
///   aggregation (the `no-repeated-literals` candidate in `ROADMAP.md`). Per-scope
///   aggregation fits when each case sits in its own scope (`repeated-index-access`).
/// - Without `=> r#"..."#`, the whole snippet must be the flagged node. With it, the finding
///   must span exactly that inner slice. Leading indentation of lines `2..N` is normalized,
///   so the slice can be written as a clean `indoc!` string.
/// - The code is also run repeated twice in one file and must report both occurrences, so
///   a rule that stops after its first match (`find` instead of `find_all`, early `return`,
///   stray `break`) fails.
///
/// # Writing cases
///
/// - At minimum: the core antipattern (`fail`) and the canonical fix from the template's
///   suggestion (`pass`).
/// - One behaviour per case (one banned pattern, one exemption, one construct), named after
///   it, so a failing case name pinpoints the regression.
/// - An exemption `pass` case must be flagged if the exemption were removed; confirm once by
///   disabling the exemption and watching the case fail.
/// - An accepted false negative is a `pass` case named `known_gap_*` with a `ROADMAP.md`
///   entry, so fixing it forces the case to be updated.
/// - Do not re-test option parsing or per-language resolution: `rule_declaration` tests it
///   centrally. Do not snapshot template prose: `tests/registry.rs` checks templates, and
///   snapshots are reserved for CLI output (`tests/cli.rs`).
/// - When a case cannot be expressed (e.g. a module-level construct broken by the
///   repeated-occurrence check), unit-test the `ast` / `semantic` helper that computes the
///   fact. Change the harness only for a whole class of rules, with a guardrail so the
///   change cannot hide regressions.
macro_rules! rule_test {
    ($rule:expr, { $($body:tt)* }) => {
        $crate::test_utils::rule_test!(tests: $rule, { $($body)* });
    };
    (
        $mod_name:ident : $rule:expr,
        {
            $(
                $lang:ident => {
                    pass: [
                        $( $pass_name:ident => $pass_code:expr ),+ $(,)?
                    ],
                    fail: [
                        $( $fail_name:ident => $fail_code:expr $( => $snippet:expr )? ),+ $(,)?
                    ] $(,)?
                }
            ),+ $(,)?
        }
    ) => {
        mod $mod_name {
            use super::*;

            #[test]
            fn language_completeness() {
                $crate::test_utils::assert_language_completeness(
                    &$rule,
                    &[$( ::ast_grep_language::SupportLang::$lang ),+],
                );
            }

            #[test]
            fn documented_examples() {
                $crate::test_utils::assert_documented_examples(&$rule);
            }

            #[rstest::rstest]
            $(
                $(
                    #[case::$pass_name(
                        ::ast_grep_language::SupportLang::$lang,
                        stringify!($pass_name),
                        indoc::indoc! { $pass_code },
                    )]
                )+
            )+
            fn pass(
                #[case] lang: ::ast_grep_language::SupportLang,
                #[case] case_name: &str,
                #[case] code: &str,
            ) {
                $crate::test_utils::assert_rule_pass(&$rule, lang, case_name, code);
            }

            #[rstest::rstest]
            $(
                $(
                    #[case::$fail_name(
                        ::ast_grep_language::SupportLang::$lang,
                        stringify!($fail_name),
                        indoc::indoc! { $fail_code },
                        None $( .or(Some(indoc::indoc! { $snippet })) )?,
                    )]
                )+
            )+
            fn fail(
                #[case] lang: ::ast_grep_language::SupportLang,
                #[case] case_name: &str,
                #[case] code: &str,
                #[case] expected_snippet: Option<&str>,
            ) {
                $crate::test_utils::assert_rule_fail(
                    &$rule,
                    lang,
                    case_name,
                    code,
                    expected_snippet,
                );
            }
        }
    };
}
pub(crate) use rule_test;
