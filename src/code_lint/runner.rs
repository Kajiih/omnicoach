//! Code lint runner: language detection, per-file rule orchestration, and parallel target linting.

architecture_component!(CodeLintRunner);

use crate::code_lint::ast::{self, ParsedFile};
use crate::code_lint::contract::{AnyCodeRule, RuleTarget};
use crate::code_lint::project::{
    ArchitectureError, DiskFileSource, OverlayFileSource, ProjectContext, ProjectIndex,
    ProjectLayout, configured_source_roots,
};
use crate::code_lint::rules::CODE_RULES;
use crate::code_lint::suppression::{
    SUPPRESSION_AUDITS, SuppressionTracker, contains_directive_prefix,
};
use crate::config::Config;
use crate::diagnostic::{Diagnostic, Language, RuleName};
use rayon::prelude::*;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

static SUPPRESSIBLE_RULES: std::sync::LazyLock<HashSet<&'static str>> =
    std::sync::LazyLock::new(|| CODE_RULES.iter().map(|rule| rule.name().0).collect());

/// Analyzes the structure of a single file without cross-file project context.
///
/// Cross-file [`crate::code_lint::contract::ProjectCodeRule`]s are skipped cleanly and excluded
/// from `evaluated_rules` so `unused-suppression` never fires falsely when linting an isolated
/// buffer without a [`ProjectContext`].
#[must_use]
pub fn lint_file(path: &Path, content: &str, config: &Config) -> Vec<Diagnostic> {
    lint_file_in_project(path, content, config, None)
}

/// Analyzes `path` (`content`) with an optional cross-file [`ProjectContext`].
#[must_use]
pub fn lint_file_in_project(
    path: &Path,
    content: &str,
    config: &Config,
    project: Option<&ProjectContext<'_>>,
) -> Vec<Diagnostic> {
    lint_file_with_index(path, content, config, None, project)
}

/// Analyzes an in-memory edit to `target_path` within `project_root`, constructing an
/// [`OverlayFileSource`]-backed [`ProjectContext`] when any enabled rule requires cross-file
/// project context.
///
/// # Errors
///
/// Returns the [`ArchitectureError`] of an invalid project architecture declaration when an
/// enabled rule needed it, matching [`run_code_lint`].
pub fn lint_virtual_file(
    project_root: &Path,
    target_path: &Path,
    virtual_content: &str,
    config: &Config,
) -> Result<Vec<Diagnostic>, ArchitectureError> {
    let raw_root = if project_root.is_absolute() {
        project_root.to_path_buf()
    } else {
        std::env::current_dir().map_or_else(
            |_| project_root.to_path_buf(),
            |working_dir| working_dir.join(project_root),
        )
    };
    let normalized_root = crate::config::lexical_normalize(&raw_root);
    let raw_target = if target_path.is_absolute() {
        target_path.to_path_buf()
    } else if let Ok(stripped) = target_path.strip_prefix(project_root) {
        normalized_root.join(stripped)
    } else {
        normalized_root.join(target_path)
    };
    let resolved_target = crate::config::lexical_normalize(&raw_target);
    let display_path = target_path
        .strip_prefix(project_root)
        .or_else(|_| resolved_target.strip_prefix(&normalized_root))
        .unwrap_or(target_path);
    let Some(lang) = Language::from_path(&resolved_target) else {
        return Ok(Vec::new());
    };
    let mut effective_config = config.clone();
    if effective_config.root_dir.is_none() {
        effective_config.root_dir = Some(normalized_root);
    }
    let is_test = effective_config.is_test_path(&resolved_target);
    let may_have_inline_tests = lang == Language::Rust;
    let has_project_rules = CODE_RULES.iter().any(|&rule| {
        rule.requires_project_context()
            && is_rule_applicable(
                rule,
                &resolved_target,
                lang,
                is_test,
                may_have_inline_tests,
                &effective_config,
            )
    });
    if !has_project_rules {
        let mut diagnostics = lint_file(&resolved_target, virtual_content, &effective_config);
        rewrite_diagnostic_paths(&mut diagnostics, display_path);
        return Ok(diagnostics);
    }
    let overlay = OverlayFileSource::new(
        DiskFileSource,
        HashMap::from([(resolved_target.clone(), virtual_content.to_owned())]),
    );
    let configured_roots = configured_source_roots(&effective_config);
    let layout = ProjectLayout::discover(&resolved_target, &overlay, &configured_roots);
    let index =
        attach_configured_architecture(ProjectIndex::new(layout, &overlay), &effective_config);
    let context = ProjectContext::new(&index);
    let mut diagnostics = lint_file_with_index(
        &resolved_target,
        virtual_content,
        &effective_config,
        Some(&index),
        Some(&context),
    );
    if let Some(error) = index.architecture_error() {
        return Err(error.clone());
    }
    rewrite_diagnostic_paths(&mut diagnostics, display_path);
    Ok(diagnostics)
}

/// Analyzes an in-memory edit like [`lint_virtual_file`], evaluating only the code rules named in
/// `rules`, so a caller that needs findings of a few rules skips building project context for
/// the others.
///
/// Suppression audits judge directives against the rules that ran, so when `rules` names an
/// audit, every rule runs and the result matches [`lint_virtual_file`].
///
/// # Errors
///
/// Returns the [`ArchitectureError`] of an invalid project architecture declaration when an
/// evaluated rule needed it.
pub fn lint_virtual_file_for_rules(
    project_root: &Path,
    target_path: &Path,
    virtual_content: &str,
    config: &Config,
    rules: &[RuleName],
) -> Result<Vec<Diagnostic>, ArchitectureError> {
    if SUPPRESSION_AUDITS
        .iter()
        .any(|audit| rules.contains(&audit.name))
    {
        return lint_virtual_file(project_root, target_path, virtual_content, config);
    }
    let mut restricted_config = config.clone();
    restricted_config.disabled_rules.extend(
        CODE_RULES
            .iter()
            .map(|rule| rule.name())
            .chain(SUPPRESSION_AUDITS.iter().map(|audit| audit.name))
            .filter(|name| !rules.contains(name)),
    );
    lint_virtual_file(
        project_root,
        target_path,
        virtual_content,
        &restricted_config,
    )
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

/// A target file that could not be read (for example, because it is not valid UTF-8).
#[derive(Debug)]
pub struct UnreadableFile {
    /// The path of the file.
    pub path: PathBuf,
    /// Why reading it failed.
    pub error: io::Error,
}

/// The outcome of [`run_code_lint`].
#[derive(Debug, Default)]
pub struct LintReport {
    /// Diagnostics from every file that could be read.
    pub diagnostics: Vec<Diagnostic>,
    /// Files that could not be read, sorted by path; the run still lints every other file.
    pub unreadable_files: Vec<UnreadableFile>,
}

/// Executes codebase linting over the specified targets using parallel analysis.
///
/// # Errors
///
/// Returns an error if:
/// - A specified input path does not exist on disk.
/// - VCS diff detection fails in `--diff` mode.
/// - Architectural declarations conflict or fail validation when `Topic::ARCHITECTURE` rules are active.
///
/// A source file that cannot be read is not an error: it is listed in
/// [`LintReport::unreadable_files`].
pub fn run_code_lint(options: &LintOptions, config: &Config) -> anyhow::Result<LintReport> {
    let targets = collect_targets(options)?;
    let has_project_rules = CODE_RULES.iter().any(|rule| {
        rule.requires_project_context() && !config.disabled_rules.contains(&rule.name())
    });
    let disk = DiskFileSource;
    let configured_roots = configured_source_roots(config);
    let mut project_indices: Vec<ProjectIndex<'_>> = Vec::new();
    let mut target_project_idx: Vec<Option<usize>> = Vec::with_capacity(targets.len());
    if has_project_rules {
        let mut directory_to_project_index: HashMap<PathBuf, usize> = HashMap::new();
        for target in &targets {
            let parent_dir = target
                .path
                .parent()
                .filter(|parent| !parent.as_os_str().is_empty())
                .unwrap_or_else(|| Path::new("."))
                .to_path_buf();
            let index_pos = if let Some(&cached_pos) = directory_to_project_index.get(&parent_dir) {
                cached_pos
            } else {
                let layout = ProjectLayout::discover(&target.path, &disk, &configured_roots);
                let resolved_pos = if let Some(existing_pos) = project_indices
                    .iter()
                    .position(|index| index.layout().project_root == layout.project_root)
                {
                    project_indices[existing_pos]
                        .merge_python_source_roots(&layout.python_source_roots);
                    existing_pos
                } else {
                    let new_pos = project_indices.len();
                    project_indices.push(attach_configured_architecture(
                        ProjectIndex::new(layout, &disk),
                        config,
                    ));
                    new_pos
                };
                directory_to_project_index.insert(parent_dir, resolved_pos);
                resolved_pos
            };
            target_project_idx.push(Some(index_pos));
        }
    } else {
        target_project_idx.resize(targets.len(), None);
    }
    let project_contexts: Vec<ProjectContext<'_>> =
        project_indices.iter().map(ProjectContext::new).collect();

    let results: Vec<Result<Vec<Diagnostic>, UnreadableFile>> = targets
        .into_par_iter()
        .zip(target_project_idx)
        .map(|(target, proj_idx)| {
            lint_single_file(
                &target.path,
                config,
                proj_idx.and_then(|idx| project_indices.get(idx)),
                proj_idx.and_then(|idx| project_contexts.get(idx)),
                target.changed_lines.as_ref(),
            )
        })
        .collect();

    for index in &project_indices {
        if let Some(error) = index.architecture_error() {
            anyhow::bail!("{error}");
        }
    }

    let mut report = LintReport::default();
    for result in results {
        match result {
            Ok(diagnostics) => report.diagnostics.extend(diagnostics),
            Err(unreadable) => report.unreadable_files.push(unreadable),
        }
    }
    report
        .unreadable_files
        .sort_by(|left, right| left.path.cmp(&right.path));
    Ok(report)
}

/// A target file scheduled for static analysis.
#[derive(Debug)]
struct LintTarget {
    path: PathBuf,
    changed_lines: Option<HashSet<usize>>,
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
        let working_dir = std::env::current_dir().ok();

        let targets = changes
            .into_iter()
            .filter(|(file_path, _)| {
                let canonical_file = file_path
                    .canonicalize()
                    .unwrap_or_else(|_| file_path.clone());
                target_paths.iter().any(|target| {
                    if target.is_file() {
                        target == &canonical_file
                    } else {
                        canonical_file.starts_with(target)
                    }
                }) && Language::from_path(file_path).is_some()
                    && file_path.is_file()
            })
            .map(|(file_path, changed_lines)| {
                let relative_path = working_dir
                    .as_deref()
                    .and_then(|dir| file_path.strip_prefix(dir).ok())
                    .map(Path::to_path_buf);
                LintTarget {
                    path: relative_path.unwrap_or(file_path),
                    changed_lines: Some(changed_lines),
                }
            })
            .collect();
        Ok(targets)
    } else {
        let mut targets = Vec::new();
        for path in &effective_paths {
            if path.is_file() {
                if Language::from_path(path).is_some() {
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
            (path.is_file() && Language::from_path(path).is_some()).then(|| LintTarget {
                path: path.to_path_buf(),
                changed_lines: None,
            })
        })
}

fn lint_single_file(
    path: &Path,
    config: &Config,
    seed_index: Option<&ProjectIndex<'_>>,
    project: Option<&ProjectContext<'_>>,
    changed_lines: Option<&HashSet<usize>>,
) -> Result<Vec<Diagnostic>, UnreadableFile> {
    if !path.is_file() || Language::from_path(path).is_none() {
        return Ok(Vec::new());
    }

    let content = fs::read_to_string(path).map_err(|error| UnreadableFile {
        path: path.to_path_buf(),
        error,
    })?;

    let diagnostics = lint_file_with_index(path, &content, config, seed_index, project);

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

fn rewrite_diagnostic_paths(diagnostics: &mut [Diagnostic], display_path: &Path) {
    for diagnostic in diagnostics {
        diagnostic.location.context =
            crate::diagnostic::LocationContext::File(display_path.to_path_buf());
    }
}

/// Attaches the `[architecture]` block of `config` to `index` when the configuration governs the
/// index's project: the configuration root is the project root or one of its ancestors (a
/// sub-package with its own `pyproject.toml` stays under the repository's `.omnicoach.toml`).
fn attach_configured_architecture<'a>(
    index: ProjectIndex<'a>,
    config: &Config,
) -> ProjectIndex<'a> {
    let applies_to_project = config.root_dir.as_deref().map_or_else(
        || config.architecture.is_some(),
        |config_root| index.layout().project_root.starts_with(config_root),
    );
    if applies_to_project {
        index.with_architecture_config(config.architecture.clone())
    } else {
        index
    }
}

fn lint_file_with_index(
    path: &Path,
    content: &str,
    config: &Config,
    seed_index: Option<&ProjectIndex<'_>>,
    project: Option<&ProjectContext<'_>>,
) -> Vec<Diagnostic> {
    let Some(lang) = Language::from_path(path) else {
        return Vec::new();
    };

    let is_test = config.is_test_path(path);
    if should_skip_ast_parse(path, lang, content, is_test, project.is_some(), config) {
        return Vec::new();
    }
    let file = ParsedFile::new(content, lang);
    if let Some(index) = seed_index {
        index.seed_from_parsed(path, &file);
    }
    let mut tracker = SuppressionTracker::from_file(&file, content);

    let inline_test_ranges = if !is_test && lang == Language::Rust {
        ast::rust::collect_inline_test_ranges(&file)
    } else {
        &[][..]
    };
    let has_inline_tests = !inline_test_ranges.is_empty();
    let mut raw_diagnostics = Vec::new();
    let mut evaluated_rules = HashSet::new();

    for &rule in CODE_RULES {
        if !is_rule_applicable(rule, path, lang, is_test, has_inline_tests, config) {
            continue;
        }
        let overrides = config.rule_overrides.get(&rule.name());
        if let Some(rule_diagnostics) = rule.check_file_in_project(path, &file, project, overrides)
        {
            evaluated_rules.insert(rule.name().0);
            raw_diagnostics.extend(filter_diagnostics_by_target(
                rule_diagnostics,
                rule.target(),
                is_test,
                inline_test_ranges,
            ));
        }
    }

    let mut diagnostics = tracker.filter_diagnostics(raw_diagnostics);
    diagnostics.extend(tracker.audit(path, config, &SUPPRESSIBLE_RULES, &evaluated_rules));
    diagnostics
}

/// Determines whether AST parsing can be skipped entirely for a file.
///
/// Skips Tree-sitter parsing when neither standard code rules nor active suppression audit
/// directives can produce findings for the file given its language, test-path status, and config.
/// Before parsing, any Rust file may hold inline tests; Omni has no inline Python tests.
#[must_use]
fn should_skip_ast_parse(
    path: &Path,
    lang: Language,
    content: &str,
    is_test: bool,
    has_project: bool,
    config: &Config,
) -> bool {
    let may_have_inline_tests = lang == Language::Rust;
    let has_code_rules = CODE_RULES.iter().any(|&rule| {
        (has_project || !rule.requires_project_context())
            && is_rule_applicable(rule, path, lang, is_test, may_have_inline_tests, config)
    });

    !has_code_rules && !has_active_suppression_audit(path, lang, content, config)
}

/// Returns true if `rule` runs on `path`: it is enabled there, analyzes `lang`, and its
/// [`RuleTarget`] matches.
///
/// `has_inline_tests` says whether a non-test file holds inline tests (Rust `#[cfg(test)]`),
/// which `TestsOnly` rules check.
fn is_rule_applicable(
    rule: &dyn AnyCodeRule,
    path: &Path,
    lang: Language,
    is_test: bool,
    has_inline_tests: bool,
    config: &Config,
) -> bool {
    config.is_rule_enabled_for_path(rule.name(), path)
        && rule.supports_language(lang)
        && match rule.target() {
            RuleTarget::SourceOnly => !is_test,
            RuleTarget::TestsOnly => is_test || has_inline_tests,
            RuleTarget::All => true,
        }
}

/// Returns true if any active suppression hygiene rule is enabled and applicable to `path`,
/// and the file content may contain a suppression directive (see [`contains_directive_prefix`]).
#[must_use]
fn has_active_suppression_audit(
    path: &Path,
    lang: Language,
    content: &str,
    config: &Config,
) -> bool {
    if !contains_directive_prefix(content, lang) {
        return false;
    }
    SUPPRESSION_AUDITS.iter().any(|audit| {
        config.is_rule_enabled_for_path(audit.name, path) && audit.languages.contains(&lang)
    })
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::code_lint::rules;
    use crate::diagnostic::RuleName;
    use crate::rule_declaration::RuleOverrides;
    use rstest::rstest;

    fn code_rule_names() -> impl Iterator<Item = RuleName> {
        CODE_RULES.iter().map(|rule| rule.name())
    }

    fn audit_names() -> impl Iterator<Item = RuleName> {
        SUPPRESSION_AUDITS.iter().map(|audit| audit.name)
    }

    fn code_rule_named(name: &str) -> &'static dyn AnyCodeRule {
        CODE_RULES
            .iter()
            .copied()
            .find(|rule| rule.name().0 == name)
            .expect("registered code rule")
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
        "error-log-in-except",
        "service.py",
        Language::Python,
        (false, false),
        true
    )]
    #[case::python_rule_on_rust_file(
        "error-log-in-except",
        "service.rs",
        Language::Rust,
        (false, false),
        false
    )]
    #[case::tests_only_on_test_file(
        "sleep-in-tests",
        "test_service.py",
        Language::Python,
        (true, false),
        true
    )]
    #[case::tests_only_on_source_file(
        "sleep-in-tests",
        "service.py",
        Language::Python,
        (false, false),
        false
    )]
    #[case::tests_only_on_source_file_with_inline_tests(
        "sleep-in-tests",
        "service.rs",
        Language::Rust,
        (false, true),
        true
    )]
    #[case::source_only_on_source_file(
        "environment-variable-in-function",
        "service.rs",
        Language::Rust,
        (false, false),
        true
    )]
    #[case::source_only_on_test_file(
        "environment-variable-in-function",
        "tests/test_service.rs",
        Language::Rust,
        (true, false),
        false
    )]
    fn test_is_rule_applicable(
        #[case] rule_name: &str,
        #[case] path: &str,
        #[case] lang: Language,
        #[case] (is_test, has_inline_tests): (bool, bool),
        #[case] expected: bool,
    ) {
        let config = Config::default();
        assert_eq!(
            is_rule_applicable(
                code_rule_named(rule_name),
                Path::new(path),
                lang,
                is_test,
                has_inline_tests,
                &config
            ),
            expected
        );
    }

    #[rstest]
    #[case::with_directive(
        "# omni:ignore[error-log-in-except] -- reason\nprint('hi')",
        Config::default(),
        true
    )]
    #[case::without_directive("print('hello world')", Config::default(), false)]
    #[case::with_directive_but_suppression_disabled(
        "# omni:ignore[error-log-in-except] -- reason\nprint('hi')",
        config_disabling(audit_names()),
        false
    )]
    fn test_has_active_suppression_audit(
        #[case] content: &str,
        #[case] config: Config,
        #[case] expected: bool,
    ) {
        assert_eq!(
            has_active_suppression_audit(Path::new("main.py"), Language::Python, content, &config),
            expected
        );
    }

    #[test]
    fn test_should_skip_ast_parse() {
        let config = Config::default();
        let no_rules_config = config_disabling(code_rule_names().chain(audit_names()));
        let suppression_only_config = config_disabling(code_rule_names());
        let py_path = Path::new("script.py");
        let code_with_comment = "# omni:ignore -- missing reason";

        // Normal file with default config: standard code rules are enabled, do NOT skip
        assert!(!should_skip_ast_parse(
            py_path,
            Language::Python,
            "x = 1",
            false,
            false,
            &config
        ));

        // Config disabling every rule: should skip AST parse
        assert!(should_skip_ast_parse(
            py_path,
            Language::Python,
            "x = 1",
            false,
            false,
            &no_rules_config
        ));

        // When all general code rules are ignored, but suppression directives are present:
        // do NOT skip so suppression hygiene can be audited
        assert!(!should_skip_ast_parse(
            py_path,
            Language::Python,
            code_with_comment,
            false,
            false,
            &suppression_only_config
        ));

        // But if that same file has no suppression comments, skip AST parsing
        assert!(should_skip_ast_parse(
            py_path,
            Language::Python,
            "x = 1",
            false,
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

        let declaration = rules::type_cast::RULE.declaration;
        let table: toml::Value =
            toml::from_str(r#"enforcement-mode = "require-explanation""#).expect("valid TOML");
        let overrides = RuleOverrides::parse(
            declaration.name.0,
            &declaration.options.declared(),
            declaration.languages,
            &table,
        )
        .expect("valid options");
        let req_doc_config = Config {
            rule_overrides: HashMap::from([(declaration.name, overrides)]),
            ..config_enabling(&["type-cast"])
        };

        let diagnostics_req_doc =
            lint_file(Path::new("main.py"), source_documented, &req_doc_config);
        let diagnostics_req_uncommented =
            lint_file(Path::new("main.py"), source_uncommented, &req_doc_config);
        assert_eq!(diagnostics_req_doc, vec![]);
        assert_eq!(diagnostics_req_uncommented.len(), 1);
    }

    #[test]
    fn test_rule_target_tests_only_filtering() {
        let source_prod = "import time\ndef run(): time.sleep(5)\n";
        let config = config_enabling(&["sleep-in-tests"]);

        let prod_diagnostics = lint_file(Path::new("src/daemon.py"), source_prod, &config);
        let test_diagnostics = lint_file(Path::new("tests/test_daemon.py"), source_prod, &config);
        assert_eq!(prod_diagnostics, vec![]);
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
        let config = config_enabling(&["sleep-in-tests"]);
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
        let config = config_enabling(&["environment-variable-in-function"]);

        let src_diagnostics = lint_file(Path::new("src/service.py"), source_py, &config);
        let test_diagnostics = lint_file(Path::new("tests/test_service.py"), source_py, &config);
        assert_eq!(src_diagnostics.len(), 1);
        assert_eq!(test_diagnostics, vec![]);
    }

    #[test]
    fn test_valid_inline_suppression_silences_violation() {
        let content = "a = 1  # omni:ignore [single-letter-name] -- math variable";
        let config = config_enabling(&["single-letter-name"]);
        let diags = lint_file(Path::new("math.py"), content, &config);
        assert!(diags.is_empty(), "Expected 0 diagnostics, got: {diags:?}");
    }

    #[rstest]
    #[case::direct("# omni:ignore [single-letter-name] -- math variable\na = 1")]
    #[case::across_decorator(
        indoc::indoc! {r"
            # omni:ignore [single-letter-name] -- math function
            @decorator
            def func(a): pass"}
    )]
    fn test_valid_preceding_line_suppression_silences_violation(#[case] content: &str) {
        let config = config_enabling(&["single-letter-name"]);
        let diags = lint_file(Path::new("math.py"), content, &config);
        assert!(diags.is_empty(), "Expected 0 diagnostics, got: {diags:?}");
    }

    #[test]
    fn test_unused_suppression_flagged() {
        let content = "clean_name = 1  # omni:ignore [single-letter-name] -- math variable";
        let config = config_enabling(&["single-letter-name"]);
        let diags = lint_file(Path::new("clean.py"), content, &config);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].rule_name.0, "unused-suppression");
    }

    #[test]
    fn test_missing_reason_flagged() {
        let content = "a = 1  # omni:ignore [single-letter-name]";
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
        let content = "a = 1  # omni:ignore [single-letter-name] --    ";
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
            # omni:disable-file [nested-function] -- legacy nested functions
            def outer():
                def inner():
                    a = 1
        "};
        let config = config_enabling(&["nested-function", "single-letter-name"]);
        let diags = lint_file(Path::new("src/module.py"), content, &config);

        // nested-function should be suppressed, but single-letter-name should be reported!
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].rule_name.0, "single-letter-name");
    }

    #[test]
    fn test_file_level_unused_suppression_flagged() {
        let content = indoc::indoc! {r"
            # omni:disable-file [error-log-in-except] -- unused file disable
            def clean():
                pass
        "};
        let config = config_enabling(&["error-log-in-except"]);
        let diags = lint_file(Path::new("src/module.py"), content, &config);

        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].rule_name.0, "unused-suppression");
    }

    #[test]
    fn test_suppressing_supp_in_config() {
        let content =
            "clean_name = 1  # omni:ignore [single-letter-name] -- intentional dormant suppression";
        let mut config = config_enabling(&["single-letter-name"]);
        config.disabled_rules.insert(RuleName("unused-suppression"));
        let diags = lint_file(Path::new("src/template.py"), content, &config);

        assert!(diags.is_empty(), "Expected 0 diagnostics, got: {diags:?}");
    }

    #[test]
    fn test_string_literal_does_not_trigger_suppression() {
        let content = r##"sample_text = "# omni:ignore [single-letter-name] -- not a comment""##;
        let config = config_enabling(&["single-letter-name"]);
        let diags = lint_file(Path::new("src/test_case.py"), content, &config);

        // Does not trigger unused-suppression since it's a string literal, not a comment
        assert!(diags.is_empty(), "Expected 0 diagnostics, got: {diags:?}");
    }

    #[test]
    fn test_comment_prefix_word_boundary() {
        // Comments containing 'omni:ignored' should not be treated as omni:ignore directives
        let content = "a = 1  # omni:ignored by other tool";
        let config = config_enabling(&["single-letter-name"]);
        let diags = lint_file(Path::new("src/test.py"), content, &config);

        // Should flag single-letter-name violation, and NOT flag blanket-suppression
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].rule_name.0, "single-letter-name");
    }

    #[rstest]
    #[case::command_rule("a = 1  # omni:ignore [edit-of-described-commit] -- invalid code rule")]
    #[case::suppression_audit("a = 1  # omni:ignore [unused-suppression] -- unsuppressible audit")]
    fn test_unsuppressible_rule_in_code_flagged_as_unknown(#[case] content: &str) {
        let config = Config::default();
        let diags = lint_file(Path::new("src/test.py"), content, &config);

        assert!(
            diags
                .iter()
                .any(|diag| diag.rule_name.0 == "unknown-suppression-rule"),
            "Expected unknown-suppression-rule for non-suppressible rule in code directive, got: {diags:?}"
        );
    }

    #[rstest]
    #[case::disabled_in_config(
        "src/clean.py",
        "clean_name = 1  # omni:ignore [single-letter-name] -- disabled in config",
        config_enabling(&[])
    )]
    #[case::wrong_language(
        "src/clean.rs",
        "pub fn run() {} // omni:ignore [error-log-in-except] -- python-only rule",
        Config::default()
    )]
    #[case::non_matching_target(
        "tests/test_clean.py",
        "def test_ok(): assert True  # omni:ignore [nested-function] -- source-only rule",
        Config::default()
    )]
    fn test_unused_suppression_skips_unevaluated_rules(
        #[case] path: &str,
        #[case] content: &str,
        #[case] config: Config,
    ) {
        let diags = lint_file(Path::new(path), content, &config);
        assert!(
            diags.is_empty(),
            "Expected unevaluated rule suppression not to be flagged as unused, got: {diags:?}"
        );
    }
}
