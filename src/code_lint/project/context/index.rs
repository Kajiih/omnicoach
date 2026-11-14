//! Memoized cross-file module summary index (`CodeLintProjectContext`).

use super::layout::ProjectLayout;
use crate::code_lint::ast::ParsedFile;
use crate::code_lint::project::foundation::{
    ArchitectureConfig, ArchitectureError, ArchitectureSpec, CONFIG_FILE_NAME,
    FileArchitectureFindings, FileSource, ProjectConfig, ProjectSourceFile, ProjectTomlError,
    build_architecture_spec, is_vcs_boundary,
};
use crate::code_lint::project::python::PythonIndex;
use crate::code_lint::project::rust::RustIndex;
use crate::diagnostic::Language;
use std::collections::HashMap;
use std::hash::{DefaultHasher, Hash as _, Hasher as _};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

type CachedArchitectureFindings = (SourceFingerprint, Arc<FileArchitectureFindings>);

/// Memoized cross-file summary index for a single CLI, hook, or test invocation.
pub struct ProjectIndex<'s> {
    layout: ProjectLayout,
    project_config: ProjectConfig,
    /// Per-language Python module summary index.
    pub(super) python: PythonIndex<'s>,
    /// Per-language Rust module header index.
    pub(super) rust: RustIndex<'s>,
    resolved_architecture: OnceLock<Result<Option<ArchitectureSpec>, ArchitectureError>>,
    architecture_findings_cache: Mutex<HashMap<PathBuf, CachedArchitectureFindings>>,
}

impl<'s> ProjectIndex<'s> {
    /// Constructs an on-demand memoized [`ProjectIndex`] for `layout` and `source`.
    #[must_use]
    pub fn new(layout: ProjectLayout, source: &'s dyn FileSource) -> Self {
        let python = PythonIndex::new(layout.python.clone(), source);
        let rust = RustIndex::new(layout.rust.clone(), source);
        Self {
            layout,
            project_config: ProjectConfig::default(),
            python,
            rust,
            resolved_architecture: OnceLock::new(),
            architecture_findings_cache: Mutex::new(HashMap::new()),
        }
    }

    /// Attaches a [`ProjectConfig`] resolved for this project.
    #[must_use]
    pub(super) fn with_project_config(mut self, project_config: ProjectConfig) -> Self {
        self.project_config = project_config;
        self
    }

    /// Discovers [`ProjectLayout`] from `anchor` on `source` and constructs a [`ProjectIndex`]
    /// configured from `config` when `config` governs the discovered project root, or from
    /// the nearest ancestor `.omnicoach.toml` when present on `source`.
    ///
    /// # Errors
    ///
    /// Returns [`ProjectTomlError`] if an external `.omnicoach.toml` fails TOML deserialization
    /// or `[architecture]` validation.
    pub fn discover_with_config(
        anchor: &Path,
        source: &'s dyn FileSource,
        config: &ProjectConfig,
    ) -> Result<Self, ProjectTomlError> {
        let configured_roots = config.configured_source_roots();
        let base_layout = ProjectLayout::discover(anchor, source, &configured_roots);
        Self::from_layout_and_config(base_layout, source, config)
    }

    /// Constructs a [`ProjectIndex`] from an already-discovered `base_layout`, applying `config`
    /// when it governs `base_layout.project_root` or loading the nearest ancestor `.omnicoach.toml`
    /// from `source`.
    ///
    /// # Errors
    ///
    /// Returns [`ProjectTomlError`] if an external `.omnicoach.toml` fails TOML deserialization
    /// or `[architecture]` validation.
    pub fn from_layout_and_config(
        base_layout: ProjectLayout,
        source: &'s dyn FileSource,
        config: &ProjectConfig,
    ) -> Result<Self, ProjectTomlError> {
        let external_config_file = find_external_config_file(&base_layout.project_root, source);
        let applies_to_project = config.root_dir.as_deref().map_or_else(
            || {
                config.architecture.is_some()
                    || !config.context.source_roots.is_empty()
                    || external_config_file.is_none()
            },
            |config_root| {
                let resolved_config_root = base_layout.resolve_query_path(config_root);
                base_layout.project_root == resolved_config_root
                    || (base_layout.project_root.starts_with(&resolved_config_root)
                        && external_config_file.as_ref().is_none_or(|external_path| {
                            external_path.parent() == Some(resolved_config_root.as_path())
                        }))
            },
        );
        if applies_to_project {
            let mut effective_config = config.clone();
            let resolved_root = effective_config.root_dir.as_deref().map_or_else(
                || base_layout.project_root.clone(),
                |root| base_layout.resolve_query_path(root),
            );
            effective_config.root_dir = Some(resolved_root);
            let configured_roots = effective_config.configured_source_roots();
            let layout = base_layout.with_configured_roots(&configured_roots);
            return Ok(Self::new(layout, source).with_project_config(effective_config));
        }
        let Some(external_config_path) = external_config_file else {
            let default_config = ProjectConfig {
                root_dir: Some(base_layout.project_root.clone()),
                ..ProjectConfig::default()
            };
            return Ok(Self::new(base_layout, source).with_project_config(default_config));
        };
        let Ok(content) = source.read_to_string(&external_config_path) else {
            let default_config = ProjectConfig {
                root_dir: Some(base_layout.project_root.clone()),
                ..ProjectConfig::default()
            };
            return Ok(Self::new(base_layout, source).with_project_config(default_config));
        };
        let mut external = ProjectConfig::from_toml_str(&content)?;
        external.root_dir = external_config_path
            .parent()
            .map(Path::to_path_buf)
            .or_else(|| Some(base_layout.project_root.clone()));
        let external_roots = external.configured_source_roots();
        let final_layout = base_layout.with_configured_roots(&external_roots);
        Ok(Self::new(final_layout, source).with_project_config(external))
    }

    /// Project layout used by this index.
    #[must_use]
    pub const fn layout(&self) -> &ProjectLayout {
        &self.layout
    }

    /// Rule-registry-independent project configuration resolved for this index.
    #[must_use]
    pub const fn project_config(&self) -> &ProjectConfig {
        &self.project_config
    }

    /// Merges additional inferred source roots from `other` discovered inside the same
    /// `project_root` when source roots were not explicitly configured.
    pub(in crate::code_lint) fn merge_discovered_layout(&mut self, other: &ProjectLayout) {
        if !self.project_config.has_configured_source_roots() {
            self.python
                .layout_mut()
                .merge_source_roots(&other.python.source_roots);
            self.layout.python.clone_from(self.python.layout());
        }
    }

    /// Scans `files` for nested subpackages under `project_root` and merges their inferred source
    /// roots when source roots were not explicitly configured.
    pub(super) fn merge_inferred_source_roots(
        &mut self,
        files: &[ProjectSourceFile],
        source: &dyn FileSource,
    ) {
        if !self.project_config.has_configured_source_roots() {
            self.python
                .layout_mut()
                .merge_inferred_subpackage_roots(files, source);
            self.layout.python.clone_from(self.python.layout());
        }
    }

    /// Seeds the memoized summary for `path` from an already-parsed [`ParsedFile`], avoiding a
    /// second parse of linted target files when they are also imported by another module.
    pub fn seed_from_parsed(&self, path: &Path, parsed: &ParsedFile) {
        match parsed.lang() {
            Language::Python => self.python.seed_from_parsed(path, parsed),
            Language::Rust => self.rust.seed_from_parsed(path, parsed),
        }
    }

    /// Lazily resolves and caches the project's [`ArchitectureSpec`] (from `.omnicoach.toml`
    /// `[architecture]` or Rust `define_architecture!` / `architecture_component!` headers).
    #[must_use]
    pub fn architecture_spec(&self) -> Option<&ArchitectureSpec> {
        self.try_architecture_spec().ok().flatten()
    }

    /// Lazily resolves and caches the project's [`ArchitectureSpec`], returning any validation or
    /// declaration-conflict [`ArchitectureError`].
    ///
    /// # Errors
    ///
    /// Returns [`ArchitectureError`] if both `.omnicoach.toml` `[[architecture.components]]` and
    /// Rust `define_architecture!` exist, or if DAG validation fails.
    pub(super) fn try_architecture_spec(
        &self,
    ) -> Result<Option<&ArchitectureSpec>, ArchitectureError> {
        if let Some(resolved) = self.resolved_architecture.get() {
            return resolved.as_ref().map(Option::as_ref).map_err(Clone::clone);
        }
        let resolved = resolve_workspace_architecture_spec(
            self.project_config.architecture.as_ref(),
            self.rust.macro_architecture_config().as_ref(),
        );
        self.resolved_architecture
            .get_or_init(|| resolved)
            .as_ref()
            .map(Option::as_ref)
            .map_err(Clone::clone)
    }

    /// Returns the [`ArchitectureError`] encountered while resolving this project's
    /// [`ArchitectureSpec`], if an architecture evaluation was attempted and failed.
    #[must_use]
    pub(crate) fn architecture_error(&self) -> Option<&ArchitectureError> {
        match self.resolved_architecture.get() {
            Some(Err(error)) => Some(error),
            _ => None,
        }
    }

    /// Evaluates (or returns cached) architectural conformance findings for `file` at `path`.
    #[must_use]
    pub(super) fn file_architecture_findings(
        &self,
        file: &ParsedFile,
        path: &Path,
    ) -> Option<Arc<FileArchitectureFindings>> {
        let resolved = self.try_architecture_spec().ok()?;
        let source_text = file.source_text();
        let mode = match file.lang() {
            Language::Python => ArchitectureEvalMode::Python(resolved?),
            Language::Rust => {
                if resolved.is_none()
                    && !RustIndex::has_unconfigured_architecture_candidates(&source_text)
                {
                    return None;
                }
                ArchitectureEvalMode::Rust(resolved)
            }
        };
        let fingerprint = source_fingerprint(&source_text);
        let normalized_path = self.layout.resolve_query_path(path);
        {
            let guard = self
                .architecture_findings_cache
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if let Some((cached_fingerprint, cached)) = guard.get(&normalized_path)
                && *cached_fingerprint == fingerprint
            {
                return Some(Arc::clone(cached));
            }
        }
        let evaluated = Arc::new(match mode {
            ArchitectureEvalMode::Python(spec) => {
                self.python
                    .evaluate_file_architecture(file, &normalized_path, spec)
            }
            ArchitectureEvalMode::Rust(spec) => {
                self.rust
                    .evaluate_file_architecture(file, &normalized_path, spec)
            }
        });
        {
            let mut guard = self
                .architecture_findings_cache
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            guard.insert(normalized_path, (fingerprint, Arc::clone(&evaluated)));
        }
        Some(evaluated)
    }
}

enum ArchitectureEvalMode<'a> {
    Python(&'a ArchitectureSpec),
    Rust(Option<&'a ArchitectureSpec>),
}

fn resolve_workspace_architecture_spec(
    toml_config: Option<&ArchitectureConfig>,
    macro_config: Option<&ArchitectureConfig>,
) -> Result<Option<ArchitectureSpec>, ArchitectureError> {
    let has_toml_components = toml_config.is_some_and(|config| !config.components.is_empty());
    if let Some(synthesized) = macro_config {
        if has_toml_components {
            return Err(ArchitectureError::ConflictingDeclarations);
        }
        return build_architecture_spec(synthesized, true).map(Some);
    }
    match toml_config {
        Some(config) if has_toml_components => build_architecture_spec(config, false).map(Some),
        _ => Ok(None),
    }
}

fn find_external_config_file(start_dir: &Path, source: &dyn FileSource) -> Option<PathBuf> {
    let mut current = Some(start_dir);
    while let Some(directory) = current {
        let candidate = directory.join(CONFIG_FILE_NAME);
        if source.is_file(&candidate) {
            return Some(candidate);
        }
        if is_vcs_boundary(directory, source) {
            break;
        }
        current = directory.parent();
    }
    None
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SourceFingerprint {
    hash: u64,
    byte_len: usize,
}

fn source_fingerprint(source: &str) -> SourceFingerprint {
    let mut hasher = DefaultHasher::new();
    source.hash(&mut hasher);
    SourceFingerprint {
        hash: hasher.finish(),
        byte_len: source.len(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::code_lint::project::foundation::{
        InvalidArchitectureComponentKind, MemoryFileSource,
    };
    use rayon::prelude::*;

    #[test]
    fn architecture_spec_finds_component_declared_after_a_test_gated_item() {
        let root = Path::new("/repo");
        let source = MemoryFileSource::from_relative_files(
            root,
            &[
                ("Cargo.toml", "[package]\nname = \"demo\"\n"),
                ("src/lib.rs", "mod architecture;\nmod core;\n"),
                (
                    "src/architecture.rs",
                    indoc::indoc! {r"
                        define_architecture! {
                            Core => [],
                        }
                    "},
                ),
                (
                    "src/core.rs",
                    indoc::indoc! {r"
                        #[cfg(test)]
                        mod tests;

                        architecture_component!(Core);
                    "},
                ),
            ],
        );
        let index = ProjectIndex::new(ProjectLayout::discover(root, &source, &[]), &source);

        let spec = index
            .architecture_spec()
            .expect("define_architecture! should yield a spec");

        assert_eq!(
            spec.components
                .iter()
                .map(|component| (component.name.as_str(), component.roots.clone()))
                .collect::<Vec<_>>(),
            [("Core", vec!["core".to_owned()])]
        );
    }

    #[rstest::rstest]
    #[case::component_macros_without_define_architecture(
        "mod core;\n",
        &[("src/core.rs", "architecture_component!(Core);\n")]
    )]
    #[case::define_architecture_in_crate_root_lib_rs(
        indoc::indoc! {r"
            define_architecture! {
                Core => [],
            }
            mod core;
        "},
        &[("src/core.rs", "architecture_component!(Core);\n")]
    )]
    #[case::define_architecture_in_non_architecture_child_module(
        "mod layers;\n",
        &[(
            "src/layers.rs",
            indoc::indoc! {r"
                define_architecture! {
                    Core => [],
                }
            "},
        )]
    )]
    #[case::define_architecture_below_nested_submodule(
        "mod app;\n",
        &[
            ("src/app.rs", "mod architecture;\n"),
            (
                "src/app/architecture.rs",
                indoc::indoc! {r"
                    define_architecture! {
                        Core => [],
                    }
                "},
            ),
        ]
    )]
    fn architecture_spec_reads_define_architecture_only_from_architecture_module(
        #[case] lib_rs: &str,
        #[case] modules: &[(&str, &str)],
    ) {
        let root = Path::new("/repo");
        let mut files = vec![
            ("Cargo.toml", "[package]\nname = \"demo\"\n"),
            ("src/lib.rs", lib_rs),
        ];
        files.extend_from_slice(modules);
        let source = MemoryFileSource::from_relative_files(root, &files);
        let index = ProjectIndex::new(ProjectLayout::discover(root, &source, &[]), &source);

        assert!(index.architecture_spec().is_none());
    }

    #[test]
    fn architecture_spec_accepts_directory_style_architecture_mod_rs() {
        let root = Path::new("/repo");
        let source = MemoryFileSource::from_relative_files(
            root,
            &[
                ("Cargo.toml", "[package]\nname = \"demo\"\n"),
                ("src/lib.rs", "mod architecture;\nmod core;\n"),
                (
                    "src/architecture/mod.rs",
                    indoc::indoc! {r"
                        define_architecture! {
                            Core => [],
                        }
                    "},
                ),
                ("src/core.rs", "architecture_component!(Core);\n"),
            ],
        );
        let index = ProjectIndex::new(ProjectLayout::discover(root, &source, &[]), &source);

        let spec = index
            .architecture_spec()
            .expect("src/architecture/mod.rs should yield a spec");
        assert_eq!(spec.components.len(), 1);
        assert_eq!(spec.components[0].roots, vec!["core".to_owned()]);
    }

    #[test]
    fn duplicate_component_macro_keeps_first_root_and_flags_duplicate_without_unmapped_cascade() {
        let root = Path::new("/repo");
        let core_src = indoc::indoc! {r"
            architecture_component!(Core);
            architecture_component!(App);

            pub fn run() {}
        "};
        let source = MemoryFileSource::from_relative_files(
            root,
            &[
                ("Cargo.toml", "[package]\nname = \"demo\"\n"),
                ("src/lib.rs", "mod architecture;\npub mod core;\n"),
                (
                    "src/architecture.rs",
                    indoc::indoc! {r"
                        define_architecture! {
                            Core => [],
                            App => [Core],
                        }
                    "},
                ),
                ("src/core.rs", core_src),
            ],
        );
        let index = ProjectIndex::new(ProjectLayout::discover(root, &source, &[]), &source);
        let spec = index
            .architecture_spec()
            .expect("src/architecture.rs should yield a spec");
        let parsed = ParsedFile::rust(core_src);
        let findings = index
            .file_architecture_findings(&parsed, &root.join("src/core.rs"))
            .expect("Rust file with architecture spec should produce findings");

        assert_eq!(
            spec.component_roots().get("core").map(String::as_str),
            Some("Core")
        );
        assert_eq!(findings.unmapped_architecture_modules, Vec::new());
        assert_eq!(findings.invalid_architecture_components.len(), 1);
        assert_eq!(
            findings.invalid_architecture_components[0].kind,
            InvalidArchitectureComponentKind::DuplicateInModule {
                first_component: "Core".to_owned(),
            }
        );
    }

    #[test]
    fn stray_component_macro_without_architecture_spec_is_reported_only_when_present() {
        let root = Path::new("/repo");
        let core_src = "architecture_component!(Core);\npub fn run() {}\n";
        let util_src = "pub fn helper() {}\n";
        let source = MemoryFileSource::from_relative_files(
            root,
            &[
                ("Cargo.toml", "[package]\nname = \"demo\"\n"),
                ("src/lib.rs", "pub mod core;\npub mod util;\n"),
                ("src/core.rs", core_src),
                ("src/util.rs", util_src),
            ],
        );
        let index = ProjectIndex::new(ProjectLayout::discover(root, &source, &[]), &source);
        let parsed_util = ParsedFile::rust(util_src);
        let util_findings =
            index.file_architecture_findings(&parsed_util, &root.join("src/util.rs"));
        let parsed_core = ParsedFile::rust(core_src);
        let core_findings = index
            .file_architecture_findings(&parsed_core, &root.join("src/core.rs"))
            .expect("stray architecture_component! should produce findings");

        assert!(index.architecture_spec().is_none());
        assert!(util_findings.is_none());
        assert_eq!(core_findings.invalid_architecture_components.len(), 1);
        assert_eq!(
            core_findings.invalid_architecture_components[0].kind,
            InvalidArchitectureComponentKind::MissingDefineArchitecture
        );
    }

    fn architecture_stress_source(root: &Path) -> MemoryFileSource {
        use std::fmt::Write as _;

        let mut owned_files = vec![
            (
                "Cargo.toml".to_owned(),
                "[package]\nname = \"demo\"\n".to_owned(),
            ),
            (
                "src/architecture.rs".to_owned(),
                indoc::indoc! {r"
                    architecture_component!(Architecture);
                    define_architecture! {
                        Architecture => [],
                        Core => [],
                    }
                "}
                .to_owned(),
            ),
            (
                "src/core.rs".to_owned(),
                "architecture_component!(Core);\n".to_owned(),
            ),
        ];
        let mut mod_decls = String::from("mod architecture;\npub mod core;\n");
        for item_index in 0..48 {
            let mod_name = format!("unit_{item_index}");
            let _ = writeln!(mod_decls, "pub mod {mod_name};");
            owned_files.push((
                format!("src/{mod_name}.rs"),
                format!(
                    "// Mentions architecture_component! in comment {item_index}\npub fn run() {{}}\n"
                ),
            ));
        }
        owned_files.push(("src/lib.rs".to_owned(), mod_decls));
        let refs: Vec<(&str, &str)> = owned_files
            .iter()
            .map(|(path, content)| (path.as_str(), content.as_str()))
            .collect();
        MemoryFileSource::from_relative_files(root, &refs)
    }

    #[test]
    fn concurrent_rayon_and_external_thread_architecture_spec_queries_do_not_deadlock() {
        let root = Path::new("/repo");
        let source = architecture_stress_source(root);
        let layout = ProjectLayout::discover(root, &source, &[]);
        let indices: Vec<ProjectIndex<'_>> = (0..16)
            .map(|_| ProjectIndex::new(layout.clone(), &source))
            .collect();

        let (external_counts, rayon_counts) = std::thread::scope(|scope| {
            let external = scope.spawn(|| {
                indices
                    .iter()
                    .map(|index| {
                        index
                            .architecture_spec()
                            .map_or(0, |spec| spec.components.len())
                    })
                    .collect::<Vec<_>>()
            });
            let pool_counts: Vec<usize> = (0..256)
                .into_par_iter()
                .map(|slot| {
                    indices[slot % indices.len()]
                        .architecture_spec()
                        .map_or(0, |spec| spec.components.len())
                })
                .collect();
            (
                external.join().expect("external thread must not panic"),
                pool_counts,
            )
        });

        assert_eq!(external_counts, vec![2; 16]);
        assert_eq!(rayon_counts, vec![2; 256]);
    }

    #[test]
    fn discover_with_config_rejects_invalid_external_project_toml() {
        let root = Path::new("/external");
        let source = MemoryFileSource::from_relative_files(
            root,
            &[(".omnicoach.toml", "[[architecture.components\n")],
        );
        let caller_config = ProjectConfig {
            root_dir: Some(PathBuf::from("/caller")),
            ..ProjectConfig::default()
        };

        let result = ProjectIndex::discover_with_config(root, &source, &caller_config);

        assert!(matches!(result, Err(ProjectTomlError::Toml(_))));
    }

    #[test]
    fn merge_discovered_layout_preserves_explicit_configured_roots() {
        let root = Path::new("/repo");
        let source = MemoryFileSource::from_relative_files(
            root,
            &[
                (
                    ".omnicoach.toml",
                    "[context]\nsource-roots = [\"python\"]\n",
                ),
                ("python/pkg/__init__.py", ""),
            ],
        );
        let caller_config = ProjectConfig {
            root_dir: Some(PathBuf::from("/other")),
            ..ProjectConfig::default()
        };
        let mut index = ProjectIndex::discover_with_config(root, &source, &caller_config)
            .expect("valid external TOML");
        let extra_layout = ProjectLayout::discover(root, &source, &[PathBuf::from("/repo/extra")]);

        index.merge_discovered_layout(&extra_layout);

        assert_eq!(
            index.layout().python.source_roots,
            vec![PathBuf::from("/repo/python")]
        );
    }

    #[test]
    fn file_architecture_findings_do_not_reuse_stub_spans_for_implementation_file() {
        let root = Path::new("/repo");
        let impl_source = "def stray() -> int:\n    return 1\n";
        let stub_source = "# long header padding so stub spans exceed impl file length\n".repeat(4)
            + "def stub_only() -> int: ...\n";
        let source = MemoryFileSource::from_relative_files(
            root,
            &[
                (
                    ".omnicoach.toml",
                    indoc::indoc! {r#"
                        [architecture]
                        require-component-coverage = true

                        [[architecture.components]]
                        name = "Core"
                        roots = ["pkg.core"]
                    "#},
                ),
                ("pkg/__init__.py", ""),
                ("pkg/core.py", "def ok() -> int:\n    return 1\n"),
                ("pkg/orphan.py", impl_source),
                ("pkg/orphan.pyi", &stub_source),
            ],
        );
        let caller_config = ProjectConfig {
            root_dir: Some(PathBuf::from("/other")),
            ..ProjectConfig::default()
        };
        let index = ProjectIndex::discover_with_config(root, &source, &caller_config)
            .expect("valid external TOML");
        let _ = index.python.summary("pkg.orphan");
        let parsed_impl = ParsedFile::new(impl_source, Language::Python);

        let findings = index
            .file_architecture_findings(&parsed_impl, Path::new("/repo/pkg/orphan.py"))
            .expect("findings should resolve");

        assert_eq!(findings.unmapped_architecture_modules.len(), 1);
        assert_eq!(findings.unmapped_architecture_modules[0].name, "stray");
        assert!(findings.unmapped_architecture_modules[0].span.end <= impl_source.len());
    }

    #[test]
    fn discover_with_config_finds_ancestor_omnicoach_toml_from_subpackage_and_preserves_context() {
        let root = Path::new("/monorepo");
        let source = MemoryFileSource::from_relative_files(
            root,
            &[
                (".jj/repo", ""),
                (
                    ".omnicoach.toml",
                    indoc::indoc! {r#"
                        [context]
                        source-roots = ["packages/app/src"]
                    "#},
                ),
                (
                    "packages/app/pyproject.toml",
                    indoc::indoc! {r#"
                        [project]
                        name = "app"
                    "#},
                ),
                ("packages/app/src/app/__init__.py", ""),
            ],
        );
        let caller_config = ProjectConfig {
            root_dir: Some(PathBuf::from("/unrelated")),
            ..ProjectConfig::default()
        };
        let index = ProjectIndex::discover_with_config(
            &root.join("packages/app/src/app/__init__.py"),
            &source,
            &caller_config,
        )
        .expect("should load ancestor .omnicoach.toml");

        assert_eq!(
            index.layout().python.source_roots,
            vec![root.join("packages/app/src")]
        );
    }

    #[test]
    fn find_external_config_file_stops_at_git_worktree_file_boundary() {
        let outer = Path::new("/outer");
        let source = MemoryFileSource::from_relative_files(
            outer,
            &[
                (".omnicoach.toml", "[context]\nsource-roots = [\"wrong\"]\n"),
                ("worktree/.git", "gitdir: /outer/.git/worktrees/wt\n"),
                ("worktree/pkg/__init__.py", ""),
            ],
        );

        assert_eq!(
            find_external_config_file(&outer.join("worktree/pkg"), &source),
            None
        );
    }

    #[test]
    fn file_architecture_findings_recomputes_when_buffer_changes_at_same_path() {
        let root = Path::new("/repo");
        let source = MemoryFileSource::from_relative_files(
            root,
            &[
                (
                    ".omnicoach.toml",
                    indoc::indoc! {r#"
                        [[architecture.components]]
                        name = "Core"
                        roots = ["pkg.core"]

                        [[architecture.components]]
                        name = "Api"
                        roots = ["pkg.api"]
                    "#},
                ),
                ("pkg/__init__.py", ""),
                ("pkg/core.py", "def run() -> None: pass\n"),
                ("pkg/api.py", ""),
            ],
        );
        let index =
            ProjectIndex::discover_with_config(root, &source, &ProjectConfig::default()).unwrap();
        let api_path = root.join("pkg/api.py");
        let clean = ParsedFile::new("x = 1\n", Language::Python);
        let violating = ParsedFile::new("from pkg.core import run\n", Language::Python);

        let clean_findings = index.file_architecture_findings(&clean, &api_path).unwrap();
        let fresh_findings = index
            .file_architecture_findings(&violating, &api_path)
            .unwrap();

        assert_eq!(clean_findings.undeclared_component_dependencies.len(), 0);
        assert_eq!(fresh_findings.undeclared_component_dependencies.len(), 1);
    }
}
