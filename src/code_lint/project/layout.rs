//! Rust module path normalization and architectural component inheritance (`CodeLintProject`).

use crate::code_lint::ast::rust::ExternalModDeclaration;
use std::collections::BTreeMap;
use std::path::Path;

const MODULE_SEP: &str = "::";
const SELF_SEGMENT: &str = "self";
const SUPER_SEGMENT: &str = "super";

/// How a syntactic path in a Rust file was anchored before normalization.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PathOrigin {
    /// `self` / `super` relative path, recording the shallowest ancestor module reached
    /// while resolving `super` segments (`None` if `super` climbed past the crate root).
    Relative {
        /// Shallowest ancestor module path reached during `super::` traversal.
        shallowest_ancestor: Option<String>,
    },
    /// Crate-absolute (`crate::`, `omnicoach::`), local child module, or external/local path.
    NonRelative,
}

/// A referenced path normalized to a canonical crate-relative (or external crate) path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedPath {
    /// How the path was anchored before normalization.
    pub origin: PathOrigin,
    /// Canonical crate-relative (or external) path.
    pub canonical_path: String,
}

/// An inherited component and the nearest ancestor module that declared it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InheritedComponent<'a, T> {
    /// Nearest ancestor (or self) module path that declared the component.
    pub root_module: &'a str,
    /// Inherited component value.
    pub component: &'a T,
}

/// Computes the crate-relative `::`-separated module path of a Rust source file path.
#[must_use]
pub fn rust_module_path(path: &str) -> String {
    let trimmed = path.strip_prefix("src/").unwrap_or(path);
    let without_extension = trimmed.strip_suffix(".rs").unwrap_or(trimmed);
    let mut parts: Vec<&str> = without_extension
        .split('/')
        .filter(|segment| !segment.is_empty())
        .collect();
    if parts.len() > 1 && parts.last() == Some(&"mod") {
        parts.pop();
    }
    parts.join(MODULE_SEP).replace('-', "_")
}

/// Returns true if `module_path` denotes a Rust crate root (`"lib"`, `"main"`, or `""`).
#[must_use]
pub fn is_rust_crate_root(module_path: &str) -> bool {
    matches!(module_path, "lib" | "main" | "")
}

/// Normalizes `raw_path` relative to `enclosing_module`, its declared `external_mods`, and the
/// optional self-crate identifier `crate_name`.
#[must_use]
pub fn resolve_rust_reference_path(
    enclosing_module: &str,
    external_mods: &[ExternalModDeclaration],
    raw_path: &str,
    crate_name: Option<&str>,
) -> ResolvedPath {
    let effective_enclosing = if is_rust_crate_root(enclosing_module) {
        ""
    } else {
        enclosing_module
    };

    if raw_path == "crate" || crate_name.is_some_and(|name| raw_path == name) {
        return ResolvedPath {
            origin: PathOrigin::NonRelative,
            canonical_path: String::new(),
        };
    }
    if let Some(rest) = raw_path.strip_prefix("crate::").or_else(|| {
        crate_name.and_then(|name| {
            raw_path
                .strip_prefix(name)
                .and_then(|tail| tail.strip_prefix(MODULE_SEP))
        })
    }) {
        return ResolvedPath {
            origin: PathOrigin::NonRelative,
            canonical_path: rest.to_owned(),
        };
    }

    let first_segment = raw_path.split(MODULE_SEP).next().unwrap_or(raw_path);
    if matches!(first_segment, SELF_SEGMENT | SUPER_SEGMENT) {
        let mut module_parts: Vec<&str> = if effective_enclosing.is_empty() {
            Vec::new()
        } else {
            effective_enclosing.split(MODULE_SEP).collect()
        };
        let mut shallowest_depth = module_parts.len();
        let mut underflow = false;
        for segment in raw_path.split(MODULE_SEP) {
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
            (!underflow).then(|| module_parts[..shallowest_depth].join(MODULE_SEP));
        return ResolvedPath {
            origin: PathOrigin::Relative {
                shallowest_ancestor,
            },
            canonical_path: module_parts.join(MODULE_SEP),
        };
    }

    if external_mods
        .iter()
        .any(|declaration| declaration.name == first_segment)
    {
        let canonical_path = if effective_enclosing.is_empty() {
            raw_path.to_owned()
        } else {
            format!("{effective_enclosing}{MODULE_SEP}{raw_path}")
        };
        return ResolvedPath {
            origin: PathOrigin::NonRelative,
            canonical_path,
        };
    }

    ResolvedPath {
        origin: PathOrigin::NonRelative,
        canonical_path: raw_path.to_owned(),
    }
}

/// Returns true if `path` equals `module_prefix` or is a `::`-delimited descendant of it.
#[must_use]
pub fn is_in_module_subtree(path: &str, module_prefix: &str) -> bool {
    if module_prefix.is_empty() {
        return true;
    }
    path == module_prefix
        || path
            .strip_prefix(module_prefix)
            .is_some_and(|rest| rest.starts_with(MODULE_SEP))
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
        if let Some((parent, _)) = current.rsplit_once(MODULE_SEP) {
            current = parent;
        } else {
            return None;
        }
    }
}

const CURRENT_DIR_PREFIX: &str = "./";
const SRC_DIR_NAME: &str = "src";

/// Normalizes `path` relative to `base_dir` (when provided) using forward slashes.
#[must_use]
pub fn normalize_relative_path(path: &Path, base_dir: Option<&Path>) -> String {
    let clean_path = path.strip_prefix(CURRENT_DIR_PREFIX).unwrap_or(path);
    let relative = base_dir.map_or(clean_path, |base| {
        let clean_base = base.strip_prefix(CURRENT_DIR_PREFIX).unwrap_or(base);
        if clean_base == Path::new(".") || clean_base.starts_with(SRC_DIR_NAME) {
            clean_path
        } else {
            clean_path.strip_prefix(clean_base).unwrap_or(clean_path)
        }
    });
    relative.to_string_lossy().replace('\\', "/")
}
