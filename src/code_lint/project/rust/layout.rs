//! Rust crate source-root discovery and file-to-module mapping (`CodeLintProjectRust`).

use super::paths::{LIB_CRATE_ROOT, MAIN_CRATE_ROOT, rust_module_from_relative};
use crate::code_lint::project::foundation::{FileSource, WorkspaceAnchor};
use serde::Deserialize;
use std::path::{Path, PathBuf};

const CARGO_TOML: &str = "Cargo.toml";
const SOURCE_DIRECTORY: &str = "src";

/// Rust crate source root, crate identifier, and file-to-module mapping for a discovered project.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::code_lint::project) struct RustLayout {
    anchor: WorkspaceAnchor,
    has_cargo_toml: bool,
    /// Rust crate source root (`<project_root>/src` when `<project_root>/src` exists, else `project_root`).
    pub(in crate::code_lint::project) source_root: PathBuf,
    /// Normalized Rust crate identifier (`[lib] name` or `[package] name` with `-` replaced by `_`),
    /// when `Cargo.toml` is present at `project_root`.
    pub(in crate::code_lint::project) crate_name: Option<String>,
}

impl RustLayout {
    /// Discovers Rust crate layout (`source_root` and `crate_name`) at `anchor.project_root`.
    #[must_use]
    pub(in crate::code_lint::project) fn discover(
        anchor: WorkspaceAnchor,
        source: &dyn FileSource,
    ) -> Self {
        let project_root = &anchor.project_root;
        let has_cargo_toml = source.is_file(&project_root.join(CARGO_TOML));
        let cargo_source = project_root.join(SOURCE_DIRECTORY);
        let source_root = if source.is_dir(&cargo_source) {
            cargo_source
        } else {
            project_root.clone()
        };
        let crate_name = has_cargo_toml
            .then(|| parse_cargo_crate_name(project_root, source))
            .flatten();
        Self {
            anchor,
            has_cargo_toml,
            source_root,
            crate_name,
        }
    }

    /// Returns [`Self::source_root`] when `Cargo.toml` at `project_root` claims a dedicated
    /// subdirectory (such as `src/`).
    #[must_use]
    pub(in crate::code_lint::project) fn claims_source_dir(&self) -> Option<&Path> {
        (self.has_cargo_toml && self.source_root != self.anchor.project_root)
            .then_some(self.source_root.as_path())
    }

    /// Maps a Rust `.rs` file path to its crate-relative module path (`"lib"`, `"code_lint::ast"`).
    #[must_use]
    pub(in crate::code_lint::project) fn module_of_path(&self, path: &Path) -> Option<String> {
        let resolved = self.anchor.resolve_query_path(path);
        let relative = resolved.strip_prefix(&self.source_root).ok()?;
        rust_module_from_relative(relative)
    }

    /// Resolves a crate-relative Rust module path (`"lib"`, `"code_lint::ast"`) to its `.rs` file.
    #[must_use]
    pub(in crate::code_lint::project) fn module_file(
        &self,
        module: &str,
        source: &dyn FileSource,
    ) -> Option<PathBuf> {
        if module == LIB_CRATE_ROOT || module == MAIN_CRATE_ROOT {
            let candidate = self.source_root.join(format!("{module}.rs"));
            return source.is_file(&candidate).then_some(candidate);
        }
        let relative = module.replace("::", "/");
        let modern = self.source_root.join(format!("{relative}.rs"));
        if source.is_file(&modern) {
            return Some(modern);
        }
        let legacy = self.source_root.join(&relative).join("mod.rs");
        if source.is_file(&legacy) {
            return Some(legacy);
        }
        None
    }

    /// Returns true when walking `resolved_walk_dir` at a Cargo crate root should exclude
    /// `full_path` because it lies outside [`Self::source_root`] (e.g. `tests/*.rs`, `benches/*.rs`).
    #[must_use]
    pub(in crate::code_lint::project) fn is_outside_source_root(
        &self,
        resolved_walk_dir: &Path,
        full_path: &Path,
    ) -> bool {
        self.has_cargo_toml
            && resolved_walk_dir == self.anchor.project_root
            && self.source_root != self.anchor.project_root
            && !full_path.starts_with(&self.source_root)
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::code_lint::project::foundation::MemoryFileSource;

    #[test]
    fn discover_extracts_normalized_crate_name_from_cargo_toml() {
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
        let layout = RustLayout::discover(WorkspaceAnchor::new(root.to_path_buf(), None), &source);

        assert_eq!(layout.crate_name.as_deref(), Some("my_custom_crate"));
        assert_eq!(layout.claims_source_dir(), Some(root.join("src").as_path()));
        assert!(layout.is_outside_source_root(root, &root.join("tests/cli.rs")));
        assert!(!layout.is_outside_source_root(root, &root.join("src/lib.rs")));
    }

    #[test]
    fn claims_source_dir_requires_both_cargo_toml_and_dedicated_src_dir() {
        let root = Path::new("/repo");
        let without_cargo =
            MemoryFileSource::from_relative_files(root, &[("src/pkg.py", "X = 1\n")]);
        let without_src = MemoryFileSource::from_relative_files(
            root,
            &[
                ("Cargo.toml", "[package]\nname = \"flat\"\n"),
                ("lib.rs", ""),
            ],
        );
        let non_cargo_layout = RustLayout::discover(
            WorkspaceAnchor::new(root.to_path_buf(), None),
            &without_cargo,
        );
        let flat_cargo_layout =
            RustLayout::discover(WorkspaceAnchor::new(root.to_path_buf(), None), &without_src);

        assert_eq!(non_cargo_layout.claims_source_dir(), None);
        assert_eq!(flat_cargo_layout.claims_source_dir(), None);
    }
}
