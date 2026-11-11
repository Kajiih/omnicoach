//! Project boundary, source-root layout, and module-path resolution (`ProjectLayout`).

use super::config::{is_rooted_path, lexical_normalize, resolve_against_cwd};
use super::source::FileSource;
use crate::code_lint::ast::rust::ExternalModDeclaration;
use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

const INIT_STEM: &str = "__init__";
const INIT_PY: &str = "__init__.py";
const INIT_PYI: &str = "__init__.pyi";
const CARGO_TOML: &str = "Cargo.toml";
const PYPROJECT_TOML: &str = "pyproject.toml";
const PYTHON_EXTENSION: &str = "py";
const PYTHON_STUB_EXTENSION: &str = "pyi";
const RUST_EXTENSION: &str = "rs";
const PYTHON_SUFFIX: &str = ".py";
const PYTHON_STUB_SUFFIX: &str = ".pyi";
const RUST_SUFFIX: &str = ".rs";
const SOURCE_DIRECTORY: &str = "src";
const RUST_MODULE_SEP: &str = "::";
const SELF_SEGMENT: &str = "self";
const SUPER_SEGMENT: &str = "super";
const MAIN_CRATE_ROOT: &str = "main";
const CURRENT_DIR_PREFIX: &str = "./";
/// Crate-relative module identifier assigned to a Rust `lib.rs` crate root.
pub(super) const LIB_CRATE_ROOT: &str = "lib";
/// Synthetic module identifier representing a root `__init__.py` directly inside a Python source root.
pub(super) const ROOT_MODULE_TAG: &str = "<root>";

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

/// How a syntactic path in a Rust file was anchored before normalization.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum PathOrigin {
    /// `self` / `super` relative path, recording the shallowest ancestor module reached
    /// while resolving `super` segments (`None` if `super` climbed past the crate root).
    Relative {
        /// Shallowest ancestor module path reached during `super::` traversal.
        shallowest_ancestor: Option<String>,
    },
    /// Explicitly crate-anchored (`crate`, `crate::...`, `<crate_name>::...`) or declared child
    /// module (`mod child; child::...`) path within the current crate.
    CrateLocal,
    /// Unanchored or leading-`::` path not anchored to `crate`, `<crate_name>`, `self`, `super`,
    /// or a declared child `mod`.
    ExternalOrUnanchored,
}

/// A referenced path normalized to a canonical crate-relative (or external crate) path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ResolvedPath {
    /// How the path was anchored before normalization.
    pub(super) origin: PathOrigin,
    /// Canonical crate-relative (or external) path.
    pub(super) canonical_path: String,
}

impl ResolvedPath {
    /// Returns true when the path was anchored to the current crate (`crate`, `<crate_name>`,
    /// `self`, `super`, or a declared child `mod`).
    #[must_use]
    pub(super) const fn is_crate_local(&self) -> bool {
        matches!(
            self.origin,
            PathOrigin::Relative { .. } | PathOrigin::CrateLocal
        )
    }
}

/// An inherited component and the nearest ancestor module that declared it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InheritedComponent<'a, T> {
    /// Nearest ancestor (or self) module path that declared the component.
    pub root_module: &'a str,
    /// Inherited component value.
    pub component: &'a T,
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

/// Resolves the [`InheritedComponent`] for `module` by climbing up its `::`-separated
/// module path to the nearest ancestor (or self) in `declared_components`.
#[must_use]
pub fn resolve_inherited_component_with_root<'a, T>(
    module: &str,
    declared_components: &'a BTreeMap<String, T>,
) -> Option<InheritedComponent<'a, T>> {
    let mut current = module;
    loop {
        if let Some((root_key, component)) = declared_components.get_key_value(current) {
            return Some(InheritedComponent {
                root_module: root_key.as_str(),
                component,
            });
        }
        if let Some((parent, _)) = current.rsplit_once(RUST_MODULE_SEP) {
            current = parent;
        } else {
            return None;
        }
    }
}

/// Package prefix and shallowest ancestor reached by a relative Python import (`level >= 1`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ResolvedRelativePackage {
    /// Target package prefix (`None` when climbing to `<root>`).
    pub(super) package_prefix: Option<String>,
    /// Shallowest ancestor reached (`"<root>"` when climbing to the source root).
    pub(super) shallowest_ancestor: String,
}

/// Resolves a Python module specifier (`level == 0` absolute or `level >= 1` relative) from
/// `from_module`.
#[must_use]
pub(super) fn resolve_relative_python_module(
    from_module: &str,
    is_package_init: bool,
    level: u32,
    specifier: Option<&str>,
) -> Option<String> {
    if level == 0 {
        return specifier.map(str::to_owned);
    }
    let resolved = resolve_relative_python_package(from_module, is_package_init, level)?;
    let suffix = specifier.filter(|text| !text.is_empty());
    match (resolved.package_prefix, suffix) {
        (Some(prefix), Some(tail)) => Some(format!("{prefix}.{tail}")),
        (Some(prefix), None) => Some(prefix),
        (None, Some(tail)) => Some(tail.to_owned()),
        (None, None) => Some(String::new()),
    }
}

/// Resolves a relative Python package prefix (`level >= 1`) from `from_module`.
///
/// Returns the target package prefix (`None` when climbing to `<root>`) and the shallowest
/// ancestor reached (`"<root>"` when climbing to the source root), or `None` if `level` climbs
/// above the source root.
#[must_use]
pub(super) fn resolve_relative_python_package(
    from_module: &str,
    is_package_init: bool,
    level: u32,
) -> Option<ResolvedRelativePackage> {
    let mut parts: Vec<&str> = if from_module.is_empty() || from_module == ROOT_MODULE_TAG {
        Vec::new()
    } else {
        from_module
            .split('.')
            .filter(|segment| !segment.is_empty() && *segment != INIT_STEM)
            .collect()
    };
    if !is_package_init && !parts.is_empty() {
        parts.pop();
    }
    let up_count = (level as usize).saturating_sub(1);
    if up_count > parts.len() {
        return None;
    }
    parts.truncate(parts.len() - up_count);
    let package_prefix = (!parts.is_empty()).then(|| parts.join("."));
    let shallowest_ancestor = package_prefix
        .clone()
        .unwrap_or_else(|| ROOT_MODULE_TAG.to_owned());
    Some(ResolvedRelativePackage {
        package_prefix,
        shallowest_ancestor,
    })
}

/// Converts a forward-slash relative Python file path into a dotted module identifier, optionally
/// trimming a trailing `__init__` segment (returning `"__init__"` when the path resolves to a
/// root `__init__.py`).
#[must_use]
pub(super) fn python_dotted_path(relative_path: &str, trim_init: bool) -> String {
    let without_extension = relative_path
        .strip_suffix(PYTHON_STUB_SUFFIX)
        .or_else(|| relative_path.strip_suffix(PYTHON_SUFFIX))
        .unwrap_or(relative_path);
    let mut segments: Vec<&str> = without_extension
        .split('/')
        .filter(|segment| !segment.is_empty() && *segment != ".")
        .collect();
    if trim_init && segments.last() == Some(&INIT_STEM) {
        segments.pop();
    }
    if segments.is_empty() {
        INIT_STEM.to_owned()
    } else {
        segments.join(".")
    }
}

/// Computes the crate-relative `::`-separated module path of a Rust source file path.
#[must_use]
pub(super) fn rust_module_path(path: &str) -> String {
    let normalized = path.replace('\\', "/");
    let trimmed = Path::new(&normalized)
        .strip_prefix(SOURCE_DIRECTORY)
        .unwrap_or_else(|_| Path::new(&normalized));
    rust_module_from_relative(trimmed).unwrap_or_default()
}

/// Computes a disambiguated `::`-separated Rust module identifier preserving `src/` or parent
/// directory segments when multiple files map to the same crate-relative module name.
#[must_use]
pub(super) fn disambiguated_rust_module_path(relative_path: &str) -> String {
    relative_path
        .trim_end_matches(RUST_SUFFIX)
        .replace('/', RUST_MODULE_SEP)
        .replace('-', "_")
}

/// Normalizes `raw_path` relative to `enclosing_module`, its declared `external_mods`, and the
/// optional self-crate identifier `crate_name`.
#[must_use]
pub(super) fn resolve_rust_reference_path(
    enclosing_module: &str,
    external_mods: &[ExternalModDeclaration],
    raw_path: &str,
    crate_name: Option<&str>,
) -> ResolvedPath {
    let had_leading_separator = raw_path.starts_with(RUST_MODULE_SEP);
    let raw_path = raw_path.trim_start_matches(RUST_MODULE_SEP);
    let effective_enclosing = if is_rust_crate_root(enclosing_module) {
        ""
    } else {
        enclosing_module
    };

    if raw_path == "crate" || crate_name.is_some_and(|name| raw_path == name) {
        return ResolvedPath {
            origin: PathOrigin::CrateLocal,
            canonical_path: String::new(),
        };
    }
    if let Some(rest) = raw_path.strip_prefix("crate::").or_else(|| {
        crate_name.and_then(|name| {
            raw_path
                .strip_prefix(name)
                .and_then(|tail| tail.strip_prefix(RUST_MODULE_SEP))
        })
    }) {
        return ResolvedPath {
            origin: PathOrigin::CrateLocal,
            canonical_path: rest.to_owned(),
        };
    }
    if had_leading_separator {
        return ResolvedPath {
            origin: PathOrigin::ExternalOrUnanchored,
            canonical_path: raw_path.to_owned(),
        };
    }

    let first_segment = raw_path.split(RUST_MODULE_SEP).next().unwrap_or(raw_path);
    if matches!(first_segment, SELF_SEGMENT | SUPER_SEGMENT) {
        let mut module_parts: Vec<&str> = if effective_enclosing.is_empty() {
            Vec::new()
        } else {
            effective_enclosing.split(RUST_MODULE_SEP).collect()
        };
        let mut shallowest_depth = module_parts.len();
        let mut underflow = false;
        for segment in raw_path.split(RUST_MODULE_SEP) {
            match segment {
                SELF_SEGMENT => {}
                SUPER_SEGMENT => {
                    if module_parts.pop().is_some() {
                        shallowest_depth = shallowest_depth.min(module_parts.len());
                    } else {
                        underflow = true;
                    }
                }
                other => module_parts.push(other),
            }
        }
        let shallowest_ancestor =
            (!underflow).then(|| module_parts[..shallowest_depth].join(RUST_MODULE_SEP));
        return ResolvedPath {
            origin: PathOrigin::Relative {
                shallowest_ancestor,
            },
            canonical_path: module_parts.join(RUST_MODULE_SEP),
        };
    }

    if external_mods
        .iter()
        .any(|declaration| declaration.name == first_segment)
    {
        let canonical_path = if effective_enclosing.is_empty() {
            raw_path.to_owned()
        } else {
            format!("{effective_enclosing}{RUST_MODULE_SEP}{raw_path}")
        };
        return ResolvedPath {
            origin: PathOrigin::CrateLocal,
            canonical_path,
        };
    }

    ResolvedPath {
        origin: PathOrigin::ExternalOrUnanchored,
        canonical_path: raw_path.to_owned(),
    }
}

/// Returns true if `path` equals `module_prefix` or is a `::`-delimited descendant of it.
#[must_use]
pub(super) fn is_in_module_subtree(path: &str, module_prefix: &str) -> bool {
    if module_prefix.is_empty() {
        return true;
    }
    path == module_prefix
        || path
            .strip_prefix(module_prefix)
            .is_some_and(|rest| rest.starts_with(RUST_MODULE_SEP))
}

/// Returns true if `module_path` denotes a Rust crate root (`"lib"`, `"main"`, or `""`).
#[must_use]
pub(super) fn is_rust_crate_root(module_path: &str) -> bool {
    matches!(module_path, LIB_CRATE_ROOT | MAIN_CRATE_ROOT | "")
}

/// Returns true if `path` is a Python package initializer (`__init__.py` or `__init__.pyi`).
#[must_use]
pub(super) fn is_python_package_init(path: &Path) -> bool {
    matches!(
        path.file_name().and_then(|name| name.to_str()),
        Some(INIT_PY | INIT_PYI)
    )
}

/// Returns true if `directory` contains a `.jj` or `.git` repository/worktree boundary.
#[must_use]
pub(super) fn is_vcs_boundary(directory: &Path, source: &dyn FileSource) -> bool {
    let git_marker = directory.join(".git");
    source.is_dir(&directory.join(".jj"))
        || source.is_dir(&git_marker)
        || source.is_file(&git_marker)
}

/// Normalizes `path` using forward slashes, stripping any leading `./` prefix.
#[must_use]
pub(super) fn normalize_relative_path(path: &Path) -> String {
    let clean_path = path.strip_prefix(CURRENT_DIR_PREFIX).unwrap_or(path);
    clean_path.to_string_lossy().replace('\\', "/")
}

/// Maps a path relative to the Rust crate source root (`src/`) to its `::`-separated module path,
/// normalizing hyphens to underscores (`my-mod/sub.rs` -> `"my_mod::sub"`).
#[must_use]
fn rust_module_from_relative(relative: &Path) -> Option<String> {
    if relative
        .extension()
        .is_none_or(|extension| extension != RUST_EXTENSION)
    {
        return None;
    }
    let mut segments: Vec<String> = relative
        .components()
        .filter_map(|component| {
            component
                .as_os_str()
                .to_str()
                .map(|segment| segment.replace('-', "_"))
        })
        .collect();
    let last = segments.pop()?;
    let stem = last.strip_suffix(RUST_SUFFIX)?;
    if stem != "mod" || segments.is_empty() {
        segments.push(stem.to_owned());
    }
    (!segments.is_empty()).then(|| segments.join(RUST_MODULE_SEP))
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
    let markers = [".omnicoach.toml", PYPROJECT_TOML, CARGO_TOML];
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
    let mut roots = Vec::new();
    if start_directory.starts_with(project_root) {
        let mut cursor = start_directory;
        while cursor != project_root
            && (source.is_file(&cursor.join(INIT_PY)) || source.is_file(&cursor.join(INIT_PYI)))
        {
            let Some(parent) = cursor.parent() else {
                break;
            };
            cursor = parent;
        }
        if cursor != project_root
            && !source.is_file(&cursor.join(INIT_PY))
            && !source.is_file(&cursor.join(INIT_PYI))
        {
            roots.push(cursor.to_path_buf());
        }
    }
    let has_cargo_toml = source.is_file(&project_root.join(CARGO_TOML));
    let has_pyproject_toml = source.is_file(&project_root.join(PYPROJECT_TOML));
    for candidate_name in [SOURCE_DIRECTORY, "python", "Lib"] {
        let candidate = project_root.join(candidate_name);
        let allow_candidate = candidate_name != SOURCE_DIRECTORY
            || !has_cargo_toml
            || (has_pyproject_toml && directory_contains_python_package(&candidate, source));
        if allow_candidate
            && is_non_package_directory(&candidate, source)
            && !roots.contains(&candidate)
        {
            roots.push(candidate);
        }
    }
    if !roots.contains(&project_root.to_path_buf()) {
        roots.push(project_root.to_path_buf());
    }
    roots
}

fn is_non_package_directory(directory: &Path, source: &dyn FileSource) -> bool {
    source.is_dir(directory)
        && !source.is_file(&directory.join(INIT_PY))
        && !source.is_file(&directory.join(INIT_PYI))
}

fn directory_contains_python_package(directory: &Path, source: &dyn FileSource) -> bool {
    source
        .read_dir(directory)
        .unwrap_or_default()
        .into_iter()
        .any(|entry| {
            source.is_dir(&entry)
                && (source.is_file(&entry.join(INIT_PY)) || source.is_file(&entry.join(INIT_PYI)))
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

#[cfg(test)]
mod tests {
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
    fn resolve_rust_reference_path_handles_leading_double_colon_paths() {
        let leading_colon_self = resolve_rust_reference_path(
            "runner",
            &[],
            "::my_custom_crate::core::Config",
            Some("my_custom_crate"),
        );
        let leading_colon_external_with_shadow_mod = resolve_rust_reference_path(
            "runner",
            &[ExternalModDeclaration {
                name: "serde".to_owned(),
                declaration_text: "mod serde;".to_owned(),
                is_private: true,
                span: crate::diagnostic::SourceSpan { start: 0, end: 10 },
            }],
            "::serde::Deserialize",
            Some("my_custom_crate"),
        );

        assert_eq!(leading_colon_self.canonical_path, "core::Config");
        assert!(leading_colon_self.is_crate_local());
        assert_eq!(
            leading_colon_external_with_shadow_mod.canonical_path,
            "serde::Deserialize"
        );
        assert!(!leading_colon_external_with_shadow_mod.is_crate_local());
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
}
