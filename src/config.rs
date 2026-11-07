//! Project configuration resolved from `.omnicoach.toml`.

architecture_component!(Config);

use std::collections::{HashMap, HashSet};
use std::path::{Component, Path, PathBuf};

use serde::Deserialize;

use crate::diagnostic::RuleName;
use crate::rule_declaration::RuleOverrides;

/// The default configuration file name.
pub const CONFIG_FILE_NAME: &str = ".omnicoach.toml";

const DEFAULT_TEST_PATTERNS: &[&str] =
    &["**/tests/**", "**/test_*.py", "**/*_test.py", "**/tests.rs"];

static CACHED_CURRENT_DIR: std::sync::LazyLock<Option<PathBuf>> =
    std::sync::LazyLock::new(|| std::env::current_dir().ok());

/// Configuration settings for path context detection (e.g. test paths) and Python source roots.
#[derive(Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct ContextConfig {
    /// Glob patterns used to identify test files, compiled at load.
    #[serde(
        default = "default_test_patterns",
        deserialize_with = "deserialize_test_patterns"
    )]
    pub test_patterns: globset::GlobSet,
    /// Optional Python import source roots relative to the project root (e.g. `["src", "python"]`).
    #[serde(default)]
    pub source_roots: Vec<String>,
}

impl Default for ContextConfig {
    fn default() -> Self {
        Self {
            test_patterns: default_test_patterns(),
            source_roots: Vec::new(),
        }
    }
}

/// Validation failure in an `[architecture]` configuration block.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ArchitectureConfigError {
    /// Two `[[architecture.components]]` entries share the same `name`.
    #[error("`architecture.components`: duplicate component `{0}`")]
    DuplicateComponent(String),
    /// A component lists a `depends-on` target that was not declared earlier in `components`.
    #[error(
        "`architecture.components`: component `{component}` cannot depend on `{dependency}`; \
         dependencies must be declared earlier in `[[architecture.components]]` to guarantee acyclicity"
    )]
    ForwardOrUnknownDependency {
        /// Dependent component name.
        component: String,
        /// Undeclared or out-of-order dependency name.
        dependency: String,
    },
    /// A component contains a malformed `public-exports` glob pattern.
    #[error(
        "`architecture.components`: component `{component}` has invalid `public-exports` glob `{pattern}`: {message}"
    )]
    InvalidPublicExportGlob {
        /// Component declaring `public-exports`.
        component: String,
        /// Raw invalid glob string.
        pattern: String,
        /// Underlying `globset` error description.
        message: String,
    },
}

/// Project-wide architectural DAG configuration (`[architecture]` in `.omnicoach.toml`).
#[derive(Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct ArchitectureConfig {
    /// Optional Python import source roots relative to the project root.
    #[serde(default)]
    pub source_roots: Vec<String>,
    /// Whether `depends-on` edges are transitive across the component DAG (default `true`).
    #[serde(default = "default_true")]
    pub transitive_dependencies: bool,
    /// Whether every production module must belong to a declared component or pure router (default `true`).
    #[serde(default = "default_true")]
    pub require_component_coverage: bool,
    /// Whether imports inside `if TYPE_CHECKING:` blocks are exempt from dependency checks.
    #[serde(default)]
    pub ignore_type_checking_imports: bool,
    /// Ordered component declarations; each `depends-on` entry must name an earlier component.
    #[serde(default)]
    pub components: Vec<ArchitectureComponentConfig>,
}

impl Default for ArchitectureConfig {
    fn default() -> Self {
        Self {
            source_roots: Vec::new(),
            transitive_dependencies: true,
            require_component_coverage: true,
            ignore_type_checking_imports: false,
            components: Vec::new(),
        }
    }
}

impl ArchitectureConfig {
    /// Validates that component names are unique and every `depends-on` entry names an earlier
    /// component in `[[architecture.components]]`.
    ///
    /// # Errors
    ///
    /// Returns [`ArchitectureConfigError`] on duplicate component names or forward/unknown
    /// `depends-on` references.
    pub fn validate(&self) -> Result<(), ArchitectureConfigError> {
        let mut seen = HashSet::with_capacity(self.components.len());
        for component in &self.components {
            for dependency in &component.depends_on {
                if !seen.contains(dependency.as_str()) {
                    return Err(ArchitectureConfigError::ForwardOrUnknownDependency {
                        component: component.name.clone(),
                        dependency: dependency.clone(),
                    });
                }
            }
            if !seen.insert(component.name.as_str()) {
                return Err(ArchitectureConfigError::DuplicateComponent(
                    component.name.clone(),
                ));
            }
            if let Some(ref patterns) = component.public_exports {
                for pattern in patterns {
                    if let Err(error) = compile_glob(pattern) {
                        return Err(ArchitectureConfigError::InvalidPublicExportGlob {
                            component: component.name.clone(),
                            pattern: pattern.clone(),
                            message: error.to_string(),
                        });
                    }
                }
            }
        }
        Ok(())
    }
}

/// Single component entry (`[[architecture.components]]`) in `.omnicoach.toml`.
#[derive(Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct ArchitectureComponentConfig {
    /// Canonical component name (`CodeLintAst`, `Core`, `Api`).
    pub name: String,
    /// Root module prefix(es) belonging to this component (`["code_lint::ast"]`, `["tach.cli"]`).
    pub roots: Vec<String>,
    /// Directly allowed component dependencies (must appear earlier in `components`).
    #[serde(default)]
    pub depends_on: Vec<String>,
    /// Whether direct child modules and multiple roots of this component are isolated from siblings.
    #[serde(default)]
    pub isolate_children: bool,
    /// External packages or crates encapsulated by this component.
    #[serde(default)]
    pub owns_external: Vec<String>,
    /// Optional exported symbol glob patterns (`["parse_*", "Config"]`).
    #[serde(default)]
    pub public_exports: Option<Vec<String>>,
}

/// Configuration settings resolved from `.omnicoach.toml` by `rule_selection::parse_config`.
#[derive(Debug, Default, Clone)]
pub struct Config {
    /// Rules disabled by `select` and `ignore`.
    pub disabled_rules: HashSet<RuleName>,
    /// Validated `[rules.<name>]` options, keyed by rule.
    pub rule_overrides: HashMap<RuleName, RuleOverrides>,
    /// Path context classifier settings.
    pub context: ContextConfig,
    /// Per-file rule ignores: glob patterns paired with the rules they disable.
    pub per_file_ignores: Vec<(globset::GlobMatcher, HashSet<RuleName>)>,
    /// Project or configuration root directory used to anchor `test-patterns` and `per-file-ignores` globs.
    pub root_dir: Option<PathBuf>,
    /// Problems that do not prevent loading, one message each, such as a `select` or
    /// `ignore` entry that changes no rule's outcome.
    pub warnings: Vec<String>,
    /// Optional validated `[architecture]` block declared in `.omnicoach.toml`.
    pub architecture: Option<ArchitectureConfig>,
}

impl Config {
    /// Returns true if the given path matches any configured test pattern.
    #[must_use]
    pub fn is_test_path(&self, path: &Path) -> bool {
        self.context
            .test_patterns
            .is_match(self.normalize_path_for_glob(path))
    }

    /// Returns true if the given rule is enabled in this configuration.
    #[must_use]
    pub fn is_rule_enabled(&self, rule: RuleName) -> bool {
        !self.disabled_rules.contains(&rule)
    }

    /// Returns true if the given rule is enabled for a specific file path.
    #[must_use]
    pub fn is_rule_enabled_for_path(&self, rule: RuleName, path: &Path) -> bool {
        if !self.is_rule_enabled(rule) {
            return false;
        }
        if self.per_file_ignores.is_empty() {
            return true;
        }

        let normalized = self.normalize_path_for_glob(path);
        !self
            .per_file_ignores
            .iter()
            .any(|(matcher, rules)| matcher.is_match(&normalized) && rules.contains(&rule))
    }

    fn normalize_path_for_glob(&self, path: &Path) -> String {
        let needs_cwd = self.root_dir.as_deref().map_or_else(
            || path.is_absolute(),
            |root| !path.is_absolute() || !path.starts_with(root),
        );
        let cwd = needs_cwd.then_some(CACHED_CURRENT_DIR.as_deref()).flatten();
        normalize_path_with_roots(path, self.root_dir.as_deref(), cwd)
    }
}

/// Normalizes `path` lexically by stripping `.` (`CurDir`) and resolving `..` (`ParentDir`)
/// components without touching the filesystem, returning `"."` when all components collapse.
#[must_use]
pub fn lexical_normalize(path: &Path) -> PathBuf {
    let mut components = Vec::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => match components.last() {
                Some(Component::Normal(_)) => {
                    components.pop();
                }
                Some(Component::RootDir | Component::Prefix(_)) => {}
                Some(Component::ParentDir | Component::CurDir) | None => {
                    components.push(component);
                }
            },
            Component::Prefix(_) | Component::RootDir | Component::Normal(_) => {
                components.push(component);
            }
        }
    }
    if components.is_empty() {
        PathBuf::from(".")
    } else {
        components.iter().collect()
    }
}

/// Compiles `pattern` with `*` matching across `/`, as all config globs do.
///
/// # Errors
///
/// Returns the glob syntax error for an invalid pattern.
pub(crate) fn compile_glob(pattern: &str) -> Result<globset::Glob, globset::Error> {
    globset::GlobBuilder::new(pattern)
        .literal_separator(false)
        .build()
}

const fn default_true() -> bool {
    true
}

fn default_test_patterns() -> globset::GlobSet {
    // The defaults are constants covered by `test_context_test_path_detection`.
    compile_glob_set(DEFAULT_TEST_PATTERNS.iter().copied()).unwrap_or_default()
}

fn deserialize_test_patterns<'de, D>(deserializer: D) -> Result<globset::GlobSet, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let patterns = Vec::<String>::deserialize(deserializer)?;
    compile_glob_set(patterns.iter().map(String::as_str)).map_err(serde::de::Error::custom)
}

fn compile_glob_set<'a>(
    patterns: impl IntoIterator<Item = &'a str>,
) -> Result<globset::GlobSet, globset::Error> {
    let mut builder = globset::GlobSetBuilder::new();
    for pattern in patterns {
        builder.add(compile_glob(pattern)?);
    }
    builder.build()
}

fn normalize_path_with_roots(path: &Path, root_dir: Option<&Path>, cwd: Option<&Path>) -> String {
    let normalized_input = lexical_normalize(path);
    let relative = if normalized_input.is_absolute() {
        root_dir
            .and_then(|root| normalized_input.strip_prefix(root).ok())
            .or_else(|| cwd.and_then(|current| normalized_input.strip_prefix(current).ok()))
            .unwrap_or(&normalized_input)
            .to_path_buf()
    } else if let (Some(root), Some(current)) = (root_dir, cwd)
        && current != root
        && current.starts_with(root)
    {
        lexical_normalize(&current.join(&normalized_input))
            .strip_prefix(root)
            .ok()
            .map_or(normalized_input, Path::to_path_buf)
    } else {
        normalized_input
    };

    relative.to_string_lossy().replace('\\', "/")
}

#[cfg(test)]
mod tests {
    use super::*;

    const LOGGING_RULE: RuleName = RuleName("mock-logging-rule");
    const STYLE_RULE: RuleName = RuleName("mock-style-rule");

    #[rstest::rstest]
    #[case("tests/foo.rs", true)]
    #[case("src/tests/foo.rs", true)]
    #[case("test_calculator.py", true)]
    #[case("foo/test_calculator.py", true)]
    #[case("foo/calculator_test.py", true)]
    #[case("src/tests.rs", true)]
    #[case("src/main.rs", false)]
    #[case("src/code_lint/rules/multi_phase_test.rs", false)]
    #[case("src/calculator.py", false)]
    fn test_context_test_path_detection(#[case] path: &str, #[case] expected: bool) {
        let config = Config::default();
        assert_eq!(config.is_test_path(Path::new(path)), expected);
    }

    #[test]
    fn test_disabled_rules_and_per_file_ignores() {
        let config = Config {
            disabled_rules: HashSet::from([LOGGING_RULE]),
            per_file_ignores: vec![(
                compile_glob("tests/**").unwrap().compile_matcher(),
                HashSet::from([STYLE_RULE]),
            )],
            ..Default::default()
        };

        assert!(!config.is_rule_enabled(LOGGING_RULE));
        assert!(!config.is_rule_enabled_for_path(STYLE_RULE, Path::new("tests/my_test.rs")));
        assert!(config.is_rule_enabled_for_path(STYLE_RULE, Path::new("src/lib.rs")));
    }

    #[test]
    fn test_custom_test_patterns_replace_defaults() {
        let context: ContextConfig = toml::from_str(r#"test-patterns = ["**/spec/**"]"#).unwrap();
        let config = Config {
            context,
            ..Default::default()
        };

        assert!(config.is_test_path(Path::new("app/spec/model.py")));
        assert!(!config.is_test_path(Path::new("tests/foo.rs")));
    }

    #[rstest::rstest]
    #[case::test_patterns("test-patterns = [\"src/[\"]")]
    fn test_invalid_glob_is_config_error(#[case] toml_content: &str) {
        let error = toml::from_str::<ContextConfig>(toml_content).unwrap_err();
        assert!(error.to_string().contains("src/["), "{error}");
    }

    #[rstest::rstest]
    #[case::absolute_under_root("/repo/tests/cli.rs", Some("/repo"), Some("/repo"), "tests/cli.rs")]
    #[case::relative_from_root("tests/cli.rs", Some("/repo"), Some("/repo"), "tests/cli.rs")]
    #[case::relative_from_subdir("cli.rs", Some("/repo"), Some("/repo/tests"), "tests/cli.rs")]
    #[case::parent_traversal_from_subdir(
        "../src/lib.rs",
        Some("/repo"),
        Some("/repo/tests"),
        "src/lib.rs"
    )]
    #[case::dot_slash_stripped("./tests/cli.rs", None, None, "tests/cli.rs")]
    fn test_normalize_path_with_roots(
        #[case] input: &str,
        #[case] root_dir: Option<&str>,
        #[case] cwd: Option<&str>,
        #[case] expected: &str,
    ) {
        assert_eq!(
            normalize_path_with_roots(
                Path::new(input),
                root_dir.map(Path::new),
                cwd.map(Path::new),
            ),
            expected
        );
    }

    #[test]
    fn test_root_dir_strips_prefix_for_per_file_ignores() {
        let config = Config {
            per_file_ignores: vec![(
                compile_glob("tests/**").unwrap().compile_matcher(),
                HashSet::from([STYLE_RULE]),
            )],
            root_dir: Some(PathBuf::from("/repo")),
            ..Default::default()
        };

        // Absolute path under root
        assert!(!config.is_rule_enabled_for_path(STYLE_RULE, Path::new("/repo/tests/cli.rs")));
        assert!(config.is_rule_enabled_for_path(STYLE_RULE, Path::new("/repo/src/lib.rs")));

        // Path relative to root
        assert!(!config.is_rule_enabled_for_path(STYLE_RULE, Path::new("tests/cli.rs")));
        assert!(config.is_rule_enabled_for_path(STYLE_RULE, Path::new("src/lib.rs")));
    }

    #[test]
    fn test_validate_rejects_invalid_public_export_glob() {
        let architecture = ArchitectureConfig {
            components: vec![ArchitectureComponentConfig {
                name: "Auth".to_owned(),
                roots: vec!["pkg.auth".to_owned()],
                depends_on: Vec::new(),
                isolate_children: false,
                owns_external: Vec::new(),
                public_exports: Some(vec!["[unclosed".to_owned()]),
            }],
            ..ArchitectureConfig::default()
        };
        let error = architecture
            .validate()
            .expect_err("unclosed bracket glob in public-exports must fail validation");
        assert!(matches!(
            error,
            ArchitectureConfigError::InvalidPublicExportGlob { ref component, ref pattern, .. }
                if component == "Auth" && pattern == "[unclosed"
        ));
    }
}
