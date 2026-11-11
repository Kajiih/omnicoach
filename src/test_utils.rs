//! Test utilities and helpers for snapshot testing.

architecture_component!(TestUtils);

use crate::code_lint::ast::{
    LiteralValue, ParsedFile, collect_comment_nodes, collect_literal_occurrences,
};
use crate::code_lint::contract::{CodeRule, ProjectCodeRule};
use crate::code_lint::policy::is_trivial_literal;
use crate::code_lint::project::{MemoryFileSource, ProjectContext, ProjectIndex, ProjectLayout};
use crate::command_lint::contract::CommandRule;
use crate::command_lint::vcs::JjClient;
use crate::diagnostic::{Diagnostic, Language};
use crate::rule_declaration::{
    Declaration, DeclaredExamples, DeclaredRule, EnforcementMode, LanguageDefaults,
    OptionsDeclaration, RuleOptions, RuleOverrides,
};
use std::collections::HashSet;
use std::fmt::Write;
use std::path::Path;

/// Uniform test-execution adapter implemented by both [`CodeRule`] and [`ProjectCodeRule`].
pub trait TestableCodeRule {
    /// Type-erased rule metadata (`name`, `languages`, `options`, `doc`).
    fn declared_rule(&self) -> DeclaredRule;

    /// True when the rule inspects multi-file [`ProjectContext`].
    fn is_project_rule(&self) -> bool;

    /// Runs the rule in `ban` mode (ignoring explanation comments) on `parsed` within `project`.
    fn run_in_ban_mode(
        &self,
        path: &Path,
        parsed: &ParsedFile,
        project: Option<&ProjectContext<'_>>,
        overrides: Option<&RuleOverrides>,
    ) -> Vec<Diagnostic>;
}

impl<Options: OptionsDeclaration> TestableCodeRule for CodeRule<Options> {
    fn declared_rule(&self) -> DeclaredRule {
        self.declaration.declared()
    }

    fn is_project_rule(&self) -> bool {
        false
    }

    fn run_in_ban_mode(
        &self,
        path: &Path,
        parsed: &ParsedFile,
        _project: Option<&ProjectContext<'_>>,
        overrides: Option<&RuleOverrides>,
    ) -> Vec<Diagnostic> {
        let banned = Self {
            declaration: Declaration {
                options: RuleOptions {
                    enforcement_mode: self
                        .declaration
                        .options
                        .enforcement_mode
                        .map(|_| LanguageDefaults::new(EnforcementMode::Ban, &[])),
                    ..self.declaration.options
                },
                ..self.declaration
            },
            ..*self
        };
        banned.check_file(path, parsed, overrides)
    }
}

impl<Options, Examples> TestableCodeRule for ProjectCodeRule<Options, Examples>
where
    Options: OptionsDeclaration,
    Examples: Copy + Into<DeclaredExamples>,
{
    fn declared_rule(&self) -> DeclaredRule {
        self.declaration.declared()
    }

    fn is_project_rule(&self) -> bool {
        true
    }

    fn run_in_ban_mode(
        &self,
        path: &Path,
        parsed: &ParsedFile,
        project: Option<&ProjectContext<'_>>,
        overrides: Option<&RuleOverrides>,
    ) -> Vec<Diagnostic> {
        let project = project.expect("ProjectCodeRule test execution requires a ProjectContext");
        let banned = Self {
            declaration: Declaration {
                options: RuleOptions {
                    enforcement_mode: self
                        .declaration
                        .options
                        .enforcement_mode
                        .map(|_| LanguageDefaults::new(EnforcementMode::Ban, &[])),
                    ..self.declaration.options
                },
                ..self.declaration
            },
            ..*self
        };
        banned.check_file(path, parsed, project, overrides)
    }
}

/// Input specification for a `pass` entry in [`rule_test!`].
#[derive(Debug, Clone, Copy)]
pub enum PassCaseInput<'a> {
    /// A single standalone snippet.
    Single(&'a str),
    /// A multi-file virtual project `[(relative_path, content), ...]`; all matching files must pass.
    Project(&'a [(&'a str, &'a str)]),
}

/// Input specification for a `fail` entry in [`rule_test!`].
#[derive(Debug, Clone, Copy)]
pub enum FailCaseInput<'a> {
    /// A single standalone snippet expected to produce one diagnostic.
    Single {
        /// Source text of the snippet.
        code: &'a str,
        /// Optional sub-slice spanned by the diagnostic (`None` means full trimmed `code`).
        expected_snippet: Option<&'a str>,
    },
    /// A multi-file virtual project expected to produce one diagnostic on `target_path` and zero
    /// diagnostics on every sibling file.
    Project {
        /// Virtual project files `[(relative_path, content), ...]`.
        files: &'a [(&'a str, &'a str)],
        /// Relative path of the file expected to report the diagnostic.
        target_path: &'a str,
        /// Optional sub-slice of `target_path` spanned by the diagnostic.
        expected_snippet: Option<&'a str>,
    },
}

/// Executes a [`TestableCodeRule`] in `ban` mode on a single snippet `source` (`filename`).
///
/// # Panics
/// Panics if `filename` does not have a recognized file extension (`.py` or `.rs`) or if `source`
/// has a syntax error.
#[must_use]
#[track_caller]
pub fn run_code_rule<R: TestableCodeRule>(
    rule: &R,
    source: &str,
    filename: &str,
    overrides: Option<&RuleOverrides>,
) -> Vec<Diagnostic> {
    let declared = rule.declared_rule();
    let relative_path = Path::new(filename);
    let lang = Language::from_path(relative_path).unwrap_or_else(|| {
        panic!("run_code_rule: unsupported extension in test file '{filename}'")
    });
    let parsed = ParsedFile::new(source, lang);
    assert!(
        !parsed.has_syntax_error(),
        "run_code_rule [{}] ({lang:?}): test snippet has a syntax error:\n{source}",
        declared.name.0,
    );
    if !rule.is_project_rule() {
        return rule.run_in_ban_mode(relative_path, &parsed, None, overrides);
    }
    let root = Path::new(MemoryFileSource::VIRTUAL_ROOT);
    let memory = MemoryFileSource::from_relative_files(root, &[(filename, source)]);
    let full_path = root.join(filename);
    let layout = ProjectLayout::discover(root, &memory, &[]);
    let index = ProjectIndex::new(layout, &memory);
    index.seed_from_parsed(&full_path, &parsed);
    let context = ProjectContext::new(&index);
    rule.run_in_ban_mode(&full_path, &parsed, Some(&context), overrides)
}

/// Builds a [`RuleOverrides`] setting `banned` to `entries` on `rule`, or `None` when `entries`
/// is empty.
///
/// # Panics
/// Panics if `entries` is non-empty and `rule` does not declare a `banned` list option.
// TODO: Investigate if this is ricr to add banned list override.
#[must_use]
#[track_caller]
pub fn banned_list_overrides<R: TestableCodeRule>(
    rule: &R,
    entries: &[&str],
) -> Option<RuleOverrides> {
    if entries.is_empty() {
        return None;
    }
    let declared = rule.declared_rule();
    let items: Vec<toml::Value> = entries
        .iter()
        .map(|entry| toml::Value::String((*entry).to_owned()))
        .collect();
    let mut table = toml::Table::new();
    table.insert("banned".to_owned(), toml::Value::Array(items));
    Some(
        RuleOverrides::parse(
            declared.name.0,
            &declared.options,
            declared.languages,
            &toml::Value::Table(table),
        )
        .expect("banned_list_overrides must be valid for rule"),
    )
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

/// In-memory [`JjClient`] fake for [`command_rule_test!`].
#[derive(Debug, Default)]
pub struct FakeJjClient {
    commits: std::collections::HashMap<String, crate::command_lint::vcs::CommitInfo>,
}

impl FakeJjClient {
    /// Creates a [`FakeJjClient`] populated with `entries`.
    #[must_use]
    pub fn from_entries(entries: &[(&str, crate::command_lint::vcs::CommitInfo)]) -> Self {
        Self {
            commits: entries
                .iter()
                .map(|(revision, info)| ((*revision).to_string(), info.clone()))
                .collect(),
        }
    }
}

impl JjClient for FakeJjClient {
    fn get_commit_info(
        &self,
        revision: &str,
    ) -> Result<crate::command_lint::vcs::CommitInfo, String> {
        self.commits
            .get(revision)
            .cloned()
            .ok_or_else(|| format!("unknown revision: {revision}"))
    }
}

/// Executes `rule` on `command_line`, asserting that `command_line` is valid shell syntax.
///
/// # Panics
/// Panics if `command_line` has a shell syntax error.
#[must_use]
#[track_caller]
pub fn run_command_rule(
    rule: &CommandRule,
    command_line: &str,
    client: &dyn JjClient,
) -> Vec<Diagnostic> {
    let parsed = crate::command_lint::command::ParsedCommand::new(command_line);
    assert!(
        !parsed.has_syntax_error(),
        "run_command_rule [{}]: test command has a shell syntax error:\n{command_line}",
        rule.declaration.name.0,
    );
    let env = crate::command_lint::contract::CommandEnv::new(client);
    rule.check_command(&parsed, &env)
}

/// Asserts that a single-file `pass` test case produces zero diagnostics.
///
/// # Panics
/// Panics if the rule emits any diagnostics on `code`.
#[track_caller]
pub fn assert_rule_pass<R: TestableCodeRule>(
    rule: &R,
    lang: Language,
    case_name: &str,
    code: &str,
    overrides: Option<&RuleOverrides>,
) {
    assert_rule_pass_input(
        rule,
        lang,
        case_name,
        PassCaseInput::Single(code),
        overrides,
    );
}

/// Asserts that a single-file or multi-file `pass` test case produces zero diagnostics.
///
/// # Panics
/// Panics if the rule emits any diagnostics, if a multi-file case is used on a single-file rule,
/// or if any `.py`/`.rs` file in the case has a syntax error.
#[track_caller]
pub fn assert_rule_pass_input<R: TestableCodeRule>(
    rule: &R,
    lang: Language,
    case_name: &str,
    input: PassCaseInput<'_>,
    overrides: Option<&RuleOverrides>,
) {
    let declared = rule.declared_rule();
    let rule_name = declared.name.0;
    match input {
        PassCaseInput::Single(code) => {
            let filename = dummy_filename(lang);
            let diags = run_code_rule(rule, code, filename, overrides);
            assert!(
                diags.is_empty(),
                indoc::indoc! {"
                    rule_test! [{}] ({:?}) PASS case '{}' failed:
                    Expected 0 diagnostics, got {}:
                    {}
                    Source:
                    {}"},
                rule_name,
                lang,
                case_name,
                diags.len(),
                format_diagnostics_for_test(&diags),
                code,
            );
        }
        PassCaseInput::Project(files) => {
            assert!(
                rule.is_project_rule(),
                "rule_test! [{rule_name}] ({lang:?}) PASS case '{case_name}': multi-file `{{ ... }}` cases are only valid on `ProjectCodeRule`s"
            );
            let parsed_files = validate_and_parse_project_files(rule_name, lang, case_name, files);
            let root = Path::new(MemoryFileSource::VIRTUAL_ROOT);
            let memory = MemoryFileSource::from_relative_files(root, files);
            let index = build_virtual_project_index(root, &memory, rule_name, lang, case_name);
            for (relative_path, parsed) in &parsed_files {
                index.seed_from_parsed(&root.join(relative_path), parsed);
            }
            let context = ProjectContext::new(&index);
            let mut checked_any = false;
            for (relative_path, parsed) in &parsed_files {
                if Language::from_path(Path::new(relative_path)) != Some(lang) {
                    continue;
                }
                checked_any = true;
                let full_path = root.join(relative_path);
                let diags = rule.run_in_ban_mode(&full_path, parsed, Some(&context), overrides);
                assert!(
                    diags.is_empty(),
                    indoc::indoc! {"
                        rule_test! [{}] ({:?}) PASS case '{}' failed on '{}':
                        Expected 0 diagnostics, got {}:
                        {}"},
                    rule_name,
                    lang,
                    case_name,
                    relative_path,
                    diags.len(),
                    format_diagnostics_for_test(&diags),
                );
            }
            if let Some(error) = index.architecture_error() {
                panic!(
                    "rule_test! [{rule_name}] ({lang:?}) PASS case '{case_name}': invalid project architecture: {error}"
                );
            }
            assert!(
                checked_any,
                "rule_test! [{rule_name}] ({lang:?}) PASS case '{case_name}': multi-file project contains no {lang:?} source files"
            );
        }
    }
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

/// Asserts that a single-file `fail` test case produces exactly one matching diagnostic.
///
/// # Panics
/// Panics if diagnostic count, `rule_name`, normalized AST span slice, or repeated-occurrence
/// spans do not match.
#[track_caller]
pub fn assert_rule_fail<R: TestableCodeRule>(
    rule: &R,
    lang: Language,
    case_name: &str,
    code: &str,
    expected_snippet: Option<&str>,
    repeat: RepeatCheck,
    overrides: Option<&RuleOverrides>,
) {
    assert_rule_fail_input(
        rule,
        lang,
        case_name,
        FailCaseInput::Single {
            code,
            expected_snippet,
        },
        repeat,
        overrides,
    );
}

/// Asserts that a single-file or multi-file `fail` test case produces exactly one matching
/// diagnostic on the target file (and zero diagnostics on sibling files), plus both occurrences
/// when the target file's contents are repeated twice.
///
/// # Panics
/// Panics if diagnostic count, `rule_name`, target file, normalized AST span slice, comment
/// hygiene, or repeated-occurrence spans do not match.
#[track_caller]
pub fn assert_rule_fail_input<R: TestableCodeRule>(
    rule: &R,
    lang: Language,
    case_name: &str,
    input: FailCaseInput<'_>,
    repeat: RepeatCheck,
    overrides: Option<&RuleOverrides>,
) {
    let declared = rule.declared_rule();
    let rule_name = declared.name.0;
    match input {
        FailCaseInput::Single {
            code,
            expected_snippet,
        } => {
            let filename = dummy_filename(lang);
            let diags = run_code_rule(rule, code, filename, overrides);
            let span = verify_single_diagnostic(
                rule_name,
                lang,
                case_name,
                code,
                expected_snippet,
                &diags,
            );
            assert_no_comment_in_explanation_position(rule_name, lang, case_name, code, span.start);
            assert_every_occurrence_reported(rule, lang, case_name, code, span, repeat, overrides);
        }
        FailCaseInput::Project {
            files,
            target_path,
            expected_snippet,
        } => {
            assert_project_fail_input(
                rule,
                lang,
                case_name,
                files,
                target_path,
                expected_snippet,
                repeat,
                overrides,
            );
        }
    }
}

const DOCUMENTED_EXAMPLE_CASE: &str = "documented example";
const DOCUMENTED_FIX_CASE: &str = "documented fix";

/// Asserts each of the rule's documented examples (`Example` or `ProjectExample`) exactly like a
/// `rule_test!` case.
///
/// # Panics
/// Panics as [`assert_rule_fail_input`] and [`assert_rule_pass_input`] do.
#[track_caller]
pub fn assert_documented_examples<R: TestableCodeRule>(
    rule: &R,
    repeat: RepeatCheck,
    overrides: Option<&RuleOverrides>,
) {
    let declared = rule.declared_rule();
    match declared.doc.examples {
        DeclaredExamples::Code(examples) => {
            for example in examples {
                assert_rule_fail(
                    rule,
                    example.language,
                    DOCUMENTED_EXAMPLE_CASE,
                    example.flagged,
                    Some(example.flagged_span),
                    repeat,
                    overrides,
                );
                assert_rule_pass(
                    rule,
                    example.language,
                    DOCUMENTED_FIX_CASE,
                    example.fixed,
                    overrides,
                );
            }
        }
        DeclaredExamples::Project(examples) => {
            for example in examples {
                let mut flagged_files: Vec<(&str, &str)> = example.context.to_vec();
                flagged_files.push((example.path, example.flagged));
                assert_rule_fail_input(
                    rule,
                    example.language,
                    DOCUMENTED_EXAMPLE_CASE,
                    FailCaseInput::Project {
                        files: &flagged_files,
                        target_path: example.path,
                        expected_snippet: Some(example.flagged_span),
                    },
                    repeat,
                    overrides,
                );

                let mut fixed_files: Vec<(&str, &str)> = example.context.to_vec();
                fixed_files.push((example.path, example.fixed));
                assert_rule_pass_input(
                    rule,
                    example.language,
                    DOCUMENTED_FIX_CASE,
                    PassCaseInput::Project(&fixed_files),
                    overrides,
                );
            }
        }
        DeclaredExamples::Command(_) => {
            panic!(
                "rule_test! [{}]: code rule cannot declare CommandExample",
                declared.name.0
            );
        }
    }
}

/// Validates that the languages tested in `rule_test!` exactly match the rule's declared languages.
///
/// # Panics
/// Panics if a supported language is missing or an unsupported language is included.
#[track_caller]
pub fn assert_language_completeness<R: TestableCodeRule>(rule: &R, tested_languages: &[Language]) {
    let declared = rule.declared_rule();
    let rule_name = declared.name.0;
    let supported = declared.languages;

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

/// Asserts that a command rule `pass` test case produces zero diagnostics (both standalone and
/// when repeated on two lines).
///
/// # Panics
/// Panics if `rule` emits any diagnostics on `command_line` or `"{command_line}\n{command_line}"`.
#[track_caller]
pub fn assert_command_rule_pass(
    rule: &CommandRule,
    case_name: &str,
    command_line: &str,
    client: &dyn JjClient,
) {
    let rule_name = rule.declaration.name.0;
    let diagnostics = run_command_rule(rule, command_line, client);
    assert!(
        diagnostics.is_empty(),
        indoc::indoc! {"
            command_rule_test! [{}] PASS case '{}' failed:
            Expected 0 diagnostics, got {}:
            {}
            Command:
            {}"},
        rule_name,
        case_name,
        diagnostics.len(),
        format_diagnostics_for_test(&diagnostics),
        command_line,
    );

    let repeated = format!("{command_line}\n{command_line}");
    let repeated_diagnostics = run_command_rule(rule, &repeated, client);
    assert!(
        repeated_diagnostics.is_empty(),
        indoc::indoc! {"
            command_rule_test! [{}] PASS case '{}' failed when repeated:
            Expected 0 diagnostics, got {}:
            {}
            Command:
            {}"},
        rule_name,
        case_name,
        repeated_diagnostics.len(),
        format_diagnostics_for_test(&repeated_diagnostics),
        repeated,
    );
}

/// Asserts that a command rule `fail` test case produces exactly one matching diagnostic whose
/// span matches `expected_snippet` (or `command_line.trim()` when `None`), and reports both
/// copies when `command_line` is repeated on two lines.
///
/// # Panics
/// Panics if diagnostic count, `rule_name`, span slice, or repeated-occurrence spans do not match.
#[track_caller]
pub fn assert_command_rule_fail(
    rule: &CommandRule,
    case_name: &str,
    command_line: &str,
    expected_snippet: Option<&str>,
    client: &dyn JjClient,
) {
    let rule_name = rule.declaration.name.0;
    let diagnostics = run_command_rule(rule, command_line, client);
    let expected_slice = expected_snippet.unwrap_or(command_line).trim();

    let [diagnostic] = diagnostics.as_slice() else {
        panic!(
            indoc::indoc! {"
                command_rule_test! [{}] FAIL case '{}' diagnostic count mismatch:
                Expected exactly 1 diagnostic, got {}:
                {}
                Command:
                {}"},
            rule_name,
            case_name,
            diagnostics.len(),
            format_diagnostics_for_test(&diagnostics),
            command_line,
        );
    };

    assert_eq!(
        diagnostic.rule_name.0, rule_name,
        "command_rule_test! [{rule_name}] FAIL case '{case_name}': diagnostic rule_name '{}' does not match rule name.",
        diagnostic.rule_name.0,
    );

    let actual_slice = &command_line[diagnostic.location.span.start..diagnostic.location.span.end];
    assert_eq!(
        actual_slice, expected_slice,
        indoc::indoc! {r#"
            command_rule_test! [{}] FAIL case '{}' flagged span mismatch:
            Expected flagged slice:
            {}
            Actual flagged slice:
            {}
            (Tip: if the flagged command is a sub-span of the test line, add `=> "..."` to specify the inner slice.)"#},
        rule_name, case_name, expected_slice, actual_slice,
    );

    let repeated = format!("{command_line}\n{command_line}");
    let second_offset = command_line.len() + 1;
    let expected_spans = [
        diagnostic.location.span.start..diagnostic.location.span.end,
        (second_offset + diagnostic.location.span.start)
            ..(second_offset + diagnostic.location.span.end),
    ];
    let actual_spans: Vec<_> = run_command_rule(rule, &repeated, client)
        .into_iter()
        .map(|item| item.location.span.start..item.location.span.end)
        .collect();
    assert_eq!(
        actual_spans, expected_spans,
        "command_rule_test! [{rule_name}] FAIL case '{case_name}': repeating the command twice must report both occurrences",
    );
}

/// Asserts each of the command rule's documented [`crate::rule_declaration::CommandExample`]s
/// against `client`.
///
/// # Panics
/// Panics as [`assert_command_rule_fail`] and [`assert_command_rule_pass`] do.
#[track_caller]
pub fn assert_command_documented_examples(rule: &CommandRule, client: &dyn JjClient) {
    for example in rule.declaration.doc.examples {
        assert_command_rule_fail(
            rule,
            DOCUMENTED_EXAMPLE_CASE,
            example.flagged,
            Some(example.flagged_span),
            client,
        );
        assert_command_rule_pass(rule, DOCUMENTED_FIX_CASE, example.fixed, client);
    }
}

#[track_caller]
#[expect(
    clippy::too_many_arguments,
    reason = "internal harness helper forwarding test context"
)]
fn assert_project_fail_input<R: TestableCodeRule>(
    rule: &R,
    lang: Language,
    case_name: &str,
    files: &[(&str, &str)],
    target_path: &str,
    expected_snippet: Option<&str>,
    repeat: RepeatCheck,
    overrides: Option<&RuleOverrides>,
) {
    let declared = rule.declared_rule();
    let rule_name = declared.name.0;
    assert!(
        rule.is_project_rule(),
        "rule_test! [{rule_name}] ({lang:?}) FAIL case '{case_name}': multi-file `{{ ... }}` cases are only valid on `ProjectCodeRule`s"
    );
    assert_eq!(
        Language::from_path(Path::new(target_path)),
        Some(lang),
        "rule_test! [{rule_name}] ({lang:?}) FAIL case '{case_name}': target_path '{target_path}' does not match {lang:?}"
    );
    let parsed_files = validate_and_parse_project_files(rule_name, lang, case_name, files);
    let target_code = files
        .iter()
        .find_map(|&(path, content)| (path == target_path).then_some(content))
        .unwrap_or_else(|| {
            panic!(
                "rule_test! [{rule_name}] ({lang:?}) FAIL case '{case_name}': target_path '{target_path}' is missing from project files"
            )
        });
    let root = Path::new(MemoryFileSource::VIRTUAL_ROOT);
    let memory = MemoryFileSource::from_relative_files(root, files);
    let index = build_virtual_project_index(root, &memory, rule_name, lang, case_name);
    for (relative_path, parsed) in &parsed_files {
        index.seed_from_parsed(&root.join(relative_path), parsed);
    }
    let context = ProjectContext::new(&index);
    let mut target_diags = None;
    for (relative_path, parsed) in &parsed_files {
        if Language::from_path(Path::new(relative_path)) != Some(lang) {
            continue;
        }
        let full_path = root.join(relative_path);
        let diags = rule.run_in_ban_mode(&full_path, parsed, Some(&context), overrides);
        if *relative_path == target_path {
            let expected_context = crate::diagnostic::LocationContext::File(full_path);
            for diagnostic in &diags {
                assert_eq!(
                    diagnostic.location.context, expected_context,
                    "rule_test! [{rule_name}] ({lang:?}) FAIL case '{case_name}': diagnostic reported on '{}' while checking '{target_path}'",
                    diagnostic.location.context
                );
            }
            target_diags = Some(diags);
        } else {
            assert!(
                diags.is_empty(),
                indoc::indoc! {"
                    rule_test! [{}] ({:?}) FAIL case '{}' emitted unexpected diagnostics on non-target context file '{}':
                    {}"},
                rule_name,
                lang,
                case_name,
                relative_path,
                format_diagnostics_for_test(&diags),
            );
        }
    }
    if let Some(error) = index.architecture_error() {
        panic!(
            "rule_test! [{rule_name}] ({lang:?}) FAIL case '{case_name}': invalid project architecture: {error}"
        );
    }
    let diags = target_diags.expect("target_path verified present");
    let span = verify_single_diagnostic(
        rule_name,
        lang,
        case_name,
        target_code,
        expected_snippet,
        &diags,
    );
    assert_no_comment_in_explanation_position(rule_name, lang, case_name, target_code, span.start);
    assert_every_occurrence_reported_in_project(
        rule,
        lang,
        case_name,
        files,
        &parsed_files,
        target_path,
        target_code,
        span,
        repeat,
        overrides,
    );
}

#[track_caller]
fn validate_and_parse_project_files<'a>(
    rule_name: &str,
    lang: Language,
    case_name: &str,
    files: &[(&'a str, &'a str)],
) -> Vec<(&'a str, ParsedFile)> {
    assert!(
        !files.is_empty(),
        "rule_test! [{rule_name}] ({lang:?}) case '{case_name}': project file map cannot be empty"
    );
    let mut seen = HashSet::with_capacity(files.len());
    let mut parsed_files = Vec::new();
    for &(relative_path, content) in files {
        assert!(
            seen.insert(relative_path),
            "rule_test! [{rule_name}] ({lang:?}) case '{case_name}': duplicate path '{relative_path}' in project map"
        );
        let path = Path::new(relative_path);
        if let Some(file_lang) = Language::from_path(path) {
            let parsed = ParsedFile::new(content, file_lang);
            assert!(
                !parsed.has_syntax_error(),
                "rule_test! [{rule_name}] ({lang:?}) case '{case_name}': file '{relative_path}' has a syntax error:\n{content}",
            );
            parsed_files.push((relative_path, parsed));
        } else if path.extension().is_some_and(|extension| extension == "pyi") {
            let parsed = ParsedFile::new(content, Language::Python);
            assert!(
                !parsed.has_syntax_error(),
                "rule_test! [{rule_name}] ({lang:?}) case '{case_name}': stub file '{relative_path}' has a syntax error:\n{content}",
            );
            parsed_files.push((relative_path, parsed));
        } else if relative_path == crate::code_lint::project::CONFIG_FILE_NAME
            && let Err(error) = crate::code_lint::project::ProjectConfig::from_toml_str(content)
        {
            panic!(
                "rule_test! [{rule_name}] ({lang:?}) case '{case_name}': invalid {relative_path}: {error}\n{content}"
            );
        }
    }
    parsed_files
}

#[track_caller]
fn verify_single_diagnostic(
    rule_name: &str,
    lang: Language,
    case_name: &str,
    code: &str,
    expected_snippet: Option<&str>,
    diags: &[Diagnostic],
) -> std::ops::Range<usize> {
    let expected_slice = expected_snippet.unwrap_or(code).trim();
    let [diagnostic] = diags else {
        panic!(
            indoc::indoc! {"
                rule_test! [{}] ({:?}) FAIL case '{}' diagnostic count mismatch:
                Expected exactly 1 diagnostic, got {}:
                {}
                Source:
                {}"},
            rule_name,
            lang,
            case_name,
            diags.len(),
            format_diagnostics_for_test(diags),
            code,
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
        indoc::indoc! {r##"
            rule_test! [{}] ({:?}) FAIL case '{}' flagged span mismatch:
            Expected flagged slice:
            {}
            Actual flagged slice:
            {}
            (Tip: if the flagged AST node is a sub-expression of the test code, add `=> r#"..."#` to specify the inner slice.)"##},
        rule_name, lang, case_name, expected_slice, actual_slice,
    );

    diagnostic.location.span.start..diagnostic.location.span.end
}

/// Repeats `code` twice in one file and asserts the rule flags the same span in both copies,
/// so a rule that stops after its first match cannot pass.
#[track_caller]
fn assert_every_occurrence_reported<R: TestableCodeRule>(
    rule: &R,
    lang: Language,
    case_name: &str,
    code: &str,
    span: std::ops::Range<usize>,
    repeat: RepeatCheck,
    overrides: Option<&RuleOverrides>,
) {
    let declared = rule.declared_rule();
    let rule_name = declared.name.0;
    let second_copy = build_second_copy(rule_name, lang, case_name, code, repeat);
    let repeated = format!("{code}\n{second_copy}");
    let offset = code.len() + 1;
    let diags = run_code_rule(rule, &repeated, dummy_filename(lang), overrides);
    assert_repeated_spans(rule_name, lang, case_name, &repeated, &diags, span, offset);
}

#[track_caller]
#[expect(
    clippy::too_many_arguments,
    reason = "internal harness helper forwarding test context"
)]
fn assert_every_occurrence_reported_in_project<R: TestableCodeRule>(
    rule: &R,
    lang: Language,
    case_name: &str,
    files: &[(&str, &str)],
    parsed_files: &[(&str, ParsedFile)],
    target_path: &str,
    target_code: &str,
    span: std::ops::Range<usize>,
    repeat: RepeatCheck,
    overrides: Option<&RuleOverrides>,
) {
    let declared = rule.declared_rule();
    let rule_name = declared.name.0;
    let second_copy = build_second_copy(rule_name, lang, case_name, target_code, repeat);
    let repeated = format!("{target_code}\n{second_copy}");
    let offset = target_code.len() + 1;

    let repeated_entries: Vec<(&str, &str)> = files
        .iter()
        .map(|&(path, content)| {
            if path == target_path {
                (path, repeated.as_str())
            } else {
                (path, content)
            }
        })
        .collect();
    let root = Path::new(MemoryFileSource::VIRTUAL_ROOT);
    let memory = MemoryFileSource::from_relative_files(root, &repeated_entries);
    let index = build_virtual_project_index(root, &memory, rule_name, lang, case_name);
    for (relative_path, parsed) in parsed_files {
        if *relative_path != target_path {
            index.seed_from_parsed(&root.join(relative_path), parsed);
        }
    }
    let full_target = root.join(target_path);
    let parsed_target = ParsedFile::new(&repeated, lang);
    assert!(
        !parsed_target.has_syntax_error(),
        "rule_test! [{rule_name}] ({lang:?}) FAIL case '{case_name}': repeated target '{target_path}' has a syntax error:\n{repeated}",
    );
    index.seed_from_parsed(&full_target, &parsed_target);
    if let Some(error) = index.architecture_error() {
        panic!(
            "rule_test! [{rule_name}] ({lang:?}) FAIL case '{case_name}' has an invalid architecture configuration on repeat check: {error}"
        );
    }
    let context = ProjectContext::new(&index);
    let diags = rule.run_in_ban_mode(&full_target, &parsed_target, Some(&context), overrides);
    assert_repeated_spans(rule_name, lang, case_name, &repeated, &diags, span, offset);
}

#[track_caller]
fn build_virtual_project_index<'a>(
    root: &Path,
    memory: &'a MemoryFileSource,
    rule_name: &str,
    lang: Language,
    case_name: &str,
) -> ProjectIndex<'a> {
    ProjectIndex::discover_with_config(
        root,
        memory,
        &crate::code_lint::project::ProjectConfig::default(),
    )
    .unwrap_or_else(|error| {
        panic!(
            "rule_test! [{rule_name}] ({lang:?}) case '{case_name}': invalid {}: {error}",
            crate::code_lint::project::CONFIG_FILE_NAME
        )
    })
}

#[track_caller]
fn build_second_copy(
    rule_name: &str,
    lang: Language,
    case_name: &str,
    code: &str,
    repeat: RepeatCheck,
) -> String {
    match repeat {
        RepeatCheck::SameCode => code.to_owned(),
        RepeatCheck::DistinctLiterals => {
            with_distinct_literals(code, lang).unwrap_or_else(|reason| {
                panic!(
                    indoc::indoc! {"
                        rule_test! [{}] ({:?}) FAIL case '{}': cannot give the second copy distinct literals ({}); rename a literal in this case.
                        Source:
                        {}"},
                    rule_name, lang, case_name, reason, code
                )
            })
        }
    }
}

#[track_caller]
fn assert_repeated_spans(
    rule_name: &str,
    lang: Language,
    case_name: &str,
    repeated_source: &str,
    diags: &[Diagnostic],
    span: std::ops::Range<usize>,
    offset: usize,
) {
    let mut actual: Vec<_> = diags
        .iter()
        .map(|diagnostic| diagnostic.location.span.start..diagnostic.location.span.end)
        .collect();
    actual.sort_unstable_by_key(|range| (range.start, range.end));
    let expected = vec![span.clone(), span.start + offset..span.end + offset];
    assert_eq!(
        actual,
        expected,
        indoc::indoc! {"
            rule_test! [{}] ({:?}) FAIL case '{}' did not report every occurrence when the code is repeated twice in one file:
            {}
            Source:
            {}"},
        rule_name,
        lang,
        case_name,
        format_diagnostics_for_test(diags),
        repeated_source,
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

/// Asserts that no comment in a `fail` case sits where `require-explanation` looks for one:
/// above the snippet's first line of code, on the flagged line, or on the line above it.
///
/// Every case runs in `ban` mode, where comments never change the verdict, so such a comment
/// cannot test the rule: it only restates framework behaviour (`semantic::comments` tests it)
/// and makes the case look like it checks something it does not. Comments inside the flagged
/// construct, which test the rule's parsing, stay allowed. Known gap: a comment on another line
/// of a multi-line statement header, which `require-explanation` also reads, is not rejected.
#[track_caller]
fn assert_no_comment_in_explanation_position(
    rule_name: &str,
    lang: Language,
    case_name: &str,
    code: &str,
    flagged_start: usize,
) {
    let line_of = |offset: usize| code[..offset].matches('\n').count();
    let flagged_line = line_of(flagged_start);
    let parsed = ParsedFile::new(code, lang);
    let comments = collect_comment_nodes(&parsed);
    let mut without_comments = code.as_bytes().to_vec();
    for comment in &comments {
        let span = comment.span();
        without_comments[span.start..span.end].fill(b' ');
    }
    let first_code_line = line_of(
        without_comments
            .iter()
            .position(|byte| !byte.is_ascii_whitespace())
            .unwrap_or(0),
    );
    for comment in comments {
        let comment_line = line_of(comment.span().start);
        assert!(
            comment_line >= first_code_line
                && comment_line != flagged_line
                && comment_line + 1 != flagged_line,
            indoc::indoc! {r#"
                rule_test! [{}] ({:?}) FAIL case '{}': comment `{}` sits where `require-explanation` reads explanations; cases run in `ban` mode, so it cannot affect the verdict. Test explanation handling in `semantic::comments`, not in a rule (see `rule_test!` "Writing cases").
                Source:
                {}"#},
            rule_name,
            lang,
            case_name,
            comment.text(),
            code,
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

fn dummy_filename(lang: Language) -> &'static str {
    match lang {
        Language::Python => "test.py",
        Language::Rust => "test.rs",
    }
}

/// Declarative macro generating the complete `#[cfg(test)] mod tests` suite for a [`CodeRule`]
/// or [`ProjectCodeRule`].
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
/// - `documented_examples`: the doc's [`crate::rule_declaration::Example`]s or
///   [`crate::rule_declaration::ProjectExample`]s, checked by [`assert_documented_examples`].
///
/// # How a `fail` case is checked
///
/// - It must produce exactly one diagnostic. Multi-node cases are not supported; known cases
///   that would need them are nested flagged constructs (a `def` inside a nested `def` in
///   `nested-function`) and rules reporting every occurrence inside one node. Per-scope
///   aggregation fits when each case sits in its own scope (`repeated-index-access`).
/// - Without `=> r#"..."#`, the whole snippet must be the flagged node. With it, the finding
///   must span exactly that inner slice (because `rule_test!` compares the flagged slice text
///   rather than its byte offset, a case asserting *which* of two occurrences is flagged must
///   spell the two occurrences differently, such as `'fast'` vs. `"fast"` or `404u16` vs. `404`).
//    TODO: Investigate if this is ricr or should be improved.
///   Leading indentation of lines `2..N` is normalized,
///   so the slice can be written as a clean `indoc!` string.
/// - The code is also run repeated twice in one file and must report both occurrences, so
///   a rule that stops after its first match (`find` instead of `find_all`, early `return`,
///   stray `break`) fails.
/// - Rules grouping equal literals across the file pass `repeat: DistinctLiterals`
///   ([`RepeatCheck`]): the second copy's literals get new values, so each copy forms its own
///   groups and the check stays as strict. Write `rule_test!(RULE, repeat: DistinctLiterals, { ... })`.
///   The case panics when no such rewrite exists (equal values spelled differently such as
///   `0xFF` and `255`, binary or octal digits); unit-test those values on the `ast` helper.
/// - Rules with an empty default `banned` [`crate::rule_declaration::ListOption`] pass
///   `banned: &["..."]` so cases and documented examples run with an active deny list:
///   `rule_test!(RULE, banned: &["..."], { ... })`.
///
/// # Writing cases
///
/// - At minimum: the core antipattern (`fail`) and the canonical fix from the template's
///   suggestion (`pass`).
/// - One behaviour per case (one banned pattern, one exemption, one construct), named after
///   it, so a failing case name pinpoints the regression.
/// - An exemption `pass` case must be flagged if the exemption were removed.
/// - An accepted false negative is a `pass` case named `known_gap_*` with a `ROADMAP.md`
///   entry, so fixing it forces the case to be updated.
/// - Test the rule, never the framework. Every case runs in `ban` mode (via [`run_code_rule`]),
///   so an explanation comment never excuses a `pass` case and never matters in a `fail`
///   case. No case may depend on behaviour the rule does not implement: enforcement modes,
///   explanation comments (`require-explanation`, including which comments count, such as
///   Rust doc comments not counting), `omni:ignore` suppressions, option parsing or
///   per-language resolution. Such a case is vacuous: it passes whatever the framework does,
///   so it guards nothing while claiming to. Those behaviours are tested once, next to their
///   code (`code_lint::contract`, `semantic::comments`, `code_lint::suppression`,
///   `rule_declaration`). A `fail` case with a comment above its code, on the flagged line or
///   on the line above it is rejected (`assert_no_comment_in_explanation_position`); a comment
///   elsewhere inside the construct is fine if it is necessary to test the rule's parsing.
///   Do not snapshot template prose: `tests/registry.rs` checks templates, and snapshots are
///   reserved for CLI output (`tests/cli.rs`).
/// - When a case cannot be expressed (e.g. a module-level construct broken by the
///   repeated-occurrence check), unit-test the `ast` / `semantic` helper that computes the
///   fact. Change the harness only for a whole class of rules, with a guardrail so the
///   change cannot hide regressions.
macro_rules! rule_test {
    (@pass_input { $( $rel_path:literal => $file_code:literal ),+ $(,)? }) => {
        $crate::test_utils::PassCaseInput::Project(&[
            $( ($rel_path, indoc::indoc! { $file_code }) ),+
        ])
    };
    (@pass_input $code:literal) => {
        $crate::test_utils::PassCaseInput::Single(indoc::indoc! { $code })
    };
    (@fail_input { $( $rel_path:literal => $file_code:literal ),+ $(,)? }, ( $target_path:literal, $snippet:literal $(,)? )) => {
        $crate::test_utils::FailCaseInput::Project {
            files: &[ $( ($rel_path, indoc::indoc! { $file_code }) ),+ ],
            target_path: $target_path,
            expected_snippet: Some(indoc::indoc! { $snippet }),
        }
    };
    (@fail_input $code:literal $( , $snippet:literal )?) => {
        $crate::test_utils::FailCaseInput::Single {
            code: indoc::indoc! { $code },
            expected_snippet: None $( .or(Some(indoc::indoc! { $snippet })) )?,
        }
    };
    ($rule:expr, repeat: $repeat:ident, { $($body:tt)* }) => {
        $crate::test_utils::rule_test!(tests: $rule, repeat: $repeat, banned: &[], { $($body)* });
    };
    ($rule:expr, banned: &[ $( $banned_entry:expr ),+ $(,)? ], { $($body:tt)* }) => {
        $crate::test_utils::rule_test!(
            tests: $rule,
            repeat: SameCode,
            banned: &[ $( $banned_entry ),+ ],
            { $($body)* }
        );
    };
    ($rule:expr, { $($body:tt)* }) => {
        $crate::test_utils::rule_test!(tests: $rule, repeat: SameCode, banned: &[], { $($body)* });
    };
    ($mod_name:ident : $rule:expr, { $($body:tt)* }) => {
        $crate::test_utils::rule_test!(
            $mod_name: $rule,
            repeat: SameCode,
            banned: &[],
            { $($body)* }
        );
    };
    (
        $mod_name:ident : $rule:expr,
        repeat: $repeat:ident,
        banned: &[ $( $banned_entry:expr ),* $(,)? ],
        {
            $(
                $lang:ident => {
                    pass: [
                        $( $pass_name:ident => $pass_spec:tt ),+ $(,)?
                    ],
                    fail: [
                        $( $fail_name:ident => $fail_spec:tt $( => $fail_target:tt )? ),+ $(,)?
                    ] $(,)?
                }
            ),+ $(,)?
        }
    ) => {
        mod $mod_name {
            use super::*;

            fn test_overrides() -> Option<$crate::rule_declaration::RuleOverrides> {
                $crate::test_utils::banned_list_overrides(&$rule, &[ $( $banned_entry ),* ])
            }

            #[test]
            fn language_completeness() {
                $crate::test_utils::assert_language_completeness(
                    &$rule,
                    &[$( $crate::diagnostic::Language::$lang ),+],
                );
            }

            #[test]
            fn documented_examples() {
                let overrides = test_overrides();
                $crate::test_utils::assert_documented_examples(
                    &$rule,
                    $crate::test_utils::RepeatCheck::$repeat,
                    overrides.as_ref(),
                );
            }

            #[rstest::rstest]
            $(
                $(
                    #[case::$pass_name(
                        $crate::diagnostic::Language::$lang,
                        stringify!($pass_name),
                        $crate::test_utils::rule_test!(@pass_input $pass_spec),
                    )]
                )+
            )+
            fn pass(
                #[case] lang: $crate::diagnostic::Language,
                #[case] case_name: &str,
                #[case] input: $crate::test_utils::PassCaseInput<'_>,
            ) {
                let overrides = test_overrides();
                $crate::test_utils::assert_rule_pass_input(
                    &$rule,
                    lang,
                    case_name,
                    input,
                    overrides.as_ref(),
                );
            }

            #[rstest::rstest]
            $(
                $(
                    #[case::$fail_name(
                        $crate::diagnostic::Language::$lang,
                        stringify!($fail_name),
                        $crate::test_utils::rule_test!(@fail_input $fail_spec $(, $fail_target )?),
                    )]
                )+
            )+
            fn fail(
                #[case] lang: $crate::diagnostic::Language,
                #[case] case_name: &str,
                #[case] input: $crate::test_utils::FailCaseInput<'_>,
            ) {
                let overrides = test_overrides();
                $crate::test_utils::assert_rule_fail_input(
                    &$rule,
                    lang,
                    case_name,
                    input,
                    $crate::test_utils::RepeatCheck::$repeat,
                    overrides.as_ref(),
                );
            }
        }
    };
}
pub(crate) use rule_test;

/// Declarative test-suite generator for [`CommandRule`]s.
///
/// Supports both purely syntactic command rules (omitting `jj: [...]`) and repository-aware
/// command rules (providing `jj: [ "rev" => CommitInfo { ... }, ... ]`).
macro_rules! command_rule_test {
    ($rule:ident, { $($body:tt)* } $(,)?) => {
        $crate::test_utils::command_rule_test!($rule, jj: [], { $($body)* });
    };
    (
        $rule:ident,
        jj: [ $( $rev:expr => $commit_info:expr ),* $(,)? ],
        {
            pass: [ $( $pass_name:ident => $pass_cmd:literal ),+ $(,)? ],
            fail: [ $( $fail_name:ident => $fail_cmd:literal $( => $snippet:literal )? ),+ $(,)? ] $(,)?
        } $(,)?
    ) => {
        mod tests {
            use super::*;

            fn fake_jj_client() -> $crate::test_utils::FakeJjClient {
                $crate::test_utils::FakeJjClient::from_entries(&[
                    $( ($rev, $commit_info) ),*
                ])
            }

            #[test]
            fn documented_examples() {
                let client = fake_jj_client();
                $crate::test_utils::assert_command_documented_examples(&$rule, &client);
            }

            #[rstest::rstest]
            $(
                #[case::$pass_name(
                    stringify!($pass_name),
                    indoc::indoc! { $pass_cmd },
                )]
            )+
            fn pass(#[case] case_name: &str, #[case] command_line: &str) {
                let client = fake_jj_client();
                $crate::test_utils::assert_command_rule_pass(&$rule, case_name, command_line, &client);
            }

            #[rstest::rstest]
            $(
                #[case::$fail_name(
                    stringify!($fail_name),
                    indoc::indoc! { $fail_cmd },
                    None $( .or(Some(indoc::indoc! { $snippet })) )?,
                )]
            )+
            fn fail(
                #[case] case_name: &str,
                #[case] command_line: &str,
                #[case] expected_snippet: Option<&str>,
            ) {
                let client = fake_jj_client();
                $crate::test_utils::assert_command_rule_fail(
                    &$rule,
                    case_name,
                    command_line,
                    expected_snippet,
                    &client,
                );
            }
        }
    };
}
pub(crate) use command_rule_test;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::code_lint::contract::RuleTarget;
    use crate::diagnostic::{RuleName, ViolationTemplate, violation_template};
    use crate::rule_declaration::{
        Classification, Consensus, ImpactedQuality, Precision, ProjectExample, RuleDoc, Topic,
    };

    const PROJECT_TEST_TEMPLATE: ViolationTemplate = violation_template! {
        summary: "Class `{class}` shadows imported class `{name}`.",
        rationale: "Shadowing an imported class obscures its origin.",
        suggestion: "Rename `{class}`.",
    };

    fn check_shadowed_imported_class(
        rule: &ProjectCodeRule,
        path: &Path,
        file: &ParsedFile,
        project: &ProjectContext<'_>,
        (): (),
    ) -> Vec<Diagnostic> {
        let Some(module) = project.python_module_of_path(path) else {
            return Vec::new();
        };
        let Some(summary) = project.python_summary(&module) else {
            return Vec::new();
        };
        summary
            .imports
            .keys()
            .filter(|imported_name| {
                summary
                    .classes
                    .iter()
                    .any(|class_summary| &class_summary.name == *imported_name)
            })
            .flat_map(|imported_name| {
                let pattern = format!("class {imported_name}");
                let source = file.source_text();
                source
                    .match_indices(&pattern)
                    .map(|(start, _)| {
                        let name_start = start + 6;
                        let span = crate::diagnostic::SourceSpan::new(
                            name_start,
                            name_start + imported_name.len(),
                        );
                        rule.diagnostic_at_span(
                            path,
                            file,
                            span,
                            &[("class", imported_name), ("name", imported_name)],
                        )
                    })
                    .collect::<Vec<_>>()
            })
            .collect()
    }

    const DUMMY_PROJECT_RULE: ProjectCodeRule = ProjectCodeRule {
        declaration: Declaration {
            name: RuleName("dummy-project-rule"),
            template: &PROJECT_TEST_TEMPLATE,
            languages: &[Language::Python],
            options: RuleOptions::code_rule(()),
            classification: Classification {
                topics: &[Topic::STATIC_TYPING],
                precision: Precision::Exact,
                consensus: Consensus::Opinionated,
                impacted_quality: ImpactedQuality::Maintainability,
            },
            doc: RuleDoc {
                summary: "Flags classes shadowing an imported name.",
                what_it_does: "Flags classes shadowing an imported name.",
                why_is_this_bad: "Shadowing is confusing.",
                known_problems: None,
                references: &[],
                examples: &[ProjectExample {
                    language: Language::Python,
                    context: &[("pkg/base.py", "class Base: pass\n")],
                    path: "pkg/service.py",
                    flagged: indoc::indoc! {"
                        from pkg.base import Base

                        class Base: pass
                    "},
                    flagged_span: "Base",
                    fixed: indoc::indoc! {"
                        from pkg.base import Base

                        class Service(Base): pass
                    "},
                }],
            },
        },
        target: RuleTarget::All,
        check: check_shadowed_imported_class,
    };

    crate::test_utils::rule_test!(
        project_rule_harness: DUMMY_PROJECT_RULE,
        {
            Python => {
                pass: [
                    distinct_class_name => {
                        "pkg/base.py" => "class Base: pass",
                        "pkg/service.py" => "
                            from pkg.base import Base

                            class Child(Base):
                                pass
                        ",
                    },
                ],
                fail: [
                    shadows_imported_class => {
                        "pkg/base.py" => "class Base: pass",
                        "pkg/service.py" => "
                            from pkg.base import Base

                            class Base:
                                pass
                        ",
                    } => ("pkg/service.py", "Base"),
                ],
            },
        }
    );

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
