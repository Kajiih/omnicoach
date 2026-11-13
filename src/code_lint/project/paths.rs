//! Syntactic module-path and reference-path resolution for Rust and Python (`CodeLintProject`).

use crate::code_lint::ast::rust::ExternalModDeclaration;
use std::path::Path;

// TODO: Is this the right abstraction to contain language specific components?
/// Stem of a Python package initializer file (`__init__.py` / `__init__.pyi`).
pub(super) const INIT_STEM: &str = "__init__";
/// Filename of a Python package initializer (`__init__.py`).
pub(super) const INIT_PY: &str = "__init__.py";
/// Filename of a Python type-stub package initializer (`__init__.pyi`).
pub(super) const INIT_PYI: &str = "__init__.pyi";
/// Crate-relative module identifier assigned to a Rust `lib.rs` crate root.
pub(super) const LIB_CRATE_ROOT: &str = "lib";
/// Crate-relative module identifier assigned to a Rust `main.rs` crate root.
pub(super) const MAIN_CRATE_ROOT: &str = "main";
/// Synthetic module identifier representing a root `__init__.py` directly inside a Python source root.
pub(super) const ROOT_MODULE_TAG: &str = "<root>";

const PYTHON_SUFFIX: &str = ".py";
const PYTHON_STUB_SUFFIX: &str = ".pyi";
const RUST_EXTENSION: &str = "rs";
const RUST_SUFFIX: &str = ".rs";
const SOURCE_DIRECTORY: &str = "src";
const RUST_MODULE_SEP: &str = "::";
const PYTHON_MODULE_SEP: &str = ".";
const SELF_SEGMENT: &str = "self";
const SUPER_SEGMENT: &str = "super";
const CURRENT_DIR_PREFIX: &str = "./";

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

/// Package prefix and shallowest ancestor reached by a relative Python import (`level >= 1`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ResolvedRelativePackage {
    /// Target package prefix (`None` when climbing to `<root>`).
    pub(super) package_prefix: Option<String>,
    /// Shallowest ancestor reached (`"<root>"` when climbing to the source root).
    pub(super) shallowest_ancestor: String,
}

/// Returns true if `path` is a Python package initializer (`__init__.py` or `__init__.pyi`).
#[must_use]
pub(super) fn is_python_package_init(path: &Path) -> bool {
    matches!(
        path.file_name().and_then(|name| name.to_str()),
        Some(INIT_PY | INIT_PYI)
    )
}

/// Normalizes `path` using forward slashes, stripping any leading `./` prefix.
#[must_use]
pub(super) fn normalize_relative_path(path: &Path) -> String {
    let clean_path = path.strip_prefix(CURRENT_DIR_PREFIX).unwrap_or(path);
    clean_path.to_string_lossy().replace('\\', "/")
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

/// Maps a path relative to the Rust crate source root (`src/`) to its `::`-separated module path,
/// normalizing hyphens to underscores (`my-mod/sub.rs` -> `"my_mod::sub"`).
#[must_use]
pub(super) fn rust_module_from_relative(relative: &Path) -> Option<String> {
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

/// Returns true if `module_path` denotes a Rust crate root (`"lib"`, `"main"`, or `""`).
#[must_use]
pub(super) fn is_rust_crate_root(module_path: &str) -> bool {
    matches!(module_path, LIB_CRATE_ROOT | MAIN_CRATE_ROOT | "")
}

/// Returns true if `path` equals `module_prefix` or is a `::`-delimited Rust descendant of it.
#[must_use]
pub(super) fn is_in_rust_module_subtree(path: &str, module_prefix: &str) -> bool {
    if module_prefix.is_empty() {
        return true;
    }
    path == module_prefix
        || path
            .strip_prefix(module_prefix)
            .is_some_and(|rest| rest.starts_with(RUST_MODULE_SEP))
}

/// Returns true if `module_path` equals `prefix` or is a `.`-delimited Python descendant of it.
#[must_use]
pub(super) fn is_in_python_module_subtree(module_path: &str, prefix: &str) -> bool {
    module_path == prefix
        || module_path
            .strip_prefix(prefix)
            .is_some_and(|rest| rest.starts_with(PYTHON_MODULE_SEP))
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
