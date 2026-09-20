//! Static file structure analysis domain using ast-grep-core.

pub(crate) mod ast_python;
pub(crate) mod ast_rust;
pub(crate) mod calls;
pub(crate) mod comments;
pub(crate) mod rules;
pub(crate) mod suppression;

use crate::core::Config;
use crate::diagnostic::Diagnostic;
use ast_grep_core::AstGrep;
use ast_grep_language::SupportLang;
use rayon::prelude::*;
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

pub use crate::core::{AstNode, SourceDoc};

/// Target execution scope for a code rule (source files vs test files).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RuleTarget {
    /// Rule runs on all matching files.
    #[default]
    All,
    /// Rule runs exclusively on test files.
    TestsOnly,
    /// Rule runs exclusively on production source files (skipped on test files).
    SourceOnly,
}

/// Common trait for static file code validation rules.
pub trait CodeRule: crate::core::Rule {
    /// Returns the target execution scope of this code rule (defaults to `RuleTarget::All`).
    #[must_use]
    fn target(&self) -> RuleTarget {
        RuleTarget::All
    }

    /// Returns true if this rule supports the given language.
    #[must_use]
    fn supports_language(&self, lang: SupportLang) -> bool {
        self.supported_languages().contains(&lang)
    }

    /// Renders this rule's violation template at the given AST node using the node's language.
    #[must_use]
    fn diagnostic_at_node(
        &self,
        path: &Path,
        node: &AstNode<'_>,
        params: &[(&str, &str)],
    ) -> Diagnostic {
        self.render_diagnostic_for_lang(
            *node.lang(),
            params,
            crate::diagnostic::SourceLocation::from_node(path, node),
        )
    }

    /// Evaluates the file against this static analysis rule.
    ///
    /// Returned diagnostics may be in any order: ordering is owned by the reporting layer
    /// ([`crate::diagnostic`]), so sorting here is dead work.
    #[must_use]
    fn check_file(
        &self,
        path: &Path,
        grep: &AstGrep<SourceDoc>,
        config: &Config,
    ) -> Vec<Diagnostic>;
}

/// Helper to detect language from file extension.
#[must_use]
pub fn detect_language(path: &Path) -> Option<SupportLang> {
    path.extension()
        .and_then(std::ffi::OsStr::to_str)
        .and_then(|ext| match ext {
            "py" => Some(SupportLang::Python),
            "rs" => Some(SupportLang::Rust),
            _ => None,
        })
}

/// Returns true if `rule` should be evaluated on `path` given its language and test-file context.
fn should_evaluate_rule(
    rule: &dyn CodeRule,
    path: &Path,
    lang: SupportLang,
    is_test: bool,
    has_inline_tests: bool,
    config: &Config,
) -> bool {
    if rule.tags().contains(&crate::rules::Tag::Suppression) {
        return false;
    }
    if !config.is_rule_enabled_for_path(rule, path) || !rule.supports_language(lang) {
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
    rule: &dyn CodeRule,
    path: &Path,
    lang: SupportLang,
    is_test: bool,
    config: &Config,
) -> bool {
    if rule.tags().contains(&crate::rules::Tag::Suppression) {
        return false;
    }
    config.is_rule_enabled_for_path(rule, path)
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
        && crate::rules::CODE_RULES.iter().any(|rule| {
            rule.tags().contains(&crate::rules::Tag::Suppression)
                && config.is_rule_enabled_for_path(*rule, path)
                && rule.supports_language(lang)
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
    let has_code_rules = crate::rules::CODE_RULES
        .iter()
        .any(|rule| is_rule_candidate_for_path(*rule, path, lang, is_test, config));

    !has_code_rules && !has_active_suppression_audit(path, lang, content, config)
}

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
    let grep = AstGrep::new(content, lang);
    let mut tracker = suppression::SuppressionTracker::from_ast(&grep, content);

    let inline_test_ranges = if !is_test && lang == SupportLang::Rust {
        ast_rust::collect_inline_test_ranges(&grep.root())
    } else {
        Vec::new()
    };
    let has_inline_tests = !inline_test_ranges.is_empty();
    let mut raw_diagnostics = Vec::new();

    for rule in crate::rules::CODE_RULES {
        if !should_evaluate_rule(*rule, path, lang, is_test, has_inline_tests, config) {
            continue;
        }
        let rule_diagnostics = rule.check_file(path, &grep, config);
        raw_diagnostics.extend(filter_diagnostics_by_target(
            rule_diagnostics,
            rule.target(),
            is_test,
            &inline_test_ranges,
        ));
    }

    let mut diagnostics = tracker.filter_diagnostics(raw_diagnostics);
    diagnostics.extend(tracker.audit(path, config));
    diagnostics
}

/// Collects all outermost test function and test method nodes (`def test_*` in Python, `#[test]` / `fn test_*` in Rust).
#[must_use]
pub(crate) fn collect_test_functions<'a>(
    root: &AstNode<'a>,
    lang: SupportLang,
) -> Vec<AstNode<'a>> {
    let mut functions = Vec::new();
    collect_outer_test_functions(root, lang, &mut functions);
    functions
}

fn collect_outer_test_functions<'a>(
    node: &AstNode<'a>,
    lang: SupportLang,
    out: &mut Vec<AstNode<'a>>,
) {
    let target_kind = match lang {
        SupportLang::Rust => "function_item",
        _ => "function_definition",
    };
    if node.kind() == target_kind {
        let is_test = match lang {
            SupportLang::Rust => ast_rust::is_test_function(node),
            SupportLang::Python => ast_python::is_test_function(node),
            _ => false,
        };
        if is_test {
            out.push(node.clone());
        }
        return;
    }
    for child in node.children() {
        collect_outer_test_functions(&child, lang, out);
    }
}

/// Helper to collect binding definition nodes from a parsed AST grep document.
#[must_use]
pub(crate) fn collect_bindings(grep: &AstGrep<SourceDoc>) -> Vec<AstNode<'_>> {
    match grep.lang() {
        SupportLang::Rust => ast_rust::collect_bindings(&grep.root()),
        SupportLang::Python => ast_python::collect_bindings(&grep.root()),
        _ => Vec::new(),
    }
}

/// Returns true if the node represents an import binding.
#[must_use]
pub(crate) fn is_import_binding(node: &AstNode<'_>, lang: SupportLang) -> bool {
    let Some(parent) = node.parent() else {
        return false;
    };
    let parent_kind = parent.kind();
    match lang {
        SupportLang::Rust => matches!(
            parent_kind.as_ref(),
            "use_declaration" | "use_list" | "use_as_clause" | "scoped_identifier"
        ),
        SupportLang::Python => matches!(
            parent_kind.as_ref(),
            "import_statement" | "import_from_statement" | "aliased_import" | "dotted_name"
        ),
        _ => false,
    }
}

/// Returns true if the node represents an unaliased import binding (an external symbol
/// imported directly without a local `as` alias).
#[must_use]
pub(crate) fn is_unaliased_import_binding(node: &AstNode<'_>, lang: SupportLang) -> bool {
    let Some(parent) = node.parent() else {
        return false;
    };
    let parent_kind = parent.kind();
    match lang {
        SupportLang::Rust => {
            matches!(
                parent_kind.as_ref(),
                "use_declaration" | "use_list" | "scoped_identifier"
            )
        }
        SupportLang::Python => matches!(
            parent_kind.as_ref(),
            "import_statement" | "import_from_statement" | "dotted_name"
        ),
        _ => false,
    }
}

/// Returns true if the node represents a structural type, class, or function definition name.
#[must_use]
pub(crate) fn is_structural_definition(node: &AstNode<'_>, lang: SupportLang) -> bool {
    let Some(parent) = node.parent() else {
        return false;
    };
    let parent_kind = parent.kind();
    match lang {
        SupportLang::Rust => matches!(
            parent_kind.as_ref(),
            "struct_item"
                | "enum_item"
                | "trait_item"
                | "type_item"
                | "associated_type"
                | "function_item"
        ),
        SupportLang::Python => {
            matches!(
                parent_kind.as_ref(),
                "class_definition" | "function_definition"
            )
        }
        _ => false,
    }
}

/// Returns true if the node is the name of a member defined inside a trait implementation
/// (`impl Trait for Type` in Rust or `@override` method in Python), i.e. a name mandated by a contract.
///
/// Such names cannot be changed without breaking the implementation, so naming rules that
/// suggest renaming do not meaningfully apply to them. Only the member name itself qualifies;
/// nested names the author does control, such as locals, do not.
#[must_use]
pub(crate) fn is_trait_impl_member(node: &AstNode<'_>, lang: SupportLang) -> bool {
    let Some(item) = node.parent() else {
        return false;
    };
    match lang {
        SupportLang::Rust => {
            if !matches!(
                item.kind().as_ref(),
                "function_item" | "type_item" | "associated_type" | "const_item"
            ) {
                return false;
            }
            let Some(body) = item.parent() else {
                return false;
            };
            if body.kind().as_ref() != "declaration_list" {
                return false;
            }
            let Some(impl_item) = body.parent() else {
                return false;
            };
            impl_item.kind().as_ref() == "impl_item" && impl_item.field("trait").is_some()
        }
        SupportLang::Python => {
            item.kind().as_ref() == "function_definition"
                && ast_python::has_override_decorator(&item)
        }
        _ => false,
    }
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

        let mut targets = Vec::new();
        for (file_path, changed_lines) in changes {
            let is_target = target_paths.iter().any(|target| {
                if target.is_file() {
                    target == &file_path
                } else {
                    file_path.starts_with(target)
                }
            });

            if is_target && file_path.is_file() && detect_language(&file_path).is_some() {
                targets.push(LintTarget {
                    path: file_path,
                    changed_lines: Some(changed_lines),
                });
            }
        }
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
                collect_directory_candidates(path, &mut targets);
            } else {
                anyhow::bail!("Path does not exist: {}", path.display());
            }
        }
        Ok(targets)
    }
}

fn collect_directory_candidates(dir: &Path, targets: &mut Vec<LintTarget>) {
    for entry in ignore::WalkBuilder::new(dir)
        .require_git(false)
        .build()
        .flatten()
    {
        let path = entry.path();
        if path.is_file() && detect_language(path).is_some() {
            targets.push(LintTarget {
                path: path.to_path_buf(),
                changed_lines: None,
            });
        }
    }
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

    let diags = lint_file(path, &content, config);

    if let Some(changed) = changed_lines {
        let filtered = diags
            .into_iter()
            .filter(|diagnostic| changed.contains(&diagnostic.location.line))
            .collect();
        Ok(filtered)
    } else {
        Ok(diags)
    }
}

// TODO: Those test should be designed properly with parameterized, and if possible test more edge cases
#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::Selector;
    use std::collections::HashSet;

    #[test]
    fn test_is_rule_candidate_language_and_suppression() {
        let config = Config::default();
        let py_rule = &rules::no_logging_error_in_except::NoLoggingErrorInExcept;
        let suppression_rule = &suppression::BlanketSuppression;

        // Python rule on Python file vs Rust file
        assert!(is_rule_candidate_for_path(
            py_rule,
            Path::new("service.py"),
            SupportLang::Python,
            false,
            &config
        ));
        assert!(!is_rule_candidate_for_path(
            py_rule,
            Path::new("service.rs"),
            SupportLang::Rust,
            false,
            &config
        ));

        // Suppression rules are excluded from standard code rule evaluation
        assert!(!is_rule_candidate_for_path(
            suppression_rule,
            Path::new("service.py"),
            SupportLang::Python,
            false,
            &config
        ));
    }

    #[test]
    fn test_is_rule_candidate_target_scope() {
        let config = Config::default();
        let test_rule = &rules::no_sleep_in_tests::NoSleepInTests;

        // Python tests vs Python non-test source files
        assert!(is_rule_candidate_for_path(
            test_rule,
            Path::new("service.py"),
            SupportLang::Python,
            true,
            &config
        ));
        assert!(!is_rule_candidate_for_path(
            test_rule,
            Path::new("service.py"),
            SupportLang::Python,
            false,
            &config
        ));

        // Rust source files remain candidates because of potential inline #[cfg(test)]
        assert!(is_rule_candidate_for_path(
            test_rule,
            Path::new("service.rs"),
            SupportLang::Rust,
            false,
            &config
        ));
    }

    #[test]
    fn test_is_rule_candidate_source_only() {
        let config = Config::default();
        let source_rule = &rules::no_env_in_functions::NoEnvInFunctions;

        // SourceOnly runs on production source files, but is skipped on test files
        assert!(is_rule_candidate_for_path(
            source_rule,
            Path::new("service.rs"),
            SupportLang::Rust,
            false,
            &config
        ));
        assert!(!is_rule_candidate_for_path(
            source_rule,
            Path::new("tests/test_service.rs"),
            SupportLang::Rust,
            true,
            &config
        ));
    }

    #[test]
    fn test_has_active_suppression_audit() {
        let config = Config::default();
        let path = Path::new("main.py");

        // Content with suppression directive prefix
        let with_directive = "# omni:ignore[no-logging-error-in-except] -- reason\nprint('hi')";
        assert!(has_active_suppression_audit(
            path,
            SupportLang::Python,
            with_directive,
            &config
        ));

        // Content without suppression directive prefix
        let without_directive = "print('hello world')";
        assert!(!has_active_suppression_audit(
            path,
            SupportLang::Python,
            without_directive,
            &config
        ));

        // When all suppression rules are ignored in config
        let disabled_config = Config {
            ignore: Some(HashSet::from([Selector::Tag(
                crate::rules::Tag::Suppression,
            )])),
            ..Default::default()
        };
        assert!(!has_active_suppression_audit(
            path,
            SupportLang::Python,
            with_directive,
            &disabled_config
        ));
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

        // Config ignoring all rules on python files: should skip AST parse
        let no_rules_config = Config {
            ignore: Some(HashSet::from([
                Selector::Tag(crate::rules::Tag::Python),
                Selector::Tag(crate::rules::Tag::Suppression),
            ])),
            ..Default::default()
        };
        assert!(should_skip_ast_parse(
            py_path,
            SupportLang::Python,
            "x = 1",
            false,
            &no_rules_config
        ));

        // When all general code rules are ignored, but suppression directives are present:
        // do NOT skip so suppression hygiene can be audited
        let suppression_only_config = Config {
            select: Some(HashSet::from([Selector::Tag(
                crate::rules::Tag::Suppression,
            )])),
            ..Default::default()
        };
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
}
