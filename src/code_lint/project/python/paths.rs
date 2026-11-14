//! Syntactic module-path and relative-package resolution for Python (`CodeLintProjectPython`).

use crate::code_lint::project::foundation::ROOT_MODULE_TAG;
use std::path::Path;

/// Stem of a Python package initializer file (`__init__.py` / `__init__.pyi`).
pub(super) const INIT_STEM: &str = "__init__";
/// Filename of a Python package initializer (`__init__.py`).
pub(super) const INIT_PY: &str = "__init__.py";
/// Filename of a Python type-stub package initializer (`__init__.pyi`).
pub(super) const INIT_PYI: &str = "__init__.pyi";

const PYTHON_SUFFIX: &str = ".py";
const PYTHON_STUB_SUFFIX: &str = ".pyi";
const PYTHON_MODULE_SEP: &str = ".";

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

/// Returns true if `module_path` equals `prefix` or is a `.`-delimited Python descendant of it.
#[must_use]
pub(super) fn is_in_python_module_subtree(module_path: &str, prefix: &str) -> bool {
    module_path == prefix
        || module_path
            .strip_prefix(prefix)
            .is_some_and(|rest| rest.starts_with(PYTHON_MODULE_SEP))
}
