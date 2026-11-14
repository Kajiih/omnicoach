//! Project boundary, source-root layout, and file-system module mapping (`CodeLintProjectContext`).

use crate::code_lint::project::foundation::{
    CONFIG_FILE_NAME, FileSource, WorkspaceAnchor, is_rooted_path, is_vcs_boundary,
    resolve_against_cwd,
};
use crate::code_lint::project::python::{PythonLayout, climb_package_init_chain};
use crate::code_lint::project::rust::RustLayout;
use crate::diagnostic::Language;
use std::path::{Path, PathBuf};

const CARGO_TOML: &str = "Cargo.toml";
const PYPROJECT_TOML: &str = "pyproject.toml";

/// Project boundary and per-language source layouts resolved once at the CLI, hook, or test boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectLayout {
    /// Nearest directory containing `.omnicoach.toml`, `pyproject.toml`, or `Cargo.toml`
    /// (falling back to the starting directory).
    pub(in crate::code_lint) project_root: PathBuf,
    /// Python source-root layout and module-path resolver.
    pub(super) python: PythonLayout,
    /// Rust crate source-root layout and module-path resolver.
    pub(super) rust: RustLayout,
}

impl ProjectLayout {
    /// Discovers project layout upward from `anchor`, honoring optional configured source roots
    /// (relative roots are resolved against the discovered `project_root`).
    #[must_use]
    pub fn discover(anchor: &Path, source: &dyn FileSource, configured_roots: &[PathBuf]) -> Self {
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
        let workspace_anchor = WorkspaceAnchor::new(project_root.clone(), working_directory);
        let rust = RustLayout::discover(workspace_anchor.clone(), source);
        let python = PythonLayout::discover(
            workspace_anchor,
            &start_directory,
            rust.claims_source_dir(),
            source,
            configured_roots,
        );
        Self {
            project_root,
            python,
            rust,
        }
    }

    /// Replaces configured source roots on this layout (resolved against [`Self::project_root`])
    /// when `configured_roots` is non-empty.
    #[must_use]
    pub(super) fn with_configured_roots(mut self, configured_roots: &[PathBuf]) -> Self {
        self.python = self.python.with_configured_roots(configured_roots);
        self
    }

    /// Normalizes `path` relative to the captured caller working directory (or
    /// [`Self::project_root`] when `anchor` was absolute) when `path` is relative.
    #[must_use]
    pub(super) fn resolve_query_path(&self, path: &Path) -> PathBuf {
        self.python.resolve_query_path(path)
    }

    /// Returns true when `resolved_dirs` covers [`Self::project_root`], the Rust crate source root,
    /// or all Python source roots.
    #[must_use]
    pub(super) fn covers_source_roots(&self, resolved_dirs: &[PathBuf]) -> bool {
        let covers_project_or_crate_root = resolved_dirs.iter().any(|resolved_dir| {
            *resolved_dir == self.project_root || *resolved_dir == self.rust.source_root
        });
        covers_project_or_crate_root || self.python.covers_source_roots(resolved_dirs)
    }

    /// Returns true when a walked directory is [`Self::project_root`] and `full_path` lies outside
    /// the language's primary source tree (such as `tests/*.rs` or `benches/*.rs` outside `src/`
    /// in a `Cargo.toml` project).
    #[must_use]
    pub(super) fn is_outside_primary_source_root(
        &self,
        resolved_walk_dir: &Path,
        full_path: &Path,
        lang: Language,
    ) -> bool {
        lang == Language::Rust
            && self
                .rust
                .is_outside_source_root(resolved_walk_dir, full_path)
    }
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
    climb_package_init_chain(start_directory, source)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::code_lint::project::foundation::{
        MemoryFileSource, ProjectConfig, lexical_normalize,
    };

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
        let config = ProjectConfig {
            context: toml::from_str(r#"source-roots = ["packages/app/src"]"#).unwrap(),
            root_dir: Some(root.to_path_buf()),
            ..ProjectConfig::default()
        };
        let layout = ProjectLayout::discover(
            &root.join("packages/app/src/app/main.py"),
            &source,
            &config.configured_source_roots(),
        );

        assert_eq!(layout.project_root, root.join("packages/app"));
        assert_eq!(
            layout
                .python
                .module_of_path(&root.join("packages/app/src/app/main.py")),
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
            layout.rust.module_of_path(Path::new("src/lib.rs")),
            Some("lib".to_owned())
        );
    }

    #[test]
    fn discover_stops_at_vcs_boundary_and_infers_maturin_source_roots() {
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
        let cargo_only_root = Path::new("/cargo_only");
        let cargo_only_source = MemoryFileSource::from_relative_files(
            cargo_only_root,
            &[
                ("Cargo.toml", "[package]\nname = \"app\"\n"),
                ("src/lib.rs", ""),
                ("src/helper.py", ""),
            ],
        );

        let worktree_layout =
            ProjectLayout::discover(&outer.join("worktree/pkg/mod.py"), &worktree_source, &[]);
        let maturin_layout = ProjectLayout::discover(maturin_root, &maturin_source, &[]);
        let cargo_only_layout = ProjectLayout::discover(cargo_only_root, &cargo_only_source, &[]);

        assert_eq!(worktree_layout.project_root, outer.join("worktree"));
        assert_eq!(
            maturin_layout
                .python
                .module_of_path(&maturin_root.join("src/src_pkg/core.py")),
            Some("src_pkg.core".to_owned())
        );
        assert_eq!(
            maturin_layout
                .python
                .module_of_path(&maturin_root.join("python/py_pkg/api.py")),
            Some("py_pkg.api".to_owned())
        );
        assert_eq!(
            cargo_only_layout.python.source_roots,
            vec![cargo_only_root.to_path_buf()]
        );
    }
}
