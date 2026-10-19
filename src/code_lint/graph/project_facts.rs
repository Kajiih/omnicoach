//! Multi-file project abstraction facts and cross-module symbol resolution (`CodeLintGraph`).

use crate::code_lint::ast::python::collect_path_references;
use crate::code_lint::ast::rust::summarize_rust_file;
use crate::code_lint::ast::{DeclarationScopeKind, ParsedFile, ScopeFacts, collect_scope_facts};
use crate::diagnostic::Language;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

const INIT_MODULE: &str = "__init__";
const LIB_MODULE: &str = "lib";
const SELF_SEGMENT: &str = "self";
const SUPER_SEGMENT: &str = "super";

/// Multi-file project facts across a Rust crate or Python package.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectFacts {
    /// Human-readable project or package name.
    pub title: String,
    /// True if architectural components and their allowed dependencies are explicitly declared
    /// (for example via `define_architecture!` + `architecture_component!`).
    pub has_declared_architecture: bool,
    /// Architectural components (declared or inferred from top-level directory / package structure).
    pub components: Vec<ComponentFact>,
    /// Parsed modules in deterministic path order.
    pub modules: Vec<ModuleFact>,
    /// Resolved cross-module reference edges (`source_module -> target_module`).
    pub cross_module_refs: Vec<CrossModuleRefFact>,
}

/// An architectural component and its declared dependency list (empty when inferred).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComponentFact {
    /// Component identifier (e.g. `"CodeLintAst"` or `"zoneinfo"`).
    pub name: String,
    /// Directly declared dependency component names (when [`ProjectFacts::has_declared_architecture`]
    /// is `true`).
    pub declared_dependencies: Vec<String>,
}

/// One module file within [`ProjectFacts`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModuleFact {
    /// Canonical module path (`"code_lint::ast"` or `"zoneinfo._tzpath"`).
    pub module_path: String,
    /// Relative source file path (`"src/code_lint/ast.rs"`).
    pub path: String,
    /// Owning component name.
    pub component: String,
    /// Per-scope facts inside this module (`scopes[0]` is the module scope).
    pub scopes: Vec<ScopeFacts>,
}

/// A resolved reference from one module (and optional top-level item) to another module in the
/// same project.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct CrossModuleRefFact {
    /// Referring module path.
    pub source_module: String,
    /// Referring top-level symbol in `source_module`, when the reference lies inside a top-level
    /// declaration.
    pub source_symbol: Option<String>,
    /// Target module path.
    pub target_module: String,
    /// Referenced top-level symbol in `target_module`, when resolved to a specific item.
    pub target_symbol: Option<String>,
    /// 1-based line number of the reference in `source_module`.
    pub line: usize,
}

/// Collects [`ProjectFacts`] from an in-memory slice of `(relative_path, source_text)` pairs.
#[must_use]
pub fn collect_project_facts_from_sources(
    title: &str,
    lang: Language,
    files: &[(&str, &str)],
) -> ProjectFacts {
    match lang {
        Language::Rust => collect_rust_project_facts(title, files),
        Language::Python => collect_python_project_facts(title, files),
    }
}

fn collect_rust_project_facts(title: &str, files: &[(&str, &str)]) -> ProjectFacts {
    let entries: Vec<RustModuleEntry> = files
        .iter()
        .map(|&(path, source)| parse_rust_module(path, source))
        .collect();

    let mut components: Vec<ComponentFact> = entries
        .iter()
        .flat_map(|entry| &entry.defined_architecture)
        .map(|(name, declared_dependencies)| ComponentFact {
            name: name.clone(),
            declared_dependencies: declared_dependencies.clone(),
        })
        .collect();
    let has_declared_architecture = !components.is_empty();
    let declared_roots = collect_declared_component_roots(&entries);

    let mut modules = Vec::new();
    let mut seen_components: BTreeSet<String> =
        components.iter().map(|comp| comp.name.clone()).collect();

    for entry in &entries {
        if entry.is_crate_root && !entry.has_non_namespace_items {
            continue;
        }
        let component = resolve_inherited_rust_component(&entry.module_path, &declared_roots)
            .unwrap_or_else(|| {
                entry
                    .module_path
                    .split("::")
                    .next()
                    .unwrap_or("root")
                    .to_owned()
            });
        if seen_components.insert(component.clone()) {
            components.push(ComponentFact {
                name: component.clone(),
                declared_dependencies: Vec::new(),
            });
        }
        modules.push(ModuleFact {
            module_path: entry.module_path.clone(),
            path: entry.path.clone(),
            component,
            scopes: entry.scopes.clone(),
        });
    }

    let cross_module_refs = resolve_rust_cross_module_refs(&entries);
    ProjectFacts {
        title: title.to_owned(),
        has_declared_architecture,
        components,
        modules,
        cross_module_refs,
    }
}

struct RustModuleEntry {
    path: String,
    module_path: String,
    is_crate_root: bool,
    declared_component: Option<String>,
    defined_architecture: Vec<(String, Vec<String>)>,
    external_mods: Vec<String>,
    visible_reexports: Vec<String>,
    has_non_namespace_items: bool,
    references: Vec<(usize, String)>,
    scopes: Vec<ScopeFacts>,
}

fn parse_rust_module(path: &str, source: &str) -> RustModuleEntry {
    let module_path = rust_module_path(path);
    let is_crate_root = matches!(module_path.as_str(), LIB_MODULE | "main" | "");
    let parsed = ParsedFile::rust(source);
    let summary = summarize_rust_file(&parsed);
    let declared_component = summary.architecture_components.first().cloned();
    let defined_architecture = summary.defined_architecture;
    let external_mods = summary
        .external_mods
        .into_iter()
        .map(|decl| decl.name)
        .collect();
    let visible_reexports = summary
        .visible_uses
        .into_iter()
        .flat_map(|decl| decl.target_paths)
        .collect();
    let has_non_namespace_items = summary
        .non_namespace_items
        .iter()
        .any(|(_, text)| !text.starts_with("architecture_component!"));
    let references = summary
        .referenced_paths
        .into_iter()
        .map(|reference| (reference.line, reference.raw_path))
        .collect();
    let scopes = collect_scope_facts(&parsed);
    RustModuleEntry {
        path: path.to_owned(),
        module_path,
        is_crate_root,
        declared_component,
        defined_architecture,
        external_mods,
        visible_reexports,
        has_non_namespace_items,
        references,
        scopes,
    }
}

fn rust_module_path(path: &str) -> String {
    let trimmed = path.strip_prefix("src/").unwrap_or(path);
    let without_extension = trimmed.strip_suffix(".rs").unwrap_or(trimmed);
    let mut parts: Vec<&str> = without_extension
        .split('/')
        .filter(|segment| !segment.is_empty())
        .collect();
    if parts.len() > 1 && parts.last() == Some(&"mod") {
        parts.pop();
    }
    parts.join("::").replace('-', "_")
}

fn collect_declared_component_roots(entries: &[RustModuleEntry]) -> BTreeMap<String, String> {
    let mut roots = BTreeMap::new();
    for entry in entries {
        if let Some(component) = &entry.declared_component {
            roots.insert(entry.module_path.clone(), component.clone());
        }
    }
    roots
}

fn resolve_inherited_rust_component(
    module_path: &str,
    declared_roots: &BTreeMap<String, String>,
) -> Option<String> {
    let mut current = module_path;
    loop {
        if let Some(component) = declared_roots.get(current) {
            return Some(component.clone());
        }
        if let Some((parent, _)) = current.rsplit_once("::") {
            current = parent;
        } else {
            return None;
        }
    }
}

fn resolve_rust_cross_module_refs(entries: &[RustModuleEntry]) -> Vec<CrossModuleRefFact> {
    let known_modules: HashSet<&str> = entries
        .iter()
        .map(|entry| entry.module_path.as_str())
        .collect();
    let mut module_symbols: HashMap<&str, HashSet<&str>> = HashMap::new();
    for entry in entries {
        let symbols = module_symbols
            .entry(entry.module_path.as_str())
            .or_default();
        if let Some(module_scope) = entry.scopes.first() {
            for item in &module_scope.items {
                symbols.insert(item.name.as_str());
            }
        }
    }
    let lib_root_names: HashSet<&str> = entries
        .iter()
        .find(|entry| entry.module_path == LIB_MODULE)
        .into_iter()
        .flat_map(|lib_entry| {
            lib_entry.external_mods.iter().map(String::as_str).chain(
                module_symbols
                    .get(LIB_MODULE)
                    .into_iter()
                    .flatten()
                    .copied(),
            )
        })
        .collect();
    let reexports = collect_rust_reexports(entries, &lib_root_names);

    let mut refs = Vec::new();
    for entry in entries {
        for &(line, ref raw_path) in &entry.references {
            let canonical = normalize_rust_reference_path(
                &entry.module_path,
                entry.is_crate_root,
                &entry.external_mods,
                &lib_root_names,
                raw_path,
            );
            if let Some(target) =
                resolve_rust_target(&canonical, &known_modules, &module_symbols, &reexports)
                && target.module != entry.module_path
            {
                refs.push(CrossModuleRefFact {
                    source_module: entry.module_path.clone(),
                    source_symbol: enclosing_symbol(&entry.scopes, line),
                    target_module: target.module,
                    target_symbol: target.symbol,
                    line,
                });
            }
        }
    }
    refs.sort();
    refs.dedup();
    refs
}

fn collect_rust_reexports(
    entries: &[RustModuleEntry],
    lib_root_names: &HashSet<&str>,
) -> HashMap<(String, String), String> {
    let mut reexports = HashMap::new();
    for entry in entries {
        for target in &entry.visible_reexports {
            let canonical = normalize_rust_reference_path(
                &entry.module_path,
                entry.is_crate_root,
                &entry.external_mods,
                lib_root_names,
                target,
            );
            if let Some(exported) = canonical.rsplit("::").next() {
                reexports.insert((entry.module_path.clone(), exported.to_owned()), canonical);
            }
        }
    }
    reexports
}

fn normalize_rust_reference_path(
    enclosing_module: &str,
    is_crate_root: bool,
    external_mods: &[String],
    lib_root_names: &HashSet<&str>,
    raw_path: &str,
) -> String {
    let effective_enclosing = if is_crate_root { "" } else { enclosing_module };
    if raw_path == "crate" {
        return String::new();
    }
    if let Some(rest) = raw_path.strip_prefix("crate::") {
        return rest.to_owned();
    }
    let first_segment = raw_path.split("::").next().unwrap_or(raw_path);
    if matches!(first_segment, SELF_SEGMENT | SUPER_SEGMENT) {
        let mut parts: Vec<&str> = if effective_enclosing.is_empty() {
            Vec::new()
        } else {
            effective_enclosing.split("::").collect()
        };
        for segment in raw_path.split("::") {
            match segment {
                SELF_SEGMENT => {}
                SUPER_SEGMENT => {
                    parts.pop();
                }
                other => parts.push(other),
            }
        }
        return parts.join("::");
    }
    if external_mods.iter().any(|name| name == first_segment) {
        return if effective_enclosing.is_empty() {
            raw_path.to_owned()
        } else {
            format!("{effective_enclosing}::{raw_path}")
        };
    }
    if let Some((_, rest)) = raw_path.split_once("::") {
        let next_segment = rest.split("::").next().unwrap_or(rest);
        if !matches!(first_segment, "std" | "core" | "alloc")
            && lib_root_names.contains(next_segment)
        {
            return rest.to_owned();
        }
    }
    raw_path.to_owned()
}

fn collect_python_project_facts(title: &str, files: &[(&str, &str)]) -> ProjectFacts {
    let entries: Vec<PythonModuleEntry> = files
        .iter()
        .map(|&(path, source)| parse_python_module(path, source))
        .collect();
    let cross_module_refs = resolve_python_cross_module_refs(&entries);
    let mut modules = Vec::with_capacity(entries.len());
    let mut seen_components = BTreeSet::new();
    let mut components = Vec::new();

    for entry in entries {
        let component = python_component_name(&entry.module_path, title);
        if seen_components.insert(component.clone()) {
            components.push(ComponentFact {
                name: component.clone(),
                declared_dependencies: Vec::new(),
            });
        }
        modules.push(ModuleFact {
            module_path: entry.module_path,
            path: entry.path,
            component,
            scopes: entry.scopes,
        });
    }

    ProjectFacts {
        title: title.to_owned(),
        has_declared_architecture: false,
        components,
        modules,
        cross_module_refs,
    }
}

struct PythonModuleEntry {
    path: String,
    module_path: String,
    is_package_init: bool,
    references: Vec<(usize, String)>,
    scopes: Vec<ScopeFacts>,
}

fn parse_python_module(path: &str, source: &str) -> PythonModuleEntry {
    let is_package_init = path.ends_with("/__init__.py") || path == "__init__.py";
    let module_path = python_module_path(path);
    let parsed = ParsedFile::new(source, Language::Python);
    let references = collect_path_references(&parsed);
    let scopes = collect_scope_facts(&parsed);
    PythonModuleEntry {
        path: path.to_owned(),
        module_path,
        is_package_init,
        references,
        scopes,
    }
}

fn python_module_path(path: &str) -> String {
    let without_extension = path.strip_suffix(".py").unwrap_or(path);
    let mut parts: Vec<&str> = without_extension
        .split('/')
        .filter(|segment| !segment.is_empty())
        .collect();
    if parts.len() > 1 && parts.last() == Some(&INIT_MODULE) {
        parts.pop();
    }
    parts.join(".")
}

fn python_component_name(module_path: &str, fallback_title: &str) -> String {
    module_path
        .split('.')
        .next()
        .filter(|segment| !segment.is_empty() && *segment != INIT_MODULE)
        .unwrap_or(fallback_title)
        .to_owned()
}

fn resolve_python_cross_module_refs(entries: &[PythonModuleEntry]) -> Vec<CrossModuleRefFact> {
    let known_modules: HashSet<&str> = entries
        .iter()
        .map(|entry| entry.module_path.as_str())
        .collect();
    let mut refs = Vec::new();
    for entry in entries {
        for &(line, ref raw_path) in &entry.references {
            let normalized =
                normalize_python_import_path(&entry.module_path, entry.is_package_init, raw_path);
            if let Some(target) = longest_module_prefix(&normalized, ".", &known_modules)
                && target.module != entry.module_path
            {
                refs.push(CrossModuleRefFact {
                    source_module: entry.module_path.clone(),
                    source_symbol: enclosing_symbol(&entry.scopes, line),
                    target_module: target.module,
                    target_symbol: target.symbol,
                    line,
                });
            }
        }
    }
    refs.sort();
    refs.dedup();
    refs
}

fn enclosing_symbol(scopes: &[ScopeFacts], line: usize) -> Option<String> {
    for scope in scopes {
        match &scope.scope {
            DeclarationScopeKind::Module => {
                if let Some(item) = scope
                    .items
                    .iter()
                    .find(|item| item.line <= line && line <= item.end_line.unwrap_or(item.line))
                {
                    return Some(item.name.clone());
                }
            }
            DeclarationScopeKind::Class { name }
            | DeclarationScopeKind::Impl { type_name: name } => {
                if scope
                    .items
                    .iter()
                    .any(|item| item.line <= line && line <= item.end_line.unwrap_or(item.line))
                {
                    return Some(name.clone());
                }
            }
        }
    }
    None
}

fn normalize_python_import_path(
    enclosing_module: &str,
    is_package_init: bool,
    raw_imported: &str,
) -> String {
    let dots = raw_imported
        .chars()
        .take_while(|&character| character == '.')
        .count();
    if dots == 0 {
        return raw_imported.to_owned();
    }
    let mut package_parts: Vec<&str> = enclosing_module
        .split('.')
        .filter(|seg| !seg.is_empty() && *seg != INIT_MODULE)
        .collect();
    if !is_package_init && !package_parts.is_empty() {
        package_parts.pop();
    }
    for _ in 1..dots {
        package_parts.pop();
    }
    let rest = &raw_imported[dots..];
    if !rest.is_empty() {
        for segment in rest.split('.').filter(|seg| !seg.is_empty()) {
            package_parts.push(segment);
        }
    }
    package_parts.join(".")
}

struct ResolvedModuleTarget {
    module: String,
    symbol: Option<String>,
}

fn resolve_rust_target(
    canonical: &str,
    known_modules: &HashSet<&str>,
    module_symbols: &HashMap<&str, HashSet<&str>>,
    reexports: &HashMap<(String, String), String>,
) -> Option<ResolvedModuleTarget> {
    if canonical.is_empty() {
        return None;
    }
    if let Some(target) = longest_module_prefix(canonical, "::", known_modules) {
        if let Some(sym) = &target.symbol
            && !module_symbols
                .get(target.module.as_str())
                .is_some_and(|set| set.contains(sym.as_str()))
            && let Some(reexported) = reexports.get(&(target.module.clone(), sym.clone()))
            && let Some(resolved) = longest_module_prefix(reexported, "::", known_modules)
        {
            return Some(resolved);
        }
        return Some(target);
    }
    let first_segment = canonical.split("::").next().unwrap_or(canonical);
    if known_modules.contains(LIB_MODULE)
        && module_symbols
            .get(LIB_MODULE)
            .is_some_and(|set| set.contains(first_segment))
    {
        return Some(ResolvedModuleTarget {
            module: LIB_MODULE.to_owned(),
            symbol: Some(first_segment.to_owned()),
        });
    }
    None
}

fn longest_module_prefix(
    path: &str,
    separator: &str,
    known_modules: &HashSet<&str>,
) -> Option<ResolvedModuleTarget> {
    if known_modules.contains(path) {
        return Some(ResolvedModuleTarget {
            module: path.to_owned(),
            symbol: None,
        });
    }
    let mut current = path;
    while let Some((prefix, _)) = current.rsplit_once(separator) {
        if known_modules.contains(prefix) {
            let remainder = &path[prefix.len() + separator.len()..];
            let symbol = remainder
                .split(separator)
                .next()
                .filter(|seg| !seg.is_empty())
                .map(str::to_owned);
            return Some(ResolvedModuleTarget {
                module: prefix.to_owned(),
                symbol,
            });
        }
        current = prefix;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rust_project_facts_resolve_components_and_cross_module_symbols() {
        let files = [
            (
                "src/architecture.rs",
                indoc::indoc! {r"
                    architecture_component!(Architecture);
                    define_architecture! {
                        Architecture => [],
                        Core => [],
                        Runner => [Core, Architecture],
                    }
                "},
            ),
            (
                "src/core.rs",
                indoc::indoc! {r"
                    architecture_component!(Core);
                    mod detail;
                    pub use self::detail::Config;

                    pub fn parse_input() -> Config {
                        Config
                    }
                "},
            ),
            (
                "src/core/detail.rs",
                indoc::indoc! {r"
                    pub struct Config;
                "},
            ),
            (
                "src/runner.rs",
                indoc::indoc! {r"
                    architecture_component!(Runner);
                    use crate::core::{Config, parse_input};

                    pub fn execute() -> Config {
                        parse_input()
                    }
                "},
            ),
        ];

        let project = collect_project_facts_from_sources("sample_rs", Language::Rust, &files);

        assert!(project.has_declared_architecture);
        assert_eq!(project.components.len(), 3);
        assert!(project.cross_module_refs.iter().any(|reference| {
            reference.source_module == "runner"
                && reference.target_module == "core"
                && reference.target_symbol.as_deref() == Some("parse_input")
        }));
    }

    #[test]
    fn python_project_facts_resolve_package_and_relative_imports() {
        let files = [
            (
                "pkg/__init__.py",
                indoc::indoc! {r"
                    from ._core import build_plan
                "},
            ),
            (
                "pkg/_core.py",
                indoc::indoc! {r"
                    from . import _util

                    def build_plan() -> int:
                        return _util.compute_step()
                "},
            ),
            (
                "pkg/_util.py",
                indoc::indoc! {r"
                    def compute_step() -> int:
                        return 1
                "},
            ),
        ];

        let project = collect_project_facts_from_sources("sample_py", Language::Python, &files);

        assert!(!project.has_declared_architecture);
        assert_eq!(project.modules.len(), 3);
        assert!(project.cross_module_refs.iter().any(|reference| {
            reference.source_module == "pkg._core"
                && reference.target_module == "pkg._util"
                && reference.target_symbol.as_deref() == Some("compute_step")
        }));
    }
}
