//! Python source-root discovery and file-to-module mapping (`CodeLintProjectPython`).

use super::paths::{INIT_PY, INIT_PYI, INIT_STEM, is_python_package_init, python_dotted_path};
use crate::code_lint::project::foundation::{
    CONFIG_FILE_NAME, FileSource, ProjectSourceFile, ROOT_MODULE_TAG, WorkspaceAnchor,
    is_vcs_boundary, lexical_normalize, normalize_relative_path,
};
use crate::diagnostic::Language;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

pub(in crate::code_lint::project) const PYPROJECT_TOML: &str = "pyproject.toml";
const PYTHON_EXTENSION: &str = "py";
const PYTHON_STUB_EXTENSION: &str = "pyi";
const PYTHON_SUFFIX: &str = ".py";
const PYTHON_STUB_SUFFIX: &str = ".pyi";

/// Python source roots and file-to-module mapping for a discovered project.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::code_lint::project) struct PythonLayout {
    anchor: WorkspaceAnchor,
    /// Ordered Python import roots (longest-prefix match maps files to dotted module names).
    pub(in crate::code_lint::project) source_roots: Vec<PathBuf>,
}

impl PythonLayout {
    /// Discovers or resolves Python source roots within `anchor.project_root`.
    #[must_use]
    pub(in crate::code_lint::project) fn discover(
        anchor: WorkspaceAnchor,
        start_directory: &Path,
        claimed_source_dir: Option<&Path>,
        source: &dyn FileSource,
        configured_roots: &[PathBuf],
    ) -> Self {
        let project_root = &anchor.project_root;
        let source_roots = if configured_roots.is_empty() {
            infer_python_source_roots(project_root, start_directory, claimed_source_dir, source)
        } else {
            configured_roots
                .iter()
                .map(|root| lexical_normalize(&project_root.join(root)))
                .collect()
        };
        Self {
            anchor,
            source_roots,
        }
    }

    /// Replaces [`Self::source_roots`] with `configured_roots` (resolved against `project_root`)
    /// when `configured_roots` is non-empty.
    #[must_use]
    pub(in crate::code_lint::project) fn with_configured_roots(
        mut self,
        configured_roots: &[PathBuf],
    ) -> Self {
        if !configured_roots.is_empty() {
            self.source_roots = configured_roots
                .iter()
                .map(|root| lexical_normalize(&self.anchor.project_root.join(root)))
                .collect();
        }
        self
    }

    /// Maps a Python file path to its canonical dotted module path under the longest matching
    /// source root (for example `.../python/tach/check.py` -> `"tach.check"`).
    #[must_use]
    pub(in crate::code_lint::project) fn module_of_path(&self, path: &Path) -> Option<String> {
        python_module_from_resolved(&self.resolve_query_path(path), &self.source_roots, false)
    }

    /// Maps a Python file path to its canonical source-root module path, or to its
    /// project-relative fallback module path when `path` is inside `project_root` outside
    /// [`Self::source_roots`] (such as `tests/*.py` or `scripts/*.py` in a `src/`-layout
    /// repository).
    #[must_use]
    pub(in crate::code_lint::project) fn context_module_of_path(
        &self,
        path: &Path,
    ) -> Option<String> {
        if let Some(module) = self.module_of_path(path) {
            return Some(module);
        }
        let resolved = self.resolve_query_path(path);
        if !has_python_extension(&resolved) {
            return None;
        }
        let relative = resolved.strip_prefix(&self.anchor.project_root).ok()?;
        let normalized = normalize_relative_path(relative);
        (!normalized.is_empty()).then(|| {
            self.fallback_module_path(&resolved, &normalized, is_python_package_init(&resolved))
        })
    }

    /// Maps a Python file path to its canonical dotted module path, returning `"<root>"` when
    /// `path` is `<source_root>/__init__.py`.
    #[must_use]
    pub(in crate::code_lint::project) fn architecture_module_of_path(
        &self,
        path: &Path,
    ) -> Option<String> {
        python_module_from_resolved(&self.resolve_query_path(path), &self.source_roots, true)
    }

    /// Computes a fallback dotted Python module identifier when `full_path` lies outside all
    /// configured Python source roots (or is a source-root `__init__.py`).
    #[must_use]
    pub(in crate::code_lint::project) fn fallback_module_path(
        &self,
        full_path: &Path,
        relative_path: &str,
        is_package_init: bool,
    ) -> String {
        if is_package_init
            && full_path
                .parent()
                .is_some_and(|parent| self.source_roots.iter().any(|root| root == parent))
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
    pub(in crate::code_lint::project) fn module_file(
        &self,
        module: &str,
        source: &dyn FileSource,
    ) -> Option<PathBuf> {
        if module.is_empty() {
            return None;
        }
        if module == ROOT_MODULE_TAG {
            for root in &self.source_roots {
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
        for root in &self.source_roots {
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
    pub(in crate::code_lint::project) fn is_namespace_dir(
        &self,
        module: &str,
        source: &dyn FileSource,
    ) -> bool {
        if module.is_empty() || module == ROOT_MODULE_TAG {
            return false;
        }
        let relative_directory = module.replace('.', "/");
        self.source_roots
            .iter()
            .any(|root| source.is_dir(&root.join(&relative_directory)))
    }

    /// Merges additional discovered Python source roots into this layout in specificity order
    /// (longest path first) without duplicates.
    pub(in crate::code_lint::project) fn merge_source_roots(&mut self, additional: &[PathBuf]) {
        if !self.source_roots.contains(&self.anchor.project_root) {
            return;
        }
        let mut changed = false;
        for candidate in additional {
            if !self.source_roots.contains(candidate) {
                self.source_roots.push(candidate.clone());
                changed = true;
            }
        }
        if changed {
            self.source_roots.sort_by(|left, right| {
                right
                    .components()
                    .count()
                    .cmp(&left.components().count())
                    .then_with(|| left.cmp(right))
            });
        }
    }

    /// Scans `files` for nested Python subpackages under `project_root` and merges their inferred
    /// source roots when `project_root` is currently in `source_roots`.
    pub(in crate::code_lint::project) fn merge_inferred_subpackage_roots(
        &mut self,
        files: &[ProjectSourceFile],
        source: &dyn FileSource,
    ) {
        if !self.source_roots.contains(&self.anchor.project_root) {
            return;
        }
        let mut visited_directories = HashSet::new();
        let mut inferred_roots = Vec::new();
        for file in files {
            if file.lang != Language::Python {
                continue;
            }
            let mut cursor = file.full_path.parent();
            while let Some(directory) = cursor {
                if !directory.starts_with(&self.anchor.project_root)
                    || directory == self.anchor.project_root
                    || !visited_directories.insert(directory.to_path_buf())
                {
                    break;
                }
                if let Some(root) = self.infer_package_source_root(directory, source) {
                    inferred_roots.push(root);
                }
                cursor = directory.parent();
            }
        }
        if !inferred_roots.is_empty() {
            self.merge_source_roots(&inferred_roots);
        }
    }

    /// Returns true if `resolved_dirs` covers every configured or inferred Python source root.
    #[must_use]
    pub(in crate::code_lint::project) fn covers_source_roots(
        &self,
        resolved_dirs: &[PathBuf],
    ) -> bool {
        !self.source_roots.is_empty()
            && self.source_roots.iter().all(|python_root| {
                resolved_dirs
                    .iter()
                    .any(|resolved_dir| python_root.starts_with(resolved_dir))
            })
    }

    #[must_use]
    pub(in crate::code_lint::project) fn resolve_query_path(&self, path: &Path) -> PathBuf {
        self.anchor.resolve_query_path(path)
    }

    fn infer_package_source_root(
        &self,
        directory: &Path,
        source: &dyn FileSource,
    ) -> Option<PathBuf> {
        let project_root = &self.anchor.project_root;
        if !self.source_roots.contains(project_root)
            || !directory.starts_with(project_root)
            || directory == project_root
        {
            return None;
        }
        let mut ancestor = directory;
        while ancestor != project_root {
            if source.is_file(&ancestor.join(CONFIG_FILE_NAME)) || is_vcs_boundary(ancestor, source)
            {
                return None;
            }
            ancestor = ancestor.parent()?;
        }
        if is_python_package_dir(directory, source) {
            return infer_enclosing_package_source_root(project_root, directory, source);
        }
        source
            .is_file(&directory.join(PYPROJECT_TOML))
            .then(|| directory.to_path_buf())
    }
}

/// Climbs above a chain of Python package directories (`__init__.py` / `__init__.pyi`) to the
/// enclosing source root when no explicit project marker file exists.
#[must_use]
pub(in crate::code_lint::project) fn climb_package_init_chain(
    start_directory: &Path,
    source: &dyn FileSource,
) -> Option<PathBuf> {
    if !is_python_package_dir(start_directory, source) {
        return None;
    }
    let mut cursor = start_directory;
    while is_python_package_dir(cursor, source) {
        let Some(parent) = cursor
            .parent()
            .filter(|parent| parent.parent().is_some() && source.is_dir(parent))
        else {
            break;
        };
        cursor = parent;
    }
    Some(cursor.to_path_buf())
}

fn infer_python_source_roots(
    project_root: &Path,
    start_directory: &Path,
    claimed_source_dir: Option<&Path>,
    source: &dyn FileSource,
) -> Vec<PathBuf> {
    let has_pyproject_toml = source.is_file(&project_root.join(PYPROJECT_TOML));
    let mut container_roots = Vec::new();
    for candidate_name in ["src", "python", "Lib"] {
        let candidate = project_root.join(candidate_name);
        let allow_candidate = claimed_source_dir != Some(candidate.as_path()) || has_pyproject_toml;
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

fn has_python_extension(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|extension| extension.to_str()),
        Some(PYTHON_EXTENSION | PYTHON_STUB_EXTENSION)
    )
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
    use super::*;
    use crate::code_lint::project::foundation::MemoryFileSource;

    #[test]
    fn module_file_prefers_stub_and_package_init_over_module_file() {
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
        let layout = PythonLayout::discover(
            WorkspaceAnchor::new(root.to_path_buf(), None),
            root,
            None,
            &source,
            &[],
        );

        assert_eq!(
            layout.module_file("pkg", &source),
            Some(root.join("pkg/__init__.pyi"))
        );
        assert_eq!(
            layout.module_file("pkg.mod", &source),
            Some(root.join("pkg/mod.pyi"))
        );
    }

    #[test]
    fn infer_source_roots_prefers_container_root_over_sibling_dev_packages() {
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
        let from_dev = PythonLayout::discover(
            WorkspaceAnchor::new(root.to_path_buf(), None),
            &root.join("dev/scripts"),
            None,
            &source,
            &[],
        );

        assert_eq!(from_dev.source_roots, vec![root.join("src")]);
        assert_eq!(
            from_dev.module_of_path(&root.join("dev/scripts/check.py")),
            None
        );
        assert_eq!(
            from_dev.infer_package_source_root(&root.join("dev/scripts"), &source),
            None
        );
    }
}
