//! Syntactic module-path and reference-path resolution for Rust (`CodeLintProjectRust`).

use crate::code_lint::ast::rust::ExternalModDeclaration;
use std::collections::BTreeMap;
use std::path::Path;

/// Crate-relative module identifier assigned to a Rust `lib.rs` crate root.
pub(super) const LIB_CRATE_ROOT: &str = "lib";
/// Crate-relative module identifier assigned to a Rust `main.rs` crate root.
pub(super) const MAIN_CRATE_ROOT: &str = "main";

const RUST_EXTENSION: &str = "rs";
const RUST_SUFFIX: &str = ".rs";
const SOURCE_DIRECTORY: &str = "src";
const RUST_MODULE_SEP: &str = "::";
const SELF_SEGMENT: &str = "self";
const SUPER_SEGMENT: &str = "super";

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

/// An inherited component and the nearest ancestor Rust module that declared it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InheritedComponent<'a, T> {
    /// Nearest ancestor (or self) module path that declared the component.
    pub root_module: &'a str,
    /// Inherited component value.
    pub component: &'a T,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subtree_inheritance_resolves_leaf_modules_and_rejects_namespace_routers() {
        let declared_components = BTreeMap::from([
            ("domain::unit".to_owned(), "CodeLintRules"),
            ("other".to_owned(), "CommandLintRules"),
        ]);
        let queries = [
            "domain::unit",
            "domain::unit::leaf::deeper",
            "other::leaf",
            "domain",
            "domain::unit_sibling",
        ];
        let resolved: Vec<Option<&str>> = queries
            .into_iter()
            .map(|module| {
                resolve_inherited_component_with_root(module, &declared_components)
                    .map(|inherited| *inherited.component)
            })
            .collect();

        assert_eq!(
            resolved,
            vec![
                Some("CodeLintRules"),
                Some("CodeLintRules"),
                Some("CommandLintRules"),
                None,
                None,
            ]
        );
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
    fn resolve_rust_reference_path_normalizes_self_crate_and_backslash_paths() {
        assert_eq!(
            resolve_rust_reference_path(
                "runner",
                &[],
                "my_custom_crate::core::Config",
                Some("my_custom_crate"),
            )
            .canonical_path,
            "core::Config"
        );
        assert_eq!(
            resolve_rust_reference_path(
                "runner",
                &[],
                "omnicoach::core::Config",
                Some("my_custom_crate"),
            )
            .canonical_path,
            "omnicoach::core::Config"
        );
        assert_eq!(rust_module_path(r"src\sub\mod.rs"), "sub");
    }
}
