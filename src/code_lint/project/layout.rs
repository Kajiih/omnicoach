//! Project boundary, source-root layout, and file-system module mapping (`ProjectLayout`).

use super::config::{CONFIG_FILE_NAME, is_rooted_path, lexical_normalize, resolve_against_cwd};
use super::paths::{
    INIT_PY, INIT_PYI, INIT_STEM, LIB_CRATE_ROOT, MAIN_CRATE_ROOT, ROOT_MODULE_TAG,
    is_python_package_init, normalize_relative_path, python_dotted_path, rust_module_from_relative,
};
use super::source::FileSource;
use serde::Deserialize;
use std::path::{Path, PathBuf};

const CARGO_TOML: &str = "Cargo.toml";
const PYPROJECT_TOML: &str = "pyproject.toml";
const PYTHON_EXTENSION: &str = "py";
const PYTHON_STUB_EXTENSION: &str = "pyi";
const PYTHON_SUFFIX: &str = ".py";
const PYTHON_STUB_SUFFIX: &str = ".pyi";
const SOURCE_DIRECTORY: &str = "src";

/// Project boundary and source roots resolved once at the CLI, hook, or test boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectLayout {
    /// Nearest directory containing `.omnicoach.toml`, `pyproject.toml`, or `Cargo.toml`
    /// (falling back to the starting directory).
    pub project_root: PathBuf,
    /// Ordered Python import roots (longest-prefix match maps files to dotted module names).
    pub python_source_roots: Vec<PathBuf>,
    /// Rust crate source root (`<project_root>/src` when `Cargo.toml` exists, else `project_root`).
    pub rust_source_root: PathBuf,
    /// Normalized Rust crate identifier (`[lib] name` or `[package] name` with `-` replaced by `_`),
    /// when `Cargo.toml` is present at `project_root`.
    pub rust_crate_name: Option<String>,
    /// Base directory used to resolve relative target paths passed by the CLI.
    working_directory: Option<PathBuf>,
}

impl ProjectLayout {
    /// Discovers project layout upward from `anchor`, honoring optional configured Python roots
    /// (relative roots are resolved against the discovered `project_root`).
    #[must_use]
    pub fn discover(
        anchor: &Path,
        source: &dyn FileSource,
        configured_python_roots: &[PathBuf],
    ) -> Self {
        let is_relative_anchor = !is_rooted_path(anchor);
        let working_directory = is_relative_anchor.then(|| source.current_dir()).flatten();
        let resolved_anchor = resolve_against_cwd(anchor, working_directory.as_deref());
        let start_directory = if source.is_dir(&resolved_anchor) {
            resolved_anchor
        } else {
            resolved_anchor
                .parent()
                .map_or_else(|| PathBuf::from("."), Path::to_path_buf)
        };
        let project_root =
            find_project_root(&start_directory, source).unwrap_or_else(|| start_directory.clone());
        let python_source_roots = if configured_python_roots.is_empty() {
            infer_python_source_roots(&project_root, &start_directory, source)
        } else {
            configured_python_roots
                .iter()
                .map(|root| lexical_normalize(&project_root.join(root)))
                .collect()
        };
        let cargo_source = project_root.join(SOURCE_DIRECTORY);
        let rust_source_root = if source.is_dir(&cargo_source) {
            cargo_source
        } else {
            project_root.clone()
        };
        let rust_crate_name = parse_cargo_crate_name(&project_root, source);
        Self {
            project_root,
            python_source_roots,
            rust_source_root,
            rust_crate_name,
            working_directory,
        }
    }

    /// Replaces [`Self::python_source_roots`] with `configured_python_roots` (resolved against
    /// [`Self::project_root`]) when `configured_python_roots` is non-empty.
    #[must_use]
    pub(super) fn with_configured_python_roots(
        mut self,
        configured_python_roots: &[PathBuf],
    ) -> Self {
        if !configured_python_roots.is_empty() {
            self.python_source_roots = configured_python_roots
                .iter()
                .map(|root| lexical_normalize(&self.project_root.join(root)))
                .collect();
        }
        self
    }

    /// Maps a Python file path to its canonical dotted module path under the longest matching
    /// source root (for example `.../python/tach/check.py` -> `"tach.check"`).
    #[must_use]
    pub(super) fn python_module_of_path(&self, path: &Path) -> Option<String> {
        python_module_from_resolved(
            &self.resolve_query_path(path),
            &self.python_source_roots,
            false,
        )
    }

    /// Maps a Python file path to its canonical source-root module path, or to its
    /// project-relative fallback module path when `path` is inside [`Self::project_root`] outside
    /// [`Self::python_source_roots`] (such as `tests/*.py` or `scripts/*.py` in a `src/`-layout
    /// repository).
    #[must_use]
    pub(super) fn python_context_module_of_path(&self, path: &Path) -> Option<String> {
        if let Some(module) = self.python_module_of_path(path) {
            return Some(module);
        }
        let resolved = self.resolve_query_path(path);
        if !has_python_extension(&resolved) {
            return None;
        }
        let relative = resolved.strip_prefix(&self.project_root).ok()?;
        let normalized = normalize_relative_path(relative);
        (!normalized.is_empty()).then(|| {
            self.fallback_python_module_path(
                &resolved,
                &normalized,
                is_python_package_init(&resolved),
            )
        })
    }

    /// Maps a Python file path to its canonical dotted module path, returning `"<root>"` when
    /// `path` is `<source_root>/__init__.py`.
    #[must_use]
    pub(super) fn python_architecture_module_of_path(&self, path: &Path) -> Option<String> {
        python_module_from_resolved(
            &self.resolve_query_path(path),
            &self.python_source_roots,
            true,
        )
    }

    /// Computes a fallback dotted Python module identifier when `full_path` lies outside all
    /// configured Python source roots (or is a source-root `__init__.py`).
    #[must_use]
    pub(super) fn fallback_python_module_path(
        &self,
        full_path: &Path,
        relative_path: &str,
        is_package_init: bool,
    ) -> String {
        if is_package_init
            && full_path
                .parent()
                .is_some_and(|parent| self.python_source_roots.iter().any(|root| root == parent))
        {
            return INIT_STEM.to_owned();
        }
        python_dotted_path(relative_path, true)
    }

    /// Resolves a dotted Python module name (`"pkg.sub"`) to a `.pyi` or `.py` file under
    /// configured source roots using PEP 561 stub-first package precedence:
    /// 1. `<root>/<module>/__init__.pyi`
    /// 2. `<root>/<module>/__init__.py`
    /// 3. `<root>/<module>.pyi`
    /// 4. `<root>/<module>.py`
    #[must_use]
    pub(super) fn python_module_file(
        &self,
        module: &str,
        source: &dyn FileSource,
    ) -> Option<PathBuf> {
        if module.is_empty() {
            return None;
        }
        if module == ROOT_MODULE_TAG {
            for root in &self.python_source_roots {
                for init_name in [INIT_PYI, INIT_PY] {
                    let candidate = root.join(init_name);
                    if source.is_file(&candidate) {
                        return Some(candidate);
                    }
                }
            }
            return None;
        }
        let relative_directory = module.replace('.', "/");
        for root in &self.python_source_roots {
            let package_dir = root.join(&relative_directory);
            let init_stub = package_dir.join(INIT_PYI);
            if source.is_file(&init_stub) {
                return Some(init_stub);
            }
            let init_py = package_dir.join(INIT_PY);
            if source.is_file(&init_py) {
                return Some(init_py);
            }
            let file_stub = root.join(format!("{relative_directory}.pyi"));
            if source.is_file(&file_stub) {
                return Some(file_stub);
            }
            let file_py = root.join(format!("{relative_directory}.py"));
            if source.is_file(&file_py) {
                return Some(file_py);
            }
        }
        None
    }

    /// Returns true if `module` corresponds to an existing directory under any Python source root
    /// (covering PEP 420 namespace packages without `__init__.py`).
    #[must_use]
    pub(super) fn is_python_namespace_dir(&self, module: &str, source: &dyn FileSource) -> bool {
        if module.is_empty() || module == ROOT_MODULE_TAG {
            return false;
        }
        let relative_directory = module.replace('.', "/");
        self.python_source_roots
            .iter()
            .any(|root| source.is_dir(&root.join(&relative_directory)))
    }

    /// Maps a Rust `.rs` file path to its crate-relative module path (`"lib"`, `"code_lint::ast"`).
    #[must_use]
    pub(super) fn rust_module_of_path(&self, path: &Path) -> Option<String> {
        let resolved = self.resolve_query_path(path);
        let relative = resolved.strip_prefix(&self.rust_source_root).ok()?;
        rust_module_from_relative(relative)
    }

    /// Resolves a crate-relative Rust module path (`"lib"`, `"code_lint::ast"`) to its `.rs` file.
    #[must_use]
    pub(super) fn rust_module_file(
        &self,
        module: &str,
        source: &dyn FileSource,
    ) -> Option<PathBuf> {
        if module == LIB_CRATE_ROOT || module == MAIN_CRATE_ROOT {
            let candidate = self.rust_source_root.join(format!("{module}.rs"));
            return source.is_file(&candidate).then_some(candidate);
        }
        let relative = module.replace("::", "/");
        let modern = self.rust_source_root.join(format!("{relative}.rs"));
        if source.is_file(&modern) {
            return Some(modern);
        }
        let legacy = self.rust_source_root.join(&relative).join("mod.rs");
        if source.is_file(&legacy) {
            return Some(legacy);
        }
        None
    }

    /// Normalizes `path` relative to `working_directory` (or `project_root` when `anchor` was
    /// absolute) when `path` is relative.
    #[must_use]
    pub(super) fn resolve_query_path(&self, path: &Path) -> PathBuf {
        let base = self
            .working_directory
            .as_deref()
            .unwrap_or(&self.project_root);
        resolve_against_cwd(path, Some(base))
    }

    /// Infers an unconfigured Python source root for `directory` inside [`Self::project_root`]
    /// when `directory` is a nested Python package (`__init__.py` / `__init__.pyi`) or carries a
    /// subpackage `pyproject.toml`, stopping at any nested `.omnicoach.toml` or VCS boundary.
    #[must_use]
    pub(super) fn infer_package_source_root(
        &self,
        directory: &Path,
        source: &dyn FileSource,
    ) -> Option<PathBuf> {
        if !self.python_source_roots.contains(&self.project_root)
            || !directory.starts_with(&self.project_root)
            || directory == self.project_root
        {
            return None;
        }
        let mut ancestor = directory;
        while ancestor != self.project_root {
            if source.is_file(&ancestor.join(CONFIG_FILE_NAME)) || is_vcs_boundary(ancestor, source)
            {
                return None;
            }
            ancestor = ancestor.parent()?;
        }
        if is_python_package_dir(directory, source) {
            return infer_enclosing_package_source_root(&self.project_root, directory, source);
        }
        source
            .is_file(&directory.join(PYPROJECT_TOML))
            .then(|| directory.to_path_buf())
    }

    /// Merges additional discovered Python source roots into this layout in specificity order
    /// (longest path first) without duplicates.
    pub(super) fn merge_python_source_roots(&mut self, additional: &[PathBuf]) {
        let mut changed = false;
        for candidate in additional {
            if !self.python_source_roots.contains(candidate) {
                self.python_source_roots.push(candidate.clone());
                changed = true;
            }
        }
        if changed {
            self.python_source_roots.sort_by(|left, right| {
                right
                    .components()
                    .count()
                    .cmp(&left.components().count())
                    .then_with(|| left.cmp(right))
            });
        }
    }
}

/// Returns true if `directory` contains a `.jj` or `.git` repository/worktree boundary.
#[must_use]
pub(super) fn is_vcs_boundary(directory: &Path, source: &dyn FileSource) -> bool {
    let git_marker = directory.join(".git");
    source.is_dir(&directory.join(".jj"))
        || source.is_dir(&git_marker)
        || source.is_file(&git_marker)
}

fn parse_cargo_crate_name(project_root: &Path, source: &dyn FileSource) -> Option<String> {
    let content = source.read_to_string(&project_root.join(CARGO_TOML)).ok()?;
    let manifest: CargoManifest = toml::from_str(&content).ok()?;
    let raw_name = manifest
        .lib
        .and_then(|lib| lib.name)
        .or_else(|| manifest.package.and_then(|package| package.name))?;
    let normalized = raw_name.trim().replace('-', "_");
    (!normalized.is_empty()).then_some(normalized)
}

#[derive(Deserialize, Default)]
struct CargoManifest {
    package: Option<CargoPackage>,
    lib: Option<CargoLibTarget>,
}

#[derive(Deserialize)]
struct CargoPackage {
    name: Option<String>,
}

#[derive(Deserialize)]
struct CargoLibTarget {
    name: Option<String>,
}

fn find_project_root(start_directory: &Path, source: &dyn FileSource) -> Option<PathBuf> {
    let markers = [CONFIG_FILE_NAME, PYPROJECT_TOML, CARGO_TOML];
    let mut current = Some(start_directory);
    while let Some(directory) = current {
        if markers
            .iter()
            .any(|marker| source.is_file(&directory.join(marker)))
            || is_vcs_boundary(directory, source)
        {
            return Some(directory.to_path_buf());
        }
        current = directory.parent();
    }
    if source.is_file(&start_directory.join(INIT_PY))
        || source.is_file(&start_directory.join(INIT_PYI))
    {
        let mut cursor = start_directory;
        while source.is_file(&cursor.join(INIT_PY)) || source.is_file(&cursor.join(INIT_PYI)) {
            let Some(parent) = cursor
                .parent()
                .filter(|parent| parent.parent().is_some() && source.is_dir(parent))
            else {
                break;
            };
            cursor = parent;
        }
        return Some(cursor.to_path_buf());
    }
    None
}

fn infer_python_source_roots(
    project_root: &Path,
    start_directory: &Path,
    source: &dyn FileSource,
) -> Vec<PathBuf> {
    let has_cargo_toml = source.is_file(&project_root.join(CARGO_TOML));
    let has_pyproject_toml = source.is_file(&project_root.join(PYPROJECT_TOML));
    let mut container_roots = Vec::new();
    for candidate_name in [SOURCE_DIRECTORY, "python", "Lib"] {
        let candidate = project_root.join(candidate_name);
        let allow_candidate =
            candidate_name != SOURCE_DIRECTORY || !has_cargo_toml || has_pyproject_toml;
        if allow_candidate
            && is_non_package_directory(&candidate, source)
            && directory_contains_python_source(&candidate, source)
        {
            container_roots.push(candidate);
        }
    }
    if !container_roots.is_empty() {
        return container_roots;
    }

    let mut roots = Vec::new();
    if let Some(inferred) =
        infer_enclosing_package_source_root(project_root, start_directory, source)
    {
        roots.push(inferred);
    }
    if !roots.contains(&project_root.to_path_buf()) {
        roots.push(project_root.to_path_buf());
    }
    roots
}

fn is_non_package_directory(directory: &Path, source: &dyn FileSource) -> bool {
    source.is_dir(directory) && !is_python_package_dir(directory, source)
}

fn directory_contains_python_source(directory: &Path, source: &dyn FileSource) -> bool {
    source
        .read_dir(directory)
        .unwrap_or_default()
        .into_iter()
        .any(|entry| {
            has_python_extension(&entry)
                || (source.is_dir(&entry)
                    && source
                        .read_dir(&entry)
                        .unwrap_or_default()
                        .into_iter()
                        .any(|nested| has_python_extension(&nested)))
        })
}

fn has_python_extension(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|extension| extension.to_str()),
        Some(PYTHON_EXTENSION | PYTHON_STUB_EXTENSION)
    )
}

fn python_module_from_resolved(
    resolved: &Path,
    python_source_roots: &[PathBuf],
    allow_root_tag: bool,
) -> Option<String> {
    let best_root = python_source_roots
        .iter()
        .filter(|root| resolved.starts_with(root.as_path()))
        .max_by_key(|root| root.components().count())?;
    let relative = resolved.strip_prefix(best_root).ok()?;
    let extension = relative.extension()?.to_str()?;
    if extension != PYTHON_EXTENSION && extension != PYTHON_STUB_EXTENSION {
        return None;
    }
    let mut segments: Vec<String> = relative
        .components()
        .filter_map(|component| component.as_os_str().to_str().map(str::to_owned))
        .collect();
    let last = segments.pop()?;
    let stem = last
        .strip_suffix(PYTHON_SUFFIX)
        .or_else(|| last.strip_suffix(PYTHON_STUB_SUFFIX))?;
    if stem != INIT_STEM {
        segments.push(stem.to_owned());
    } else if segments.is_empty() && allow_root_tag {
        return Some(ROOT_MODULE_TAG.to_owned());
    }
    (!segments.is_empty()).then(|| segments.join("."))
}

fn infer_enclosing_package_source_root(
    project_root: &Path,
    start_directory: &Path,
    source: &dyn FileSource,
) -> Option<PathBuf> {
    if !start_directory.starts_with(project_root) {
        return None;
    }
    let mut cursor = start_directory;
    while cursor != project_root && is_python_package_dir(cursor, source) {
        cursor = cursor.parent()?;
    }
    (cursor != project_root && !is_python_package_dir(cursor, source)).then(|| cursor.to_path_buf())
}

fn is_python_package_dir(directory: &Path, source: &dyn FileSource) -> bool {
    source.is_file(&directory.join(INIT_PY)) || source.is_file(&directory.join(INIT_PYI))
}

#[cfg(test)]
mod tests {
    use super::super::paths::{resolve_rust_reference_path, rust_module_path};
    use super::*;
    use crate::code_lint::project::source::MemoryFileSource;

    #[test]
    fn python_module_file_prefers_stub_and_package_init_over_module_file() {
        let root = Path::new("/repo");
        let source = MemoryFileSource::from_relative_files(
            root,
            &[
                (".omnicoach.toml", ""),
                ("pkg/__init__.pyi", ""),
                ("pkg/__init__.py", ""),
                ("pkg/mod.pyi", ""),
                ("pkg/mod.py", ""),
            ],
        );
        let layout = ProjectLayout::discover(root, &source, &[]);
        assert_eq!(
            layout.python_module_file("pkg", &source),
            Some(root.join("pkg/__init__.pyi"))
        );
        assert_eq!(
            layout.python_module_file("pkg.mod", &source),
            Some(root.join("pkg/mod.pyi"))
        );
    }

    #[test]
    fn discover_extracts_rust_crate_name_and_normalizes_self_crate_paths() {
        let root = Path::new("/repo");
        let source = MemoryFileSource::from_relative_files(
            root,
            &[
                (
                    "Cargo.toml",
                    indoc::indoc! {r#"
                        [package]
                        name = "my-custom-crate"
                        version = "0.1.0"
                    "#},
                ),
                ("src/lib.rs", ""),
            ],
        );
        let layout = ProjectLayout::discover(root, &source, &[]);
        let self_resolved = resolve_rust_reference_path(
            "runner",
            &[],
            "my_custom_crate::core::Config",
            layout.rust_crate_name.as_deref(),
        );
        let external_resolved = resolve_rust_reference_path(
            "runner",
            &[],
            "omnicoach::core::Config",
            layout.rust_crate_name.as_deref(),
        );

        assert_eq!(layout.rust_crate_name.as_deref(), Some("my_custom_crate"));
        assert_eq!(self_resolved.canonical_path, "core::Config");
        assert_eq!(external_resolved.canonical_path, "omnicoach::core::Config");
    }

    #[test]
    fn configured_source_roots_resolve_against_the_configuration_root_from_a_sub_package() {
        let root = Path::new("/repo");
        let source = MemoryFileSource::from_relative_files(
            root,
            &[
                (".omnicoach.toml", ""),
                ("packages/app/pyproject.toml", ""),
                ("packages/app/src/app/__init__.py", ""),
                ("packages/app/src/app/main.py", ""),
            ],
        );
        let config = super::super::config::ProjectConfig {
            context: toml::from_str(r#"source-roots = ["packages/app/src"]"#).unwrap(),
            root_dir: Some(root.to_path_buf()),
            ..super::super::config::ProjectConfig::default()
        };
        let layout = ProjectLayout::discover(
            &root.join("packages/app/src/app/main.py"),
            &source,
            &config.configured_source_roots(),
        );

        assert_eq!(layout.project_root, root.join("packages/app"));
        assert_eq!(
            layout.python_module_of_path(&root.join("packages/app/src/app/main.py")),
            Some("app.main".to_owned())
        );
    }

    #[test]
    fn discover_resolves_relative_anchor_against_file_source_current_dir() {
        let root = Path::new("/virtual_workspace");
        let source = MemoryFileSource::from_relative_files(
            root,
            &[
                ("Cargo.toml", "[package]\nname = \"demo\"\n"),
                ("src/lib.rs", ""),
            ],
        );
        let layout = ProjectLayout::discover(Path::new("src/lib.rs"), &source, &[]);

        assert_eq!(layout.project_root, lexical_normalize(root));
        assert_eq!(
            layout.rust_module_of_path(Path::new("src/lib.rs")),
            Some("lib".to_owned())
        );
        assert_eq!(rust_module_path(r"src\sub\mod.rs"), "sub");
    }

    #[test]
    fn discover_stops_at_vcs_boundary_and_infers_maturin_python_roots() {
        let outer = Path::new("/outer");
        let worktree_source = MemoryFileSource::from_relative_files(
            outer,
            &[
                ("Cargo.toml", "[package]\nname = \"outer\"\n"),
                ("worktree/.jj/repo", ""),
                ("worktree/pkg/__init__.py", ""),
                ("worktree/pkg/mod.py", ""),
            ],
        );
        let maturin_root = Path::new("/maturin");
        let maturin_source = MemoryFileSource::from_relative_files(
            maturin_root,
            &[
                ("Cargo.toml", "[package]\nname = \"ext\"\n"),
                ("pyproject.toml", "[project]\nname = \"ext\"\n"),
                ("src/lib.rs", ""),
                ("src/src_pkg/__init__.py", ""),
                ("src/src_pkg/core.py", ""),
                ("python/py_pkg/__init__.py", ""),
                ("python/py_pkg/api.py", ""),
            ],
        );

        let worktree_layout =
            ProjectLayout::discover(&outer.join("worktree/pkg/mod.py"), &worktree_source, &[]);
        let maturin_layout = ProjectLayout::discover(maturin_root, &maturin_source, &[]);

        assert_eq!(worktree_layout.project_root, outer.join("worktree"));
        assert_eq!(
            maturin_layout.python_module_of_path(&maturin_root.join("src/src_pkg/core.py")),
            Some("src_pkg.core".to_owned())
        );
        assert_eq!(
            maturin_layout.python_module_of_path(&maturin_root.join("python/py_pkg/api.py")),
            Some("py_pkg.api".to_owned())
        );
    }

    #[test]
    fn infer_python_source_roots_prefers_container_root_over_sibling_dev_packages() {
        let root = Path::new("/repo");
        let source = MemoryFileSource::from_relative_files(
            root,
            &[
                ("pyproject.toml", "[project]\nname = \"polybot\"\n"),
                ("src/polybot/__init__.py", ""),
                ("src/polybot/core.py", ""),
                ("dev/scripts/__init__.py", ""),
                ("dev/scripts/check.py", ""),
            ],
        );
        let from_dev = ProjectLayout::discover(&root.join("dev/scripts/check.py"), &source, &[]);

        assert_eq!(from_dev.python_source_roots, vec![root.join("src")]);
        assert_eq!(
            from_dev.python_module_of_path(&root.join("dev/scripts/check.py")),
            None
        );
        assert_eq!(
            from_dev.infer_package_source_root(&root.join("dev/scripts"), &source),
            None
        );
    }
}
