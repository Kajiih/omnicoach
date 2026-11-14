//! Project source discovery and multi-file `ProjectFacts` dispatch (`CodeLintProjectContext`).

use super::index::ProjectIndex;
use super::layout::ProjectLayout;
use crate::code_lint::project::foundation::{
    ArchitectureError, DiskFileSource, FileSource, ProjectConfig, ProjectFacts, ProjectGraphScope,
    ProjectSourceFile, normalize_relative_path, resolve_against_cwd,
};
use crate::code_lint::project::python::collect_python_project_facts;
use crate::code_lint::project::rust::collect_rust_project_facts;
use crate::diagnostic::Language;
use std::path::{Path, PathBuf};

/// Discovered source files paired with the shared [`ProjectIndex`] governing their repository.
pub(in crate::code_lint) struct DiscoveredProjectSources<'s> {
    index: ProjectIndex<'s>,
    /// Deterministic, deduplicated source files under the requested paths.
    pub(in crate::code_lint) files: Vec<ProjectSourceFile>,
    scope: ProjectGraphScope,
}

impl DiscoveredProjectSources<'_> {
    /// Extracts multi-file [`ProjectFacts`] for `lang` from these discovered sources, seeding the
    /// shared [`ProjectIndex`] from each file's single AST parse and resolving architecture and
    /// cross-module re-exports against [`ProjectIndex`].
    ///
    /// # Errors
    ///
    /// Returns [`ArchitectureError`] when project architecture declarations conflict or fail
    /// validation.
    pub(in crate::code_lint) fn collect_project_facts(
        &self,
        title: &str,
        lang: Language,
    ) -> Result<ProjectFacts, ArchitectureError> {
        match lang {
            Language::Rust => {
                collect_rust_project_facts(title, &self.files, &self.index.rust, self.scope, || {
                    self.index.try_architecture_spec()
                })
            }
            Language::Python => {
                let spec = self.index.try_architecture_spec()?;
                Ok(collect_python_project_facts(
                    title,
                    &self.files,
                    &self.index.python,
                    spec,
                    self.scope,
                ))
            }
        }
    }
}

/// Discovers non-excluded `.rs` and `.py` files under `paths` alongside a shared [`ProjectIndex`]
/// backed by [`DiskFileSource`].
///
/// # Errors
///
/// Returns an error if an explicitly passed file path cannot be read or if an external
/// `.omnicoach.toml` fails validation.
pub(in crate::code_lint) fn discover_project_sources(
    paths: &[PathBuf],
    config: &ProjectConfig,
) -> anyhow::Result<DiscoveredProjectSources<'static>> {
    discover_project_sources_in(paths, &DiskFileSource, config)
}

/// Discovers non-excluded `.rs` and `.py` files under `paths` in `source` alongside a shared
/// [`ProjectIndex`].
///
/// # Errors
///
/// Returns an error if an explicitly passed file path cannot be read from `source` or if an
/// external `.omnicoach.toml` in `source` fails validation.
pub(in crate::code_lint) fn discover_project_sources_in<'s>(
    paths: &[PathBuf],
    source: &'s dyn FileSource,
    config: &ProjectConfig,
) -> anyhow::Result<DiscoveredProjectSources<'s>> {
    let anchor = paths
        .first()
        .map_or_else(|| Path::new("."), PathBuf::as_path);
    let mut index = ProjectIndex::discover_with_config(anchor, source, config)?;
    let configured_roots = index.project_config().configured_source_roots();
    for extra_path in paths.iter().skip(1) {
        let extra_layout = ProjectLayout::discover(extra_path, source, &configured_roots);
        if extra_layout.project_root == index.layout().project_root {
            index.merge_discovered_layout(&extra_layout);
        }
    }

    let mut files = Vec::new();
    for path in paths {
        if source.is_file(path) {
            if let Some(collected) = collect_single_file_source(path, source, index.layout())? {
                files.push(collected);
            }
        } else if source.is_dir(path) {
            collect_directory_source_files(
                path,
                source,
                index.layout(),
                index.project_config(),
                &mut files,
            );
        }
    }
    files.sort_by(|left, right| left.relative_path.cmp(&right.relative_path));
    files.dedup_by(|left, right| left.relative_path == right.relative_path);

    index.merge_inferred_source_roots(&files, source);
    let scope = determine_graph_scope(paths, source, &index);

    Ok(DiscoveredProjectSources {
        index,
        files,
        scope,
    })
}

/// Resolves `paths` (defaulting to `"."` when empty) and verifies that each path exists on disk.
///
/// # Errors
///
/// Returns an error if any resolved path does not exist.
pub(in crate::code_lint) fn resolve_effective_paths(
    paths: &[PathBuf],
) -> anyhow::Result<Vec<PathBuf>> {
    let effective = if paths.is_empty() {
        vec![PathBuf::from(".")]
    } else {
        paths.to_vec()
    };
    for path in &effective {
        if !DiskFileSource.is_file(path) && !DiskFileSource.is_dir(path) {
            anyhow::bail!("Path does not exist: {}", path.display());
        }
    }
    Ok(effective)
}

/// Derives a human-readable project title from `paths` (falling back to the current directory name).
#[must_use]
pub(in crate::code_lint) fn derive_project_title(paths: &[PathBuf]) -> String {
    derive_project_title_with_cwd(paths, DiskFileSource.current_dir().as_deref())
}

fn derive_project_title_with_cwd(paths: &[PathBuf], current_dir: Option<&Path>) -> String {
    if let [only] = paths {
        let resolved = resolve_against_cwd(only, current_dir);
        if let Some(name) = resolved.file_name().and_then(|os| os.to_str())
            && !name.is_empty()
        {
            return name.to_owned();
        }
    }
    "project".to_owned()
}

fn determine_graph_scope(
    paths: &[PathBuf],
    source: &dyn FileSource,
    index: &ProjectIndex<'_>,
) -> ProjectGraphScope {
    let layout = index.layout();
    let resolved_dirs: Vec<PathBuf> = paths
        .iter()
        .filter(|path| source.is_dir(path))
        .map(|path| layout.resolve_query_path(path))
        .collect();
    if layout.covers_source_roots(&resolved_dirs) {
        ProjectGraphScope::FullProject
    } else {
        ProjectGraphScope::Subset
    }
}

fn collect_single_file_source(
    path: &Path,
    source: &dyn FileSource,
    layout: &ProjectLayout,
) -> anyhow::Result<Option<ProjectSourceFile>> {
    let Some(lang) = Language::from_path(path) else {
        return Ok(None);
    };
    let file_source = source.read_to_string(path)?;
    let full_path = layout.resolve_query_path(path);
    let relative_path =
        normalize_relative_path(full_path.strip_prefix(&layout.project_root).unwrap_or(path));
    Ok(Some(ProjectSourceFile {
        full_path,
        relative_path,
        lang,
        source: file_source,
    }))
}

fn collect_directory_source_files(
    dir: &Path,
    source: &dyn FileSource,
    layout: &ProjectLayout,
    config: &ProjectConfig,
    out: &mut Vec<ProjectSourceFile>,
) {
    let resolved_dir = layout.resolve_query_path(dir);
    let allow_test_files = config.is_test_directory(&resolved_dir);
    let Ok(walked_files) = source.walk_files(dir) else {
        return;
    };
    for entry_path in walked_files {
        let Some(lang) = Language::from_path(&entry_path) else {
            continue;
        };
        let full_path = layout.resolve_query_path(&entry_path);
        if !allow_test_files && config.is_test_path(&full_path) {
            continue;
        }
        if layout.is_outside_primary_source_root(&resolved_dir, &full_path, lang) {
            continue;
        }
        let Ok(file_source) = source.read_to_string(&entry_path) else {
            continue;
        };
        let relative_path = normalize_relative_path(
            full_path
                .strip_prefix(&layout.project_root)
                .or_else(|_| full_path.strip_prefix(&resolved_dir))
                .unwrap_or(&entry_path),
        );
        out.push(ProjectSourceFile {
            full_path,
            relative_path,
            lang,
            source: file_source,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::code_lint::project::foundation::{
        ArchitectureConfig, ComponentFact, CrossModuleRefFact, MemoryFileSource,
    };

    #[test]
    fn project_facts_resolve_components_reexports_and_skip_pure_routers() {
        let files = [
            (
                "src/lib.rs",
                indoc::indoc! {r"
                    mod architecture;
                    mod core;
                    mod router;
                    mod runner;
                "},
            ),
            ("src/router.rs", "pub mod nested;\n"),
            (
                "src/architecture.rs",
                indoc::indoc! {r"
                    architecture_component!(Architecture);
                    define_architecture! {
                        Architecture => [],
                        Core => [],
                        Runner => [Core, Architecture],
                    }
                "},
            ),
            (
                "src/core.rs",
                indoc::indoc! {r"
                    architecture_component!(Core);
                    mod detail;
                    pub use self::detail::Config;

                    pub fn parse_input() -> Config {
                        Config
                    }
                "},
            ),
            (
                "src/core/detail.rs",
                indoc::indoc! {r"
                    pub struct Config;
                "},
            ),
            (
                "src/runner.rs",
                indoc::indoc! {r"
                    architecture_component!(Runner);
                    use crate::core::{Config, parse_input};
                    use crate::router;

                    pub fn execute() -> Config {
                        parse_input()
                    }
                "},
            ),
        ];

        let project = collect_virtual_facts(
            "sample_rs",
            Language::Rust,
            &files,
            &ProjectConfig::default(),
        );

        assert_eq!(project.components.len(), 3);
        assert!(
            project
                .modules
                .iter()
                .all(|module| module.module_path != "lib" && module.module_path != "router")
        );
        assert_eq!(
            cross_module_edges(&project),
            vec![
                ("core", None, "core::detail", Some("Config"), 3),
                (
                    "core",
                    Some("parse_input"),
                    "core::detail",
                    Some("Config"),
                    5
                ),
                (
                    "core",
                    Some("parse_input"),
                    "core::detail",
                    Some("Config"),
                    6
                ),
                ("runner", None, "core", Some("parse_input"), 2),
                ("runner", None, "core::detail", Some("Config"), 2),
                ("runner", Some("execute"), "core", Some("parse_input"), 6),
                ("runner", Some("execute"), "core::detail", Some("Config"), 5),
            ]
        );
    }

    #[test]
    fn project_facts_resolve_package_and_relative_imports() {
        let files = [
            (
                "pkg/__init__.py",
                indoc::indoc! {r"
                    from ._core import build_plan
                "},
            ),
            (
                "pkg/_core.py",
                indoc::indoc! {r"
                    from . import _util

                    def build_plan() -> int:
                        return _util.compute_step()
                "},
            ),
            (
                "pkg/_util.py",
                indoc::indoc! {r"
                    def compute_step() -> int:
                        return 1
                "},
            ),
        ];

        let project = collect_virtual_facts(
            "sample_py",
            Language::Python,
            &files,
            &ProjectConfig::default(),
        );

        assert!(!project.has_declared_architecture);
        assert_eq!(project.modules.len(), 3);
        assert!(project.cross_module_refs.iter().any(|reference| {
            reference.source_module == "pkg._core"
                && reference.target_module == "pkg._util"
                && reference.target_symbol.as_deref() == Some("compute_step")
        }));
    }

    #[test]
    fn cross_module_refs_attribute_method_sources_and_associated_fn_targets() {
        let files = [
            (
                "src/core.rs",
                indoc::indoc! {r"
                    pub struct Parser;

                    impl Parser {
                        pub fn parse() -> Self {
                            Self
                        }
                    }
                "},
            ),
            (
                "src/runner.rs",
                indoc::indoc! {r"
                    use crate::core::Parser;

                    pub struct Runner;

                    impl Runner {
                        pub fn execute(&self) -> Parser {
                            Parser::parse()
                        }
                    }
                "},
            ),
        ];

        let project = collect_virtual_facts(
            "sample_methods",
            Language::Rust,
            &files,
            &ProjectConfig::default(),
        );

        assert!(project.cross_module_refs.iter().any(|reference| {
            reference.source_module == "runner"
                && reference.source_symbol.as_deref() == Some("Runner::execute")
                && reference.target_module == "core"
                && reference.target_symbol.as_deref() == Some("Parser::parse")
        }));
    }

    #[test]
    fn project_facts_honor_declared_toml_architecture_and_non_transitive_mode() {
        let files = [
            ("pkg/__init__.py", "from .core import low\n"),
            ("pkg/core.py", "def low() -> int:\n    return 1\n"),
            ("pkg/domain.py", "def compute() -> int:\n    return 1\n"),
            (
                "pkg/api.py",
                indoc::indoc! {"
                    import pkg
                    from pkg.domain import compute

                    def serve() -> int:
                        return compute()
                "},
            ),
            ("pkg/orphan.py", "def stray() -> int:\n    return 0\n"),
        ];
        let architecture_config: ArchitectureConfig = toml::from_str(indoc::indoc! {r#"
            transitive-dependencies = false
            require-component-coverage = false

            [[components]]
            name = "Core"
            roots = ["pkg.core"]

            [[components]]
            name = "Domain"
            roots = ["pkg.domain"]
            depends-on = ["Core"]

            [[components]]
            name = "Api"
            roots = ["pkg.api"]
            depends-on = ["Domain"]
        "#})
        .expect("parse architecture config");
        let config = ProjectConfig {
            architecture: Some(architecture_config),
            ..ProjectConfig::default()
        };

        let project = collect_virtual_facts("sample_py", Language::Python, &files, &config);

        assert!(project.has_declared_architecture);
        assert!(
            project
                .cross_module_refs
                .iter()
                .all(|reference| reference.source_module != "pkg"
                    && reference.target_module != "pkg")
        );
        assert_eq!(
            project.components,
            vec![
                ComponentFact {
                    name: "Core".to_owned(),
                    declared_dependencies: Some(Vec::new()),
                    allowed_dependencies: Vec::new(),
                },
                ComponentFact {
                    name: "Domain".to_owned(),
                    declared_dependencies: Some(vec!["Core".to_owned()]),
                    allowed_dependencies: vec!["Core".to_owned()],
                },
                ComponentFact {
                    name: "Api".to_owned(),
                    declared_dependencies: Some(vec!["Domain".to_owned()]),
                    allowed_dependencies: vec!["Domain".to_owned()],
                },
                ComponentFact {
                    name: "pkg".to_owned(),
                    declared_dependencies: None,
                    allowed_dependencies: Vec::new(),
                },
            ]
        );
    }

    #[test]
    fn project_facts_follow_reexports_and_reexported_submodules_via_project_index() {
        let files = [
            (
                "pkg/__init__.py",
                indoc::indoc! {"
                    from ._core import build_plan
                    from . import _core as core
                "},
            ),
            (
                "pkg/_core.py",
                indoc::indoc! {"
                    def build_plan() -> int:
                        return 1

                    class Parser:
                        @staticmethod
                        def parse() -> int:
                            return 2
                "},
            ),
            (
                "pkg/api.py",
                indoc::indoc! {"
                    import pkg
                    from pkg import build_plan

                    def handle() -> int:
                        return build_plan() + pkg.core.Parser.parse()
                "},
            ),
        ];

        let project = collect_virtual_facts(
            "sample_reexport",
            Language::Python,
            &files,
            &ProjectConfig::default(),
        );

        assert!(project.cross_module_refs.iter().any(|reference| {
            reference.source_module == "pkg.api"
                && reference.source_symbol.as_deref() == Some("handle")
                && reference.target_module == "pkg._core"
                && reference.target_symbol.as_deref() == Some("build_plan")
        }));
        assert!(project.cross_module_refs.iter().any(|reference| {
            reference.source_module == "pkg.api"
                && reference.source_symbol.as_deref() == Some("handle")
                && reference.target_module == "pkg._core"
                && reference.target_symbol.as_deref() == Some("Parser::parse")
        }));
    }

    #[test]
    fn subset_scope_separates_direct_declared_dependencies_from_transitive_allowed_dependencies() {
        let all_files = [
            (
                "src/architecture.rs",
                indoc::indoc! {r"
                    architecture_component!(Architecture);
                    define_architecture! {
                        Architecture => [],
                        Unused => [],
                        Core => [],
                        Runner => [Core],
                        Cli => [Runner],
                    }
                "},
            ),
            (
                "src/unused.rs",
                "architecture_component!(Unused);\npub fn idle() {}\n",
            ),
            (
                "src/core.rs",
                "architecture_component!(Core);\npub fn compute() -> usize { 1 }\n",
            ),
            (
                "src/runner.rs",
                "architecture_component!(Runner);\npub fn run() -> usize { crate::core::compute() }\n",
            ),
            (
                "src/cli.rs",
                "architecture_component!(Cli);\npub fn entry() -> usize { crate::core::compute() }\n",
            ),
        ];
        let root = Path::new(MemoryFileSource::VIRTUAL_ROOT);
        let memory = MemoryFileSource::from_relative_files(root, &all_files);
        let subset_paths = [root.join("src/core.rs"), root.join("src/cli.rs")];
        let discovered =
            discover_project_sources_in(&subset_paths, &memory, &ProjectConfig::default())
                .expect("discover subset sources");

        let project = discovered
            .collect_project_facts("subset_rs", Language::Rust)
            .expect("collect subset project facts");

        assert_eq!(
            project.components,
            vec![
                ComponentFact {
                    name: "Core".to_owned(),
                    declared_dependencies: Some(Vec::new()),
                    allowed_dependencies: Vec::new(),
                },
                ComponentFact {
                    name: "Cli".to_owned(),
                    declared_dependencies: Some(Vec::new()),
                    allowed_dependencies: vec!["Core".to_owned()],
                },
            ]
        );
    }

    #[test]
    fn project_facts_normalize_self_crate_paths_from_cargo_toml() {
        let files = [
            (
                "Cargo.toml",
                indoc::indoc! {r#"
                    [package]
                    name = "my-sample"
                    version = "0.1.0"
                "#},
            ),
            ("src/lib.rs", "pub mod core;\npub mod api;\n"),
            ("src/core.rs", "pub fn compute() -> usize { 1 }\n"),
            (
                "src/api.rs",
                "pub fn serve() -> usize { my_sample::core::compute() }\n",
            ),
        ];

        let project = collect_virtual_facts(
            "my_sample",
            Language::Rust,
            &files,
            &ProjectConfig::default(),
        );

        assert_eq!(
            project.cross_module_refs,
            vec![CrossModuleRefFact {
                source_module: "api".to_owned(),
                source_symbol: Some("serve".to_owned()),
                target_module: "core".to_owned(),
                target_symbol: Some("compute".to_owned()),
                line: 1,
            }]
        );
    }

    #[test]
    fn project_facts_honor_source_roots_and_keep_distinct_out_of_root_scripts() {
        let files = [
            (
                ".omnicoach.toml",
                indoc::indoc! {r#"
                    [context]
                    source-roots = ["python"]
                "#},
            ),
            (
                "python/pkg/core.py",
                "def compute() -> int:\n    return 1\n",
            ),
            (
                "python/pkg/api.py",
                indoc::indoc! {"
                    from pkg.core import compute

                    def serve() -> int:
                        return compute()
                "},
            ),
            ("setup.py", "def build() -> int:\n    return 0\n"),
            (
                "scripts/noxfile.py",
                "def session() -> int:\n    return 0\n",
            ),
        ];

        let project = collect_virtual_facts(
            "custom_root_py",
            Language::Python,
            &files,
            &ProjectConfig::default(),
        );
        let module_paths: Vec<&str> = project
            .modules
            .iter()
            .map(|module| module.module_path.as_str())
            .collect();

        assert_eq!(
            module_paths,
            ["pkg.api", "pkg.core", "scripts.noxfile", "setup"]
        );
        assert_eq!(
            project.cross_module_refs,
            vec![
                CrossModuleRefFact {
                    source_module: "pkg.api".to_owned(),
                    source_symbol: None,
                    target_module: "pkg.core".to_owned(),
                    target_symbol: Some("compute".to_owned()),
                    line: 1,
                },
                CrossModuleRefFact {
                    source_module: "pkg.api".to_owned(),
                    source_symbol: Some("serve".to_owned()),
                    target_module: "pkg.core".to_owned(),
                    target_symbol: Some("compute".to_owned()),
                    line: 4,
                },
            ]
        );
    }

    #[test]
    fn collect_project_facts_propagates_conflicting_architecture_error() {
        let files = [
            (
                ".omnicoach.toml",
                indoc::indoc! {r#"
                    [[architecture.components]]
                    name = "Core"
                    roots = ["core"]
                "#},
            ),
            (
                "src/architecture.rs",
                indoc::indoc! {r"
                    architecture_component!(Architecture);
                    define_architecture! {
                        Architecture => [],
                        Core => [],
                    }
                "},
            ),
            (
                "src/core.rs",
                "architecture_component!(Core);\npub fn compute() -> usize { 1 }\n",
            ),
        ];
        let root = PathBuf::from(MemoryFileSource::VIRTUAL_ROOT);
        let memory = MemoryFileSource::from_relative_files(&root, &files);
        let discovered = discover_project_sources_in(
            std::slice::from_ref(&root),
            &memory,
            &ProjectConfig::default(),
        )
        .expect("discover sources without eager architecture evaluation");

        let error = discovered
            .collect_project_facts("conflict", Language::Rust)
            .expect_err("conflicting architecture should fail");

        assert!(
            error
                .to_string()
                .contains("conflicting architecture declarations")
        );
    }

    #[test]
    fn discover_project_sources_merges_nested_subpackage_roots() {
        let files = [
            (".omnicoach.toml", ""),
            (
                "services/auth/auth_svc/__init__.py",
                "from .tokens import issue_token\n",
            ),
            (
                "services/auth/auth_svc/tokens.py",
                "def issue_token() -> str:\n    return 'ok'\n",
            ),
        ];

        let project = collect_virtual_facts(
            "multi_pkg",
            Language::Python,
            &files,
            &ProjectConfig::default(),
        );
        let module_paths: Vec<&str> = project
            .modules
            .iter()
            .map(|module| module.module_path.as_str())
            .collect();

        assert_eq!(module_paths, ["auth_svc", "auth_svc.tokens"]);
    }

    #[test]
    fn discover_project_sources_includes_explicitly_targeted_test_directory() {
        let files = [
            (
                "Cargo.toml",
                indoc::indoc! {r#"
                    [package]
                    name = "demo"
                    version = "0.1.0"
                "#},
            ),
            ("src/lib.rs", "pub fn prod() -> usize { 1 }\n"),
            (
                "tests/integration_helper.rs",
                "pub fn helper() -> usize { 2 }\n",
            ),
        ];
        let root = PathBuf::from(MemoryFileSource::VIRTUAL_ROOT);
        let tests_dir = root.join("tests");
        let memory = MemoryFileSource::from_relative_files(&root, &files);
        let config = ProjectConfig::default();

        let root_discovered =
            discover_project_sources_in(std::slice::from_ref(&root), &memory, &config)
                .expect("discover root");
        let tests_discovered =
            discover_project_sources_in(std::slice::from_ref(&tests_dir), &memory, &config)
                .expect("discover explicit tests dir");
        let root_paths: Vec<&str> = root_discovered
            .files
            .iter()
            .map(|file| file.relative_path.as_str())
            .collect();
        let test_paths: Vec<&str> = tests_discovered
            .files
            .iter()
            .map(|file| file.relative_path.as_str())
            .collect();

        assert_eq!(root_paths, ["src/lib.rs"]);
        assert_eq!(test_paths, ["tests/integration_helper.rs"]);
    }

    #[test]
    fn derive_project_title_normalizes_parent_dir_components() {
        let nested_parent = [PathBuf::from("services/auth/..")];
        let absolute_parent = [PathBuf::from("/workspace/crates/engine/..")];
        let bare_parent = [PathBuf::from("..")];

        assert_eq!(derive_project_title(&nested_parent), "services");
        assert_eq!(derive_project_title(&absolute_parent), "crates");
        assert_eq!(
            derive_project_title_with_cwd(
                &bare_parent,
                Some(Path::new("/workspace/omnicoach/src"))
            ),
            "omnicoach"
        );
    }

    #[test]
    fn project_facts_skip_pure_package_routers_and_out_of_root_scripts_under_architecture() {
        let files = [
            (
                "pyproject.toml",
                indoc::indoc! {r#"
                    [project]
                    name = "polybot"
                "#},
            ),
            (
                ".omnicoach.toml",
                indoc::indoc! {r#"
                    [[architecture.components]]
                    name = "Core"
                    roots = ["polybot.core"]

                    [[architecture.components]]
                    name = "Api"
                    roots = ["polybot.api"]
                    depends-on = ["Core"]
                "#},
            ),
            ("src/polybot/__init__.py", "from .core import compute\n"),
            (
                "src/polybot/core.py",
                "def compute() -> int:\n    return 1\n",
            ),
            (
                "src/polybot/api.py",
                indoc::indoc! {"
                    from polybot import compute

                    def serve() -> int:
                        return compute()
                "},
            ),
            ("dev/scripts/__init__.py", ""),
            (
                "dev/scripts/check.py",
                indoc::indoc! {"
                    from polybot.core import compute

                    def main() -> int:
                        return compute()
                "},
            ),
        ];

        let project = collect_virtual_facts(
            "polybot",
            Language::Python,
            &files,
            &ProjectConfig::default(),
        );
        let module_paths: Vec<&str> = project
            .modules
            .iter()
            .map(|module| module.module_path.as_str())
            .collect();
        let component_names: Vec<&str> = project
            .components
            .iter()
            .map(|component| component.name.as_str())
            .collect();

        assert_eq!(module_paths, ["polybot.api", "polybot.core"]);
        assert_eq!(component_names, ["Core", "Api"]);
        assert_eq!(
            project.cross_module_refs,
            vec![
                CrossModuleRefFact {
                    source_module: "polybot.api".to_owned(),
                    source_symbol: None,
                    target_module: "polybot.core".to_owned(),
                    target_symbol: Some("compute".to_owned()),
                    line: 1,
                },
                CrossModuleRefFact {
                    source_module: "polybot.api".to_owned(),
                    source_symbol: Some("serve".to_owned()),
                    target_module: "polybot.core".to_owned(),
                    target_symbol: Some("compute".to_owned()),
                    line: 4,
                },
            ]
        );
    }

    #[test]
    fn collect_rust_project_facts_preseeds_index_without_rereading_files() {
        struct CountingFileSource {
            inner: MemoryFileSource,
            reads: std::sync::Mutex<std::collections::HashMap<PathBuf, usize>>,
        }

        impl FileSource for CountingFileSource {
            fn read_to_string(&self, path: &Path) -> std::io::Result<String> {
                let result = self.inner.read_to_string(path);
                if result.is_ok() {
                    *self
                        .reads
                        .lock()
                        .unwrap()
                        .entry(path.to_path_buf())
                        .or_insert(0) += 1;
                }
                result
            }

            fn is_file(&self, path: &Path) -> bool {
                self.inner.is_file(path)
            }

            fn is_dir(&self, path: &Path) -> bool {
                self.inner.is_dir(path)
            }

            fn read_dir(&self, directory: &Path) -> std::io::Result<Vec<PathBuf>> {
                self.inner.read_dir(directory)
            }

            fn current_dir(&self) -> Option<PathBuf> {
                self.inner.current_dir()
            }
        }

        let root = PathBuf::from(MemoryFileSource::VIRTUAL_ROOT);
        let source = CountingFileSource {
            inner: MemoryFileSource::from_relative_files(
                &root,
                &[
                    ("Cargo.toml", "[package]\nname = \"demo\"\n"),
                    (
                        "src/lib.rs",
                        indoc::indoc! {r"
                            mod architecture;
                            mod core;
                            mod runner;
                        "},
                    ),
                    (
                        "src/architecture.rs",
                        indoc::indoc! {r"
                            architecture_component!(Architecture);
                            define_architecture! {
                                Architecture => [],
                                Core => [],
                                Runner => [Core],
                            }
                        "},
                    ),
                    (
                        "src/core.rs",
                        "architecture_component!(Core);\npub fn helper() -> usize { 1 }\n",
                    ),
                    (
                        "src/runner.rs",
                        indoc::indoc! {r"
                            architecture_component!(Runner);
                            use crate::core::helper;
                            pub fn run() -> usize { helper() }
                        "},
                    ),
                ],
            ),
            reads: std::sync::Mutex::new(std::collections::HashMap::new()),
        };

        let discovered = discover_project_sources_in(
            std::slice::from_ref(&root),
            &source,
            &ProjectConfig::default(),
        )
        .expect("discover virtual project sources");
        let facts = discovered
            .collect_project_facts("demo", Language::Rust)
            .expect("collect rust project facts");
        let rs_reads: Vec<(PathBuf, usize)> = source
            .reads
            .lock()
            .unwrap()
            .iter()
            .filter(|(path, _)| {
                path.extension().and_then(|extension| extension.to_str()) == Some("rs")
            })
            .map(|(path, &count)| (path.clone(), count))
            .collect();

        assert_eq!(facts.components.len(), 3);
        assert!(
            rs_reads.iter().all(|(_, count)| *count == 1),
            "expected every `.rs` file to be read once via RustIndex pre-seeding, got: {rs_reads:?}"
        );
    }

    fn collect_virtual_facts(
        title: &str,
        lang: Language,
        files: &[(&str, &str)],
        config: &ProjectConfig,
    ) -> ProjectFacts {
        let root = PathBuf::from(MemoryFileSource::VIRTUAL_ROOT);
        let memory = MemoryFileSource::from_relative_files(&root, files);
        let discovered = discover_project_sources_in(std::slice::from_ref(&root), &memory, config)
            .expect("discover virtual project sources");
        discovered
            .collect_project_facts(title, lang)
            .expect("collect virtual project facts")
    }

    type CrossModuleEdge<'a> = (&'a str, Option<&'a str>, &'a str, Option<&'a str>, usize);

    fn cross_module_edges(project: &ProjectFacts) -> Vec<CrossModuleEdge<'_>> {
        project
            .cross_module_refs
            .iter()
            .map(|reference| {
                (
                    reference.source_module.as_str(),
                    reference.source_symbol.as_deref(),
                    reference.target_module.as_str(),
                    reference.target_symbol.as_deref(),
                    reference.line,
                )
            })
            .collect()
    }
}
