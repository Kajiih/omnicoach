//! Static file structure analysis domain using ast-grep-core.

pub mod ast_python;
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

    /// Resolves this rule's banned call patterns against `defaults` for the file's language
    /// and returns all matching call expressions in `grep`.
    #[must_use]
    fn find_configured_banned_calls<'a>(
        &self,
        grep: &'a AstGrep<SourceDoc>,
        config: &Config,
        defaults: &crate::core::FilterListDefaults,
    ) -> Vec<calls::CallMatch<'a>> {
        let effective_banned = self.effective_banned_set(*grep.lang(), config, defaults);
        calls::find_banned_calls(grep, &effective_banned)
    }

    /// Evaluates `find_configured_banned_calls` and emits a diagnostic with `("callee", &matched.callee)`
    /// for every matched call expression.
    #[must_use]
    fn check_banned_calls(
        &self,
        path: &Path,
        grep: &AstGrep<SourceDoc>,
        config: &Config,
        defaults: &crate::core::FilterListDefaults,
    ) -> Vec<Diagnostic> {
        self.find_configured_banned_calls(grep, config, defaults)
            .into_iter()
            .map(|matched| {
                self.diagnostic_at_node(path, &matched.node, &[("callee", &matched.callee)])
            })
            .collect()
    }

    /// Resolves this rule's banned identifier suffixes against `defaults` for the file's language
    /// and emits a diagnostic with `("name", ...), ("actual_suffix", ...), ("base_name", ...)`
    /// for every matching variable, constant, or parameter binding.
    #[must_use]
    fn check_banned_suffixes(
        &self,
        path: &Path,
        grep: &AstGrep<SourceDoc>,
        config: &Config,
        defaults: &crate::core::FilterListDefaults,
    ) -> Vec<Diagnostic> {
        let effective_banned = self.effective_banned_set(*grep.lang(), config, defaults);
        find_suffixed_bindings(grep, &effective_banned)
            .into_iter()
            .map(|matched| {
                self.diagnostic_at_node(
                    path,
                    &matched.node,
                    &[
                        ("name", &matched.name),
                        ("actual_suffix", &matched.actual_suffix),
                        ("base_name", &matched.base_name),
                    ],
                )
            })
            .collect()
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
    let mut comment_index = None;

    for rule in crate::rules::CODE_RULES {
        if !should_evaluate_rule(*rule, path, lang, is_test, has_inline_tests, config) {
            continue;
        }
        let mode = rule.enforcement_mode(lang, config);
        let mut rule_diagnostics = rule.check_file(path, &grep, config);
        if mode == crate::core::EnforcementMode::RequireExplanation {
            let index =
                comment_index.get_or_insert_with(|| comments::CommentIndex::from_ast(&grep));
            rule_diagnostics
                .retain(|diagnostic| !index.has_adjacent_explanation(diagnostic.location.line));
        }
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

/// Collects all binding nodes in `grep` whose identifier names are locally authored and
/// eligible for renaming (excluding unaliased imports and trait/override contract members).
#[must_use]
pub(crate) fn collect_renameable_bindings(grep: &AstGrep<SourceDoc>) -> Vec<AstNode<'_>> {
    let lang = *grep.lang();
    collect_bindings(grep)
        .into_iter()
        .filter(|node| {
            !is_unaliased_import_binding(node, lang) && !is_trait_impl_member(node, lang)
        })
        .collect()
}

/// A variable, constant, or parameter binding whose identifier ends with a matched suffix.
pub(crate) struct SuffixedBindingMatch<'a> {
    /// The matched identifier AST node.
    pub node: AstNode<'a>,
    /// Full identifier text (e.g. `timeout_seconds` or `user_list`).
    pub name: String,
    /// Matched suffix slice preserving the identifier's original case (e.g. `_seconds` or `_INT`).
    pub actual_suffix: String,
    /// Identifier stem preceding the matched suffix (e.g. `timeout` or `MY`).
    pub base_name: String,
}

/// Finds all variable, constant, and parameter bindings in `grep` whose name ends
/// (case-insensitively) with any suffix in `banned_suffixes`.
///
/// Automatically skips imports ([`is_import_binding`]), structural definitions
/// ([`is_structural_definition`]: functions, classes, structs, enums, traits), and
/// trait/override contract names ([`is_trait_impl_member`]).
/// Suffixes are evaluated longest-first for deterministic matching.
#[must_use]
pub(crate) fn find_suffixed_bindings<'a, S: std::hash::BuildHasher>(
    grep: &'a AstGrep<SourceDoc>,
    banned_suffixes: &HashSet<String, S>,
) -> Vec<SuffixedBindingMatch<'a>> {
    if banned_suffixes.is_empty() {
        return Vec::new();
    }

    let mut sorted_suffixes: Vec<String> = banned_suffixes
        .iter()
        .map(|suffix| suffix.to_lowercase())
        .collect();
    // Sort descending by length, then ascending lexicographically for deterministic tie-breaking
    sorted_suffixes
        .sort_by(|left, right| right.len().cmp(&left.len()).then_with(|| left.cmp(right)));

    let bindings = collect_bindings(grep);
    let lang = *grep.lang();
    let mut matches = Vec::new();

    for node in bindings {
        if is_import_binding(&node, lang)
            || is_structural_definition(&node, lang)
            || is_trait_impl_member(&node, lang)
        {
            continue;
        }

        let name = node.text();
        let name_lower = name.to_lowercase();

        for suffix_lower in &sorted_suffixes {
            if name.len() > suffix_lower.len() && name_lower.ends_with(suffix_lower.as_str()) {
                let split_idx = name.len() - suffix_lower.len();
                let base_name = &name[..split_idx];
                let actual_suffix = &name[split_idx..];

                matches.push(SuffixedBindingMatch {
                    node,
                    name: name.into_owned(),
                    actual_suffix: actual_suffix.to_string(),
                    base_name: base_name.to_string(),
                });
                break;
            }
        }
    }

    matches
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
    use crate::core::Selector;
    use rstest::rstest;
    use std::collections::HashSet;

    #[rstest]
    #[case::python_rule_on_python_file(
        &rules::no_logging_error_in_except::NoLoggingErrorInExcept,
        "service.py",
        SupportLang::Python,
        false,
        true
    )]
    #[case::python_rule_on_rust_file(
        &rules::no_logging_error_in_except::NoLoggingErrorInExcept,
        "service.rs",
        SupportLang::Rust,
        false,
        false
    )]
    #[case::suppression_rule_excluded(
        &suppression::BlanketSuppression,
        "service.py",
        SupportLang::Python,
        false,
        false
    )]
    #[case::tests_only_python_in_test_path(
        &rules::no_sleep_in_tests::NoSleepInTests,
        "test_service.py",
        SupportLang::Python,
        true,
        true
    )]
    #[case::tests_only_python_in_source_path(
        &rules::no_sleep_in_tests::NoSleepInTests,
        "service.py",
        SupportLang::Python,
        false,
        false
    )]
    #[case::tests_only_rust_source_path_eligible_for_inline_cfg_test(
        &rules::no_sleep_in_tests::NoSleepInTests,
        "service.rs",
        SupportLang::Rust,
        false,
        true
    )]
    #[case::source_only_on_production_file(
        &rules::no_env_in_functions::NoEnvInFunctions,
        "service.rs",
        SupportLang::Rust,
        false,
        true
    )]
    #[case::source_only_skipped_on_test_file(
        &rules::no_env_in_functions::NoEnvInFunctions,
        "tests/test_service.rs",
        SupportLang::Rust,
        true,
        false
    )]
    fn test_is_rule_candidate(
        #[case] rule: &dyn CodeRule,
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
            Config {
                ignore: Some(HashSet::from([Selector::Tag(
                    crate::rules::Tag::Suppression,
                )])),
                ..Default::default()
            }
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

    #[test]
    fn test_framework_enforcement_mode_ban_default() {
        let source_documented = indoc::indoc! {r"
            # Valid explanation for type cast
            x = cast(int, y)
        "};
        let source_uncommented = indoc::indoc! {r"
            x = cast(int, y)
        "};

        let default_config = Config::default();
        let diagnostics_banned_doc =
            lint_file(Path::new("main.py"), source_documented, &default_config);
        assert_eq!(diagnostics_banned_doc.len(), 1);

        let diagnostics_banned_uncommented =
            lint_file(Path::new("main.py"), source_uncommented, &default_config);
        assert_eq!(diagnostics_banned_uncommented.len(), 1);
    }

    #[test]
    fn test_framework_enforcement_mode_require_explanation() {
        let source_documented = indoc::indoc! {r"
            # Valid explanation for type cast
            x = cast(int, y)
        "};
        let source_uncommented = indoc::indoc! {r"
            x = cast(int, y)
        "};

        let config_toml = r#"
            [rules.no-typing-cast]
            mode = "require-explanation"
        "#;
        let req_doc_config: Config = toml::from_str(config_toml).unwrap();

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
        let default_config = Config::default();

        let prod_diagnostics = lint_file(Path::new("src/daemon.py"), source_prod, &default_config);
        assert!(prod_diagnostics.is_empty());

        let test_diagnostics = lint_file(
            Path::new("tests/test_daemon.py"),
            source_prod,
            &default_config,
        );
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
        let default_config = Config::default();
        let diagnostics = lint_file(Path::new("src/worker.rs"), source_rust, &default_config);
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
        let default_config = Config::default();

        let src_diagnostics = lint_file(Path::new("src/service.py"), source_py, &default_config);
        assert_eq!(src_diagnostics.len(), 1);

        let test_diagnostics = lint_file(
            Path::new("tests/test_service.py"),
            source_py,
            &default_config,
        );
        assert!(test_diagnostics.is_empty());
    }
}
