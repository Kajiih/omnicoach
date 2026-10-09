//! Test utilities and helpers for snapshot testing.

architecture_component!(TestUtils);

use crate::code_lint::ast::{LiteralValue, ParsedFile, collect_literal_occurrences};
use crate::code_lint::contract::CodeRule;
use crate::code_lint::policy::is_trivial_literal;
use crate::command_lint::contract::CommandRule;
use crate::command_lint::vcs::JjClient;
use crate::diagnostic::{Diagnostic, Language};
use crate::rule_declaration::{
    Declaration, EnforcementMode, LanguageDefaults, OptionsDeclaration, RuleOptions,
};
use std::fmt::Write;
use std::path::Path;

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
    let lang = Language::from_path(path).unwrap_or_else(|| {
        panic!("run_code_rule: unsupported extension in test file '{filename}'")
    });
    rule.check_file(path, &ParsedFile::new(source, lang), None)
}

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

/// Asserts that a `pass` test case produces zero diagnostics.
///
/// # Panics
/// Panics if the rule emits any diagnostics on `code`.
#[track_caller]
pub fn assert_rule_pass<Options: OptionsDeclaration>(
    rule: &CodeRule<Options>,
    lang: Language,
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

/// How the repeated-occurrence check of a `fail` case builds the second copy of the code.
#[derive(Clone, Copy, Debug)]
pub enum RepeatCheck {
    /// The second copy is the code unchanged.
    SameCode,
    /// The second copy's non-trivial literals get new values, so each copy forms its own
    /// groups in rules that group equal literals across the file (`repeated-literal`).
    DistinctLiterals,
}

/// Asserts that a `fail` test case produces exactly one matching diagnostic.
///
/// Asserts that the produced diagnostic AST span matches `expected_snippet` (or `code.trim()` when
/// `expected_snippet` is `None`), and that the same span is reported in both copies when `code` is
/// repeated twice in one file (the second copy built as `repeat` says).
///
/// # Panics
/// Panics if diagnostic count, `rule_name`, normalized AST span slice, or repeated-occurrence
/// spans do not match.
#[track_caller]
pub fn assert_rule_fail<Options: OptionsDeclaration>(
    rule: &CodeRule<Options>,
    lang: Language,
    case_name: &str,
    code: &str,
    expected_snippet: Option<&str>,
    repeat: RepeatCheck,
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
    assert_every_occurrence_reported(rule, lang, case_name, code, span, repeat);
}

/// Asserts each of the rule's documented examples exactly like a `rule_test!` case: the flagged
/// snippet as a `fail` case expecting its `flagged_span`, the fixed snippet as a `pass` case.
///
/// For a rule with an enforcement mode, the fixed snippet must also pass in `ban` mode, so a
/// documented fix never relies on an explanatory comment.
///
/// # Panics
/// Panics as [`assert_rule_fail`] and [`assert_rule_pass`] do.
#[track_caller]
pub fn assert_documented_examples<Options: OptionsDeclaration>(
    rule: &CodeRule<Options>,
    repeat: RepeatCheck,
) {
    let declaration = &rule.declaration;
    let banned = CodeRule {
        declaration: Declaration {
            name: declaration.name,
            template: declaration.template,
            languages: declaration.languages,
            options: RuleOptions {
                enforcement_mode: declaration
                    .options
                    .enforcement_mode
                    .map(|_| LanguageDefaults::new(EnforcementMode::Ban, &[])),
                options: declaration.options.options,
            },
            classification: declaration.classification,
            doc: declaration.doc,
        },
        target: rule.target,
        check: rule.check,
    };
    for example in rule.declaration.doc.examples {
        assert_rule_fail(
            rule,
            example.language,
            "documented example",
            example.flagged,
            Some(example.flagged_span),
            repeat,
        );
        assert_rule_pass(rule, example.language, "documented fix", example.fixed);
        assert_rule_pass(
            &banned,
            example.language,
            "documented fix in `ban` mode",
            example.fixed,
        );
    }
}

/// Validates that the languages tested in `rule_test!` exactly match the rule's declared languages.
///
/// # Panics
/// Panics if a supported language is missing or an unsupported language is included.
#[track_caller]
pub fn assert_language_completeness<Options: OptionsDeclaration>(
    rule: &CodeRule<Options>,
    tested_languages: &[Language],
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

/// Repeats `code` twice in one file and asserts the rule flags the same span in both copies,
/// so a rule that stops after its first match cannot pass.
#[track_caller]
fn assert_every_occurrence_reported<Options: OptionsDeclaration>(
    rule: &CodeRule<Options>,
    lang: Language,
    case_name: &str,
    code: &str,
    span: std::ops::Range<usize>,
    repeat: RepeatCheck,
) {
    let rule_name = rule.declaration.name.0;
    let second_copy = match repeat {
        RepeatCheck::SameCode => code.to_owned(),
        RepeatCheck::DistinctLiterals => with_distinct_literals(code, lang).unwrap_or_else(|reason| {
            panic!(
                "rule_test! [{rule_name}] ({lang:?}) FAIL case '{case_name}': cannot give the second copy distinct literals ({reason}); rename a literal in this case.\nSource:\n{code}"
            )
        }),
    };
    let repeated = format!("{code}\n{second_copy}");
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

/// Returns `code` with every non-trivial literal rewritten in place, with the same byte length,
/// to a value absent from `code`; `Err` when equal literals would no longer be equal, distinct
/// ones no longer distinct, or a rewrite no longer parses (binary and octal digits, equal values
/// spelled differently such as `0xFF` and `255`).
///
/// A string toggles the case of its first unescaped ASCII letter, or changes its first digit;
/// a number changes its first digit after the sign and base prefix (see [`changed_digit`]).
fn with_distinct_literals(code: &str, lang: Language) -> Result<String, String> {
    let original = ParsedFile::new(code, lang);
    let original_occurrences = collect_literal_occurrences(&original);
    let mut rewritten = code.to_owned();
    for occurrence in original_occurrences
        .iter()
        .filter(|occurrence| !is_trivial_literal(&occurrence.value))
    {
        let text = occurrence.node.text();
        let offset = character_to_change(&text, &occurrence.value)
            .ok_or_else(|| format!("no ASCII letter or digit to change in `{text}`"))?;
        let position = occurrence.node.span().start + offset;
        let current = char::from(code.as_bytes()[position]);
        let replacement = match occurrence.value {
            LiteralValue::Str(_) | LiteralValue::Bytes(_) if current.is_ascii_lowercase() => {
                current.to_ascii_uppercase()
            }
            LiteralValue::Str(_) | LiteralValue::Bytes(_) if current.is_ascii_uppercase() => {
                current.to_ascii_lowercase()
            }
            _ => changed_digit(current),
        };
        rewritten.replace_range(position..=position, &replacement.to_string());
    }

    let new_file = ParsedFile::new(&rewritten, lang);
    let old_values: Vec<&LiteralValue> = original_occurrences
        .iter()
        .map(|occurrence| &occurrence.value)
        .collect();
    let new_values: Vec<LiteralValue> = collect_literal_occurrences(&new_file)
        .into_iter()
        .map(|occurrence| occurrence.value)
        .collect();
    if new_file.has_syntax_error() || new_values.len() != old_values.len() {
        return Err("a rewritten literal no longer parses as a literal".to_owned());
    }
    for (index, (old, new)) in old_values.iter().zip(&new_values).enumerate() {
        if !is_trivial_literal(old) && old_values.contains(&new) {
            return Err(format!("the new value {new:?} already appears in the case"));
        }
        for (other_old, other_new) in old_values.iter().zip(&new_values).skip(index + 1) {
            if (old == other_old) != (new == other_new) {
                return Err(format!(
                    "{old:?} and {other_old:?} no longer compare as before"
                ));
            }
        }
    }
    Ok(rewritten)
}

/// Returns the byte offset, within a literal's source `text`, of the character to change: the
/// first unescaped ASCII letter or digit after a string's opening quote, or the first digit of
/// a number after its sign and base prefix.
fn character_to_change(text: &str, value: &LiteralValue) -> Option<usize> {
    match value {
        LiteralValue::Str(_) | LiteralValue::Bytes(_) => {
            let content_start = text.find(['"', '\''])?;
            let mut is_escaped = false;
            text[content_start..]
                .char_indices()
                .find_map(|(index, character)| {
                    let is_candidate = !is_escaped && character.is_ascii_alphanumeric();
                    is_escaped = !is_escaped && character == '\\';
                    is_candidate.then_some(content_start + index)
                })
        }
        LiteralValue::Int(_) | LiteralValue::Float(_) => {
            let unsigned_start = text.find(|character: char| character.is_ascii_digit())?;
            let prefix = text
                .get(unsigned_start..unsigned_start + 2)
                .unwrap_or_default();
            let is_hex = prefix.eq_ignore_ascii_case("0x");
            let has_base_prefix = is_hex
                || ["0o", "0b"]
                    .iter()
                    .any(|base| prefix.eq_ignore_ascii_case(base));
            let digits_start = unsigned_start + if has_base_prefix { 2 } else { 0 };
            let in_digits = text[digits_start..].find(|character: char| {
                if is_hex {
                    character.is_ascii_hexdigit()
                } else {
                    character.is_ascii_digit()
                }
            })?;
            Some(digits_start + in_digits)
        }
    }
}

/// Returns a different digit: a decimal digit moves into `3..=9` so the number stays
/// non-trivial (`0`/`1`/`2` and `9` become `3`, others increment), and a hex letter swaps
/// within its pair (`a`/`b`, `c`/`d`, `e`/`f`).
fn changed_digit(digit: char) -> char {
    match digit {
        '0'..='2' | '9' => '3',
        '3'..='8' | 'a' | 'c' | 'e' | 'A' | 'C' | 'E' => char::from(digit as u8 + 1),
        'b' | 'd' | 'f' | 'B' | 'D' | 'F' => char::from(digit as u8 - 1),
        _ => unreachable!("`{digit}` is not a hex digit"),
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

fn dummy_filename(lang: Language) -> &'static str {
    match lang {
        Language::Python => "test.py",
        Language::Rust => "test.rs",
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
///   `nested-function`) and rules reporting every occurrence inside one node. Per-scope
///   aggregation fits when each case sits in its own scope (`repeated-index-access`).
/// - Without `=> r#"..."#`, the whole snippet must be the flagged node. With it, the finding
///   must span exactly that inner slice. Leading indentation of lines `2..N` is normalized,
///   so the slice can be written as a clean `indoc!` string.
/// - The code is also run repeated twice in one file and must report both occurrences, so
///   a rule that stops after its first match (`find` instead of `find_all`, early `return`,
///   stray `break`) fails.
/// - Rules grouping equal literals across the file pass `repeat: DistinctLiterals`
///   ([`RepeatCheck`]): the second copy's literals get new values, so each copy forms its own
///   groups and the check stays as strict. Write `rule_test!(RULE, repeat: DistinctLiterals, { ... })`.
///   The case panics when no such rewrite exists (equal values spelled differently such as
///   `0xFF` and `255`, binary or octal digits); unit-test those values on the `ast` helper.
///
/// # Writing cases
///
/// - At minimum: the core antipattern (`fail`) and the canonical fix from the template's
///   suggestion (`pass`).
/// - One behaviour per case (one banned pattern, one exemption, one construct), named after
///   it, so a failing case name pinpoints the regression.
/// - An exemption `pass` case must be flagged if the exemption were removed. List it in
///   `scripts/exemption_mutations.py` with the edit that disables the exemption, and re-run the
///   script after changing the rule or its helpers: a one-off check goes stale.
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
    ($rule:expr, repeat: $repeat:ident, { $($body:tt)* }) => {
        $crate::test_utils::rule_test!(tests: $rule, repeat: $repeat, { $($body)* });
    };
    ($rule:expr, { $($body:tt)* }) => {
        $crate::test_utils::rule_test!(tests: $rule, repeat: SameCode, { $($body)* });
    };
    ($mod_name:ident : $rule:expr, { $($body:tt)* }) => {
        $crate::test_utils::rule_test!($mod_name: $rule, repeat: SameCode, { $($body)* });
    };
    (
        $mod_name:ident : $rule:expr,
        repeat: $repeat:ident,
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
                    &[$( $crate::diagnostic::Language::$lang ),+],
                );
            }

            #[test]
            fn documented_examples() {
                $crate::test_utils::assert_documented_examples(
                    &$rule,
                    $crate::test_utils::RepeatCheck::$repeat,
                );
            }

            #[rstest::rstest]
            $(
                $(
                    #[case::$pass_name(
                        $crate::diagnostic::Language::$lang,
                        stringify!($pass_name),
                        indoc::indoc! { $pass_code },
                    )]
                )+
            )+
            fn pass(
                #[case] lang: $crate::diagnostic::Language,
                #[case] case_name: &str,
                #[case] code: &str,
            ) {
                $crate::test_utils::assert_rule_pass(&$rule, lang, case_name, code);
            }

            #[rstest::rstest]
            $(
                $(
                    #[case::$fail_name(
                        $crate::diagnostic::Language::$lang,
                        stringify!($fail_name),
                        indoc::indoc! { $fail_code },
                        None $( .or(Some(indoc::indoc! { $snippet })) )?,
                    )]
                )+
            )+
            fn fail(
                #[case] lang: $crate::diagnostic::Language,
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
                    $crate::test_utils::RepeatCheck::$repeat,
                );
            }
        }
    };
}
pub(crate) use rule_test;

#[cfg(test)]
mod tests {
    use super::*;

    #[rstest::rstest]
    #[case::python_strings_and_numbers(
        Language::Python,
        "f('ab', \"ab\", 1_000, -42, 0x1F, 9, 0.5, 2)",
        "f('Ab', \"Ab\", 3_000, -52, 0x3F, 3, 3.5, 2)"
    )]
    #[case::python_hex_letter(Language::Python, "f(0xff)", "f(0xef)")]
    #[case::rust_suffix_and_escape(
        Language::Rust,
        "fn f() { g(30u64, 30, \"\\nab\", b\"by\"); }",
        "fn f() { g(40u64, 40, \"\\nAb\", b\"By\"); }"
    )]
    fn test_with_distinct_literals_rewrites_in_place(
        #[case] lang: Language,
        #[case] code: &str,
        #[case] expected: &str,
    ) {
        assert_eq!(with_distinct_literals(code, lang).as_deref(), Ok(expected));
    }

    #[rstest::rstest]
    #[case::new_value_already_present("f('ab', 'Ab')", "already appears")]
    #[case::equal_values_split("f(31, 0x1F)", "no longer compare")]
    #[case::nothing_to_change("f('日本')", "no ASCII letter or digit")]
    #[case::binary_digit("f(0b11)", "no longer parses")]
    fn test_with_distinct_literals_rejects_ambiguous_cases(
        #[case] code: &str,
        #[case] reason: &str,
    ) {
        let error = with_distinct_literals(code, Language::Python).unwrap_err();
        assert!(error.contains(reason), "unexpected reason: {error}");
    }
}
