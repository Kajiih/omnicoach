//! Code lint runner: language detection, per-file rule orchestration, and parallel target linting.

architecture_component!(CodeLintRunner);

use crate::code_lint::ast::{self, ParsedFile};
use crate::code_lint::rule::{CodeDetector, RuleTarget};
use crate::code_lint::semantic::comments::CommentIndex;
use crate::code_lint::suppression::{SUPPRESSION_AUDITS, SuppressionTracker};
use crate::core::{Config, EnforcementMode, ResolvedOptions};
use crate::diagnostic::Diagnostic;
use ast_grep_language::SupportLang;
use rayon::prelude::*;
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

/// Helper to detect language from file extension.
#[must_use]
pub fn detect_language(path: &Path) -> Option<SupportLang> {
    path.extension()
        .and_then(std::ffi::OsStr::to_str)
        .and_then(|extension| match extension {
            "py" => Some(SupportLang::Python),
            "rs" => Some(SupportLang::Rust),
            _ => None,
        })
}

/// Returns true if `rule` should be evaluated on `path` given its language and test-file context.
fn should_evaluate_rule(
    rule: &dyn CodeDetector,
    path: &Path,
    lang: SupportLang,
    is_test: bool,
    has_inline_tests: bool,
    config: &Config,
) -> bool {
    if !config.is_rule_enabled_for_path(rule.name(), path) || !rule.supports_language(lang) {
        return false;
    }
    match rule.target() {
        RuleTarget::SourceOnly => !is_test,
        RuleTarget::TestsOnly => is_test || has_inline_tests,
        RuleTarget::All => true,
    }
}

/// Filters rule diagnostics according to `RuleTarget` and inline `#[cfg(test)]` / `#[test]` byte ranges.
fn filter_diagnostics_by_target(
    rule_diagnostics: Vec<Diagnostic>,
    target: RuleTarget,
    is_test: bool,
    inline_test_ranges: &[std::ops::Range<usize>],
) -> Vec<Diagnostic> {
    if is_test || inline_test_ranges.is_empty() {
        return rule_diagnostics;
    }
    match target {
        RuleTarget::TestsOnly => rule_diagnostics
            .into_iter()
            .filter(|diagnostic| {
                let pos = diagnostic.location.span.start;
                inline_test_ranges.iter().any(|range| range.contains(&pos))
            })
            .collect(),
        RuleTarget::SourceOnly => rule_diagnostics
            .into_iter()
            .filter(|diagnostic| {
                let pos = diagnostic.location.span.start;
                !inline_test_ranges.iter().any(|range| range.contains(&pos))
            })
            .collect(),
        RuleTarget::All => rule_diagnostics,
    }
}

/// Returns true if a code rule may produce diagnostics for the given path and language context
/// prior to AST parsing.
///
/// For `RuleTarget::TestsOnly`, Python source files (`!is_test`) cannot produce diagnostics because
/// Omni does not support inline Python tests. Rust source files, however, can declare inline test
/// modules (`#[cfg(test)]`), so `TestsOnly` rules remain eligible until AST ranges are inspected.
#[must_use]
fn is_rule_candidate_for_path(
    rule: &dyn CodeDetector,
    path: &Path,
    lang: SupportLang,
    is_test: bool,
    config: &Config,
) -> bool {
    config.is_rule_enabled_for_path(rule.name(), path)
        && rule.supports_language(lang)
        && match rule.target() {
            RuleTarget::SourceOnly => !is_test,
            RuleTarget::TestsOnly => is_test || lang == SupportLang::Rust,
            RuleTarget::All => true,
        }
}

/// Returns true if any active suppression hygiene rule is enabled and applicable to `path`,
/// and the file content contains a suppression directive prefix (`"omni:"`).
#[must_use]
fn has_active_suppression_audit(
    path: &Path,
    lang: SupportLang,
    content: &str,
    config: &Config,
) -> bool {
    content.contains("omni:")
        && SUPPRESSION_AUDITS.iter().any(|audit| {
            let rule = audit.detector;
            config.is_rule_enabled_for_path(rule.name(), path)
                && rule.supported_languages().contains(&lang)
        })
}

/// Determines whether AST parsing can be skipped entirely for a file.
///
/// Skips Tree-sitter parsing when neither standard code rules nor active suppression audit
/// directives can produce findings for the file given its language, test-path status, and config.
#[must_use]
fn should_skip_ast_parse(
    path: &Path,
    lang: SupportLang,
    content: &str,
    is_test: bool,
    config: &Config,
) -> bool {
    let has_code_rules = crate::code_lint::rules::CODE_RULES
        .iter()
        .any(|registered| {
            is_rule_candidate_for_path(registered.detector, path, lang, is_test, config)
        });

    !has_code_rules && !has_active_suppression_audit(path, lang, content, config)
}

static SUPPRESSIBLE_RULES: std::sync::LazyLock<HashSet<&'static str>> =
    std::sync::LazyLock::new(|| {
        crate::code_lint::rules::CODE_RULES
            .iter()
            .map(|registered| registered.detector.name().0)
            .collect()
    });

/// Analyzes the structure of a file and returns diagnostic alerts.
#[must_use]
pub fn lint_file(path: &Path, content: &str, config: &Config) -> Vec<Diagnostic> {
    let Some(lang) = detect_language(path) else {
        return Vec::new();
    };

    let is_test = config.is_test_path(path);
    if should_skip_ast_parse(path, lang, content, is_test, config) {
        return Vec::new();
    }
    let file = ParsedFile::new(content, lang);
    let mut tracker = SuppressionTracker::from_file(&file, content);

    let inline_test_ranges = if !is_test && lang == SupportLang::Rust {
        ast::rust::collect_inline_test_ranges(&file)
    } else {
        Vec::new()
    };
    let has_inline_tests = !inline_test_ranges.is_empty();
    let mut raw_diagnostics = Vec::new();
    let mut comment_index = None;

    for registered in crate::code_lint::rules::CODE_RULES {
        let rule = registered.detector;
        if !should_evaluate_rule(rule, path, lang, is_test, has_inline_tests, config) {
            continue;
        }
        let options = ResolvedOptions::new(
            lang,
            &registered.options,
            config.rule_overrides.get(&rule.name()),
        );
        let mut rule_diagnostics = rule.check_file(path, &file, &options);
        if options.enforcement_mode() == EnforcementMode::RequireExplanation {
            let index = comment_index.get_or_insert_with(|| CommentIndex::from_file(&file));
            rule_diagnostics.retain(|diagnostic| {
                !index.has_explanation_for_span(
                    &file,
                    diagnostic.location.span,
                    diagnostic.location.line,
                )
            });
        }
        raw_diagnostics.extend(filter_diagnostics_by_target(
            rule_diagnostics,
            rule.target(),
            is_test,
            &inline_test_ranges,
        ));
    }

    let mut diagnostics = tracker.filter_diagnostics(raw_diagnostics);
    diagnostics.extend(tracker.audit(path, config, &SUPPRESSIBLE_RULES));
    diagnostics
}

/// Configuration options for the codebase linting pipeline.
#[derive(Debug, Clone, Default)]
pub struct LintOptions {
    /// File or directory paths to inspect.
    pub paths: Vec<PathBuf>,
    /// Run linter only on files/lines changed in VCS.
    pub diff: bool,
    /// Run linter comparing against a custom revision/commit (implies diff).
    pub diff_rev: Option<String>,
}

impl LintOptions {
    /// Creates a new `LintOptions` with default values.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
}

/// A target file scheduled for static analysis.
#[derive(Debug)]
struct LintTarget {
    path: PathBuf,
    changed_lines: Option<HashSet<usize>>,
}

/// Executes codebase linting over the specified targets using parallel analysis.
///
/// # Errors
///
/// Returns an error if:
/// - A specified input path does not exist on disk.
/// - VCS diff detection fails in `--diff` mode.
/// - A source file cannot be read from disk.
pub fn run_code_lint(options: &LintOptions, config: &Config) -> anyhow::Result<Vec<Diagnostic>> {
    let targets = collect_targets(options)?;

    let nested_diagnostics: Vec<Vec<Diagnostic>> = targets
        .into_par_iter()
        .map(|target| lint_single_file(&target.path, config, target.changed_lines.as_ref()))
        .collect::<anyhow::Result<Vec<_>>>()?;

    let all_diagnostics = nested_diagnostics.into_iter().flatten().collect();
    Ok(all_diagnostics)
}

fn collect_targets(options: &LintOptions) -> anyhow::Result<Vec<LintTarget>> {
    let enable_diff = options.diff || options.diff_rev.is_some();
    let effective_paths = if options.paths.is_empty() {
        vec![PathBuf::from(".")]
    } else {
        options.paths.clone()
    };

    if enable_diff {
        for path in &effective_paths {
            if !path.exists() {
                anyhow::bail!("Path does not exist: {}", path.display());
            }
        }

        let (vcs_type, resolved_rev, changes) =
            crate::diff::detect_vcs_diff(options.diff_rev.as_deref())?;

        eprintln!("Info: Comparing against {vcs_type:?} revision '{resolved_rev}'.");

        let target_paths: Vec<PathBuf> = effective_paths
            .iter()
            .map(|path| path.canonicalize().unwrap_or_else(|_| path.clone()))
            .collect();

        let targets = changes
            .into_iter()
            .filter(|(file_path, _)| {
                target_paths.iter().any(|target| {
                    if target.is_file() {
                        target == file_path
                    } else {
                        file_path.starts_with(target)
                    }
                }) && detect_language(file_path).is_some()
                    && file_path.is_file()
            })
            .map(|(file_path, changed_lines)| LintTarget {
                path: file_path,
                changed_lines: Some(changed_lines),
            })
            .collect();
        Ok(targets)
    } else {
        let mut targets = Vec::new();
        for path in &effective_paths {
            if path.is_file() {
                if detect_language(path).is_some() {
                    targets.push(LintTarget {
                        path: path.clone(),
                        changed_lines: None,
                    });
                }
            } else if path.is_dir() {
                targets.extend(collect_directory_candidates(path));
            } else {
                anyhow::bail!("Path does not exist: {}", path.display());
            }
        }
        Ok(targets)
    }
}

fn collect_directory_candidates(dir: &Path) -> impl Iterator<Item = LintTarget> {
    ignore::WalkBuilder::new(dir)
        .require_git(false)
        .build()
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            (path.is_file() && detect_language(path).is_some()).then(|| LintTarget {
                path: path.to_path_buf(),
                changed_lines: None,
            })
        })
}

fn lint_single_file(
    path: &Path,
    config: &Config,
    changed_lines: Option<&HashSet<usize>>,
) -> anyhow::Result<Vec<Diagnostic>> {
    if !path.is_file() || detect_language(path).is_none() {
        return Ok(Vec::new());
    }

    let content = fs::read_to_string(path)
        .map_err(|error| anyhow::anyhow!("Failed to read file '{}': {}", path.display(), error))?;

    let diagnostics = lint_file(path, &content, config);

    if let Some(changed) = changed_lines {
        let filtered = diagnostics
            .into_iter()
            .filter(|diagnostic| changed.contains(&diagnostic.location.line))
            .collect();
        Ok(filtered)
    } else {
        Ok(diagnostics)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::code_lint::rules;
    use crate::core::RuleOverrides;
    use crate::diagnostic::RuleName;
    use rstest::rstest;
    use std::collections::HashMap;

    fn code_rule_names() -> impl Iterator<Item = RuleName> {
        rules::CODE_RULES
            .iter()
            .map(|registered| registered.detector.name())
    }

    fn audit_names() -> impl Iterator<Item = RuleName> {
        SUPPRESSION_AUDITS
            .iter()
            .map(|registered| registered.detector.name())
    }

    fn config_disabling(rules: impl Iterator<Item = RuleName>) -> Config {
        Config {
            disabled_rules: rules.collect(),
            ..Default::default()
        }
    }

    /// Every suppression audit, and only the `enabled` code rules.
    fn config_enabling(enabled: &[&str]) -> Config {
        config_disabling(code_rule_names().filter(|name| !enabled.contains(&name.0)))
    }

    #[rstest]
    #[case::python_rule_on_python_file(
        rules::no_logging_error_in_except::RULE.detector,
        "service.py",
        SupportLang::Python,
        false,
        true
    )]
    #[case::python_rule_on_rust_file(
        rules::no_logging_error_in_except::RULE.detector,
        "service.rs",
        SupportLang::Rust,
        false,
        false
    )]
    #[case::tests_only_python_in_test_path(
        rules::no_sleep_in_tests::NO_SLEEP_IN_TESTS.detector,
        "test_service.py",
        SupportLang::Python,
        true,
        true
    )]
    #[case::tests_only_python_in_source_path(
        rules::no_sleep_in_tests::NO_SLEEP_IN_TESTS.detector,
        "service.py",
        SupportLang::Python,
        false,
        false
    )]
    #[case::tests_only_rust_source_path_eligible_for_inline_cfg_test(
        rules::no_sleep_in_tests::NO_SLEEP_IN_TESTS.detector,
        "service.rs",
        SupportLang::Rust,
        false,
        true
    )]
    #[case::source_only_on_production_file(
        rules::no_env_in_functions::RULE.detector,
        "service.rs",
        SupportLang::Rust,
        false,
        true
    )]
    #[case::source_only_skipped_on_test_file(
        rules::no_env_in_functions::RULE.detector,
        "tests/test_service.rs",
        SupportLang::Rust,
        true,
        false
    )]
    fn test_is_rule_candidate(
        #[case] rule: &dyn CodeDetector,
        #[case] path: &str,
        #[case] lang: SupportLang,
        #[case] is_test: bool,
        #[case] expected: bool,
    ) {
        let config = Config::default();
        assert_eq!(
            is_rule_candidate_for_path(rule, Path::new(path), lang, is_test, &config),
            expected
        );
    }

    #[rstest]
    #[case::with_directive(
        "# omni:ignore[no-logging-error-in-except] -- reason\nprint('hi')",
        false,
        true
    )]
    #[case::without_directive("print('hello world')", false, false)]
    #[case::with_directive_but_suppression_disabled(
        "# omni:ignore[no-logging-error-in-except] -- reason\nprint('hi')",
        true,
        false
    )]
    fn test_has_active_suppression_audit(
        #[case] content: &str,
        #[case] disable_suppression_rules: bool,
        #[case] expected: bool,
    ) {
        let config = if disable_suppression_rules {
            config_disabling(audit_names())
        } else {
            Config::default()
        };
        assert_eq!(
            has_active_suppression_audit(
                Path::new("main.py"),
                SupportLang::Python,
                content,
                &config
            ),
            expected
        );
    }

    #[test]
    fn test_should_skip_ast_parse() {
        let config = Config::default();
        let py_path = Path::new("script.py");

        // Normal file with default config: standard code rules are enabled, do NOT skip
        assert!(!should_skip_ast_parse(
            py_path,
            SupportLang::Python,
            "x = 1",
            false,
            &config
        ));

        // Config disabling every rule: should skip AST parse
        let no_rules_config = config_disabling(code_rule_names().chain(audit_names()));
        assert!(should_skip_ast_parse(
            py_path,
            SupportLang::Python,
            "x = 1",
            false,
            &no_rules_config
        ));

        // When all general code rules are ignored, but suppression directives are present:
        // do NOT skip so suppression hygiene can be audited
        let suppression_only_config = config_disabling(code_rule_names());
        let code_with_comment = "# omni:ignore -- missing reason";
        assert!(!should_skip_ast_parse(
            py_path,
            SupportLang::Python,
            code_with_comment,
            false,
            &suppression_only_config
        ));

        // But if that same file has no suppression comments, skip AST parsing
        assert!(should_skip_ast_parse(
            py_path,
            SupportLang::Python,
            "x = 1",
            false,
            &suppression_only_config
        ));
    }

    #[test]
    fn test_framework_enforcement_mode_require_explanation() {
        let source_documented = indoc::indoc! {r"
            # Valid explanation for type cast
            x = cast(int, y)
        "};
        let source_uncommented = "x = cast(int, y)";

        let rule = rules::no_typing_cast::RULE;
        let table: toml::Value =
            toml::from_str(r#"enforcement_mode = "require-explanation""#).expect("valid TOML");
        let overrides = RuleOverrides::parse(
            rule.detector.name().0,
            &rule.options,
            rule.detector.supported_languages(),
            &table,
        )
        .expect("valid options");
        let req_doc_config = Config {
            rule_overrides: HashMap::from([(rule.detector.name(), overrides)]),
            ..config_enabling(&["no-typing-cast"])
        };

        let diagnostics_req_doc =
            lint_file(Path::new("main.py"), source_documented, &req_doc_config);
        assert!(diagnostics_req_doc.is_empty());

        let diagnostics_req_uncommented =
            lint_file(Path::new("main.py"), source_uncommented, &req_doc_config);
        assert_eq!(diagnostics_req_uncommented.len(), 1);
    }

    #[test]
    fn test_rule_target_tests_only_filtering() {
        let source_prod = "import time\ndef run(): time.sleep(5)\n";
        let config = config_enabling(&["no-sleep-in-tests"]);

        let prod_diagnostics = lint_file(Path::new("src/daemon.py"), source_prod, &config);
        assert!(prod_diagnostics.is_empty());

        let test_diagnostics = lint_file(Path::new("tests/test_daemon.py"), source_prod, &config);
        assert_eq!(test_diagnostics.len(), 1);
    }

    #[test]
    fn test_rule_target_tests_only_rust_conditional_test() {
        let source_rust = indoc::indoc! {r"
            pub fn run_worker() {
                std::thread::sleep(std::time::Duration::from_secs(1));
            }
            #[cfg(test)]
            mod tests {
                #[test]
                fn test_worker() {
                    std::thread::sleep(std::time::Duration::from_millis(10));
                }
            }
        "};
        let config = config_enabling(&["no-sleep-in-tests"]);
        let diagnostics = lint_file(Path::new("src/worker.rs"), source_rust, &config);
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].location.line, 8);
    }

    #[test]
    fn test_rule_target_source_only_filtering() {
        let source_py = indoc::indoc! {r#"
            import os
            def read_key():
                return os.getenv("API_KEY")
        "#};
        let config = config_enabling(&["no-env-in-functions"]);

        let src_diagnostics = lint_file(Path::new("src/service.py"), source_py, &config);
        assert_eq!(src_diagnostics.len(), 1);

        let test_diagnostics = lint_file(Path::new("tests/test_service.py"), source_py, &config);
        assert!(test_diagnostics.is_empty());
    }

    #[test]
    fn test_valid_inline_suppression_silences_violation() {
        let content = "a = 1  # omni:ignore [single-letter-variable-name] -- math variable";
        let config = config_enabling(&["single-letter-variable-name"]);
        let diags = lint_file(Path::new("math.py"), content, &config);
        assert!(diags.is_empty(), "Expected 0 diagnostics, got: {diags:?}");
    }

    #[test]
    fn test_valid_preceding_line_suppression_silences_violation() {
        let content = "# omni:ignore [single-letter-variable-name] -- math variable\na = 1";
        let config = config_enabling(&["single-letter-variable-name"]);
        let diags = lint_file(Path::new("math.py"), content, &config);
        assert!(diags.is_empty(), "Expected 0 diagnostics, got: {diags:?}");
    }

    #[test]
    fn test_unused_suppression_flagged() {
        let content =
            "clean_name = 1  # omni:ignore [single-letter-variable-name] -- math variable";
        let config = config_enabling(&["single-letter-variable-name"]);
        let diags = lint_file(Path::new("clean.py"), content, &config);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].rule_name.0, "unused-suppression");
    }

    #[test]
    fn test_missing_reason_flagged() {
        let content = "a = 1  # omni:ignore [single-letter-variable-name]";
        let config = Config::default();
        let diags = lint_file(Path::new("math.py"), content, &config);
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
        let diags = lint_file(Path::new("math.py"), content, &config);
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
        let diags = lint_file(Path::new("math.py"), content, &config);
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
        let diags = lint_file(Path::new("math.py"), content, &config);
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
        let config = config_enabling(&["flat-scope-enforced", "single-letter-variable-name"]);
        let diags = lint_file(Path::new("src/module.py"), content, &config);

        // flat-scope-enforced should be suppressed, but single-letter-variable-name should be reported!
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].rule_name.0, "single-letter-variable-name");
    }

    #[test]
    fn test_file_level_unused_suppression_flagged() {
        let content = indoc::indoc! {r"
            # omni:disable-file [no-logging-error-in-except] -- unused file disable
            def clean():
                pass
        "};
        let config = config_enabling(&["no-logging-error-in-except"]);
        let diags = lint_file(Path::new("src/module.py"), content, &config);

        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].rule_name.0, "unused-suppression");
    }

    #[test]
    fn test_suppressing_supp_in_config() {
        let content = "clean_name = 1  # omni:ignore [single-letter-variable-name] -- intentional dormant suppression";
        let mut config = config_enabling(&["single-letter-variable-name"]);
        config.disabled_rules.insert(RuleName("unused-suppression"));
        let diags = lint_file(Path::new("src/template.py"), content, &config);

        assert!(diags.is_empty(), "Expected 0 diagnostics, got: {diags:?}");
    }

    #[test]
    fn test_string_literal_does_not_trigger_suppression() {
        let content =
            r##"sample_text = "# omni:ignore [single-letter-variable-name] -- not a comment""##;
        let config = config_enabling(&["single-letter-variable-name"]);
        let diags = lint_file(Path::new("src/test_case.py"), content, &config);

        // Does not trigger unused-suppression since it's a string literal, not a comment
        assert!(diags.is_empty(), "Expected 0 diagnostics, got: {diags:?}");
    }

    #[test]
    fn test_comment_prefix_word_boundary() {
        // Comments containing 'omni:ignored' should not be treated as omni:ignore directives
        let content = "a = 1  # omni:ignored by other tool";
        let config = config_enabling(&["single-letter-variable-name"]);
        let diags = lint_file(Path::new("src/test.py"), content, &config);

        // Should flag single-letter-variable-name violation, and NOT flag blanket-suppression
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].rule_name.0, "single-letter-variable-name");
    }

    #[test]
    fn test_command_rule_in_code_flagged_as_unknown() {
        // no-edits-on-described-commits is a command rule and cannot be suppressed in code files
        let content = "a = 1  # omni:ignore [no-edits-on-described-commits] -- invalid code rule";
        let config = Config::default();
        let diags = lint_file(Path::new("src/test.py"), content, &config);

        assert!(
            diags
                .iter()
                .any(|diag| diag.rule_name.0 == "unknown-suppression-rule"),
            "Expected unknown-suppression-rule for non-code rule in code directive, got: {diags:?}"
        );
    }
}
