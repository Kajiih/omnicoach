//! Language-agnostic multi-file project fact models and module-graph helpers (`CodeLintProjectFoundation`).

use super::architecture::ArchitectureSpec;
use crate::code_lint::ast::{IntraFileReference, ScopeFacts};
use crate::diagnostic::Language;
use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::PathBuf;

/// A project source file discovered via [`super::source::FileSource`], paired with its full query
/// path, normalized display path, and UTF-8 source text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::code_lint) struct ProjectSourceFile {
    pub(in crate::code_lint::project) full_path: PathBuf,
    /// Forward-slash-normalized display path relative to the project root.
    pub(in crate::code_lint) relative_path: String,
    /// Detected source language (`Rust` or `Python`).
    pub(in crate::code_lint) lang: Language,
    /// Full UTF-8 source text.
    pub(in crate::code_lint) source: String,
}

/// Multi-file project facts across a codebase.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::code_lint) struct ProjectFacts {
    /// Human-readable project or package name.
    pub(in crate::code_lint) title: String,
    /// True if architectural components and their allowed dependencies are explicitly declared
    /// (for example via `define_architecture!` + `architecture_component!` or `.omnicoach.toml`).
    pub(in crate::code_lint) has_declared_architecture: bool,
    /// Architectural components (declared or inferred from top-level directory / package structure).
    pub(in crate::code_lint) components: Vec<ComponentFact>,
    /// Parsed modules in deterministic path order.
    pub(in crate::code_lint) modules: Vec<ModuleFact>,
    /// Resolved cross-module reference edges (`source_module -> target_module`).
    pub(in crate::code_lint) cross_module_refs: Vec<CrossModuleRefFact>,
}

impl ProjectFacts {
    /// Assembles [`ProjectFacts`] for `modules`, filtering `cross_module_refs` to retained
    /// modules and finalizing declared and inferred [`ComponentFact`]s for `scope`.
    #[must_use]
    pub(in crate::code_lint::project) fn build(
        title: &str,
        spec: Option<&ArchitectureSpec>,
        modules: Vec<ModuleFact>,
        mut cross_module_refs: Vec<CrossModuleRefFact>,
        scope: ProjectGraphScope,
    ) -> Self {
        let retained_module_paths: HashSet<&str> = modules
            .iter()
            .map(|module| module.module_path.as_str())
            .collect();
        cross_module_refs.retain(|reference| {
            retained_module_paths.contains(reference.source_module.as_str())
                && retained_module_paths.contains(reference.target_module.as_str())
        });

        let mut components = Vec::new();
        let mut seen_components = BTreeSet::new();

        if let Some(resolved) = spec.filter(|resolved| !resolved.components.is_empty()) {
            let included_declared: HashSet<&str> = match scope {
                ProjectGraphScope::FullProject => resolved
                    .components
                    .iter()
                    .map(|component| component.name.as_str())
                    .collect(),
                ProjectGraphScope::Subset => modules
                    .iter()
                    .map(|module| module.component.as_str())
                    .collect(),
            };
            for component_spec in &resolved.components {
                if !included_declared.contains(component_spec.name.as_str()) {
                    continue;
                }
                let declared_dependencies = component_spec
                    .depends_on
                    .iter()
                    .filter(|dependency| included_declared.contains(dependency.as_str()))
                    .cloned()
                    .collect();
                let allowed_dependencies = resolved
                    .components
                    .iter()
                    .map(|candidate| candidate.name.as_str())
                    .filter(|&candidate| {
                        candidate != component_spec.name
                            && included_declared.contains(candidate)
                            && resolved.allows_dependency(&component_spec.name, candidate)
                    })
                    .map(str::to_owned)
                    .collect();
                seen_components.insert(component_spec.name.clone());
                components.push(ComponentFact {
                    name: component_spec.name.clone(),
                    declared_dependencies: Some(declared_dependencies),
                    allowed_dependencies,
                });
            }
        }

        let has_declared_architecture = !components.is_empty();
        for module in &modules {
            if seen_components.insert(module.component.clone()) {
                components.push(ComponentFact {
                    name: module.component.clone(),
                    declared_dependencies: None,
                    allowed_dependencies: Vec::new(),
                });
            }
        }

        Self {
            title: title.to_owned(),
            has_declared_architecture,
            components,
            modules,
            cross_module_refs,
        }
    }
}

/// An architectural component, its direct `depends_on` list (when declared), and its allowed
/// dependency targets within this graph.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::code_lint) struct ComponentFact {
    /// Component identifier (e.g. `"CodeLintAst"` or `"zoneinfo"`).
    pub(in crate::code_lint) name: String,
    /// Directly declared dependency component names present in this graph (`Some(...)` when this
    /// component is explicitly declared in architecture, `None` when inferred or unmapped).
    pub(in crate::code_lint) declared_dependencies: Option<Vec<String>>,
    /// Active component names in this graph to which [`ArchitectureSpec::allows_dependency`]
    /// permits references from `name` (honoring `transitive-dependencies` and elided subset
    /// intermediates).
    pub(in crate::code_lint) allowed_dependencies: Vec<String>,
}

/// One module file within [`ProjectFacts`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::code_lint) struct ModuleFact {
    /// Canonical module path (`"code_lint::ast"` or `"zoneinfo._tzpath"`).
    pub(in crate::code_lint) module_path: String,
    /// Relative source file path (`"src/code_lint/ast.rs"`).
    pub(in crate::code_lint) path: String,
    /// Owning component name.
    pub(in crate::code_lint) component: String,
    /// Per-scope facts inside this module (`scopes[0]` is the module scope).
    pub(in crate::code_lint) scopes: Vec<ScopeFacts>,
    /// Intra-file references between module declarations and methods in this module.
    pub(in crate::code_lint) intra_file_refs: Vec<IntraFileReference>,
}

/// A resolved reference from one module (and optional top-level item) to another module in the
/// same project.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(in crate::code_lint) struct CrossModuleRefFact {
    /// Referring module path.
    pub(in crate::code_lint) source_module: String,
    /// Referring top-level symbol in `source_module`, when the reference lies inside a top-level
    /// declaration.
    pub(in crate::code_lint) source_symbol: Option<String>,
    /// Target module path.
    pub(in crate::code_lint) target_module: String,
    /// Referenced top-level symbol in `target_module`, when resolved to a specific item.
    pub(in crate::code_lint) target_symbol: Option<String>,
    /// 1-based line number of the reference in `source_module`.
    pub(in crate::code_lint) line: usize,
}

/// Whether project-fact extraction covers a whole project/source root or a narrower file/directory subset.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::code_lint::project) enum ProjectGraphScope {
    /// Full project or source root: include all declared components from [`ArchitectureSpec`],
    /// preserving empty components and `Absent` Reflexion edges.
    FullProject,
    /// Subdirectory or explicit file subset: resolve each module's true declared component from
    /// [`ArchitectureSpec`] while retaining only components that own at least one collected module.
    Subset,
}

/// Canonical module and optional symbol (`"fn"`, `"Type"`, or `"Type::method"`) resolved for
/// a cross-module reference.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::code_lint::project) struct ResolvedModuleTarget {
    /// Canonical target module path within the project.
    pub(in crate::code_lint::project) module: String,
    /// Target top-level symbol or `Type::method` within `module`, when resolved below module level.
    pub(in crate::code_lint::project) symbol: Option<String>,
}

/// Module prefix, immediate symbol segment, and optional trailing path segments split out of a
/// separator-delimited path against a known module set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::code_lint::project) struct SplitModulePath {
    pub(in crate::code_lint::project) module: String,
    pub(in crate::code_lint::project) symbol: Option<String>,
    pub(in crate::code_lint::project) remainder: Option<String>,
}

/// Longest matching known module prefix of a separator-delimited path and its unconsumed suffix.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::code_lint::project) struct KnownModulePrefix<'a> {
    pub(in crate::code_lint::project) module: &'a str,
    pub(in crate::code_lint::project) remainder: &'a str,
}

#[must_use]
pub(in crate::code_lint::project) fn split_module_and_segments(
    path: &str,
    separator: &str,
    known_modules: &HashSet<&str>,
) -> Option<SplitModulePath> {
    if known_modules.contains(path) {
        return Some(SplitModulePath {
            module: path.to_owned(),
            symbol: None,
            remainder: None,
        });
    }
    let KnownModulePrefix { module, remainder } =
        longest_known_module_prefix(path, separator, known_modules)?;
    let mut segments = remainder
        .split(separator)
        .filter(|segment| !segment.is_empty());
    let symbol = segments.next().map(str::to_owned);
    let tail: Vec<&str> = segments.collect();
    let remainder = (!tail.is_empty()).then(|| tail.join(separator));
    Some(SplitModulePath {
        module: module.to_owned(),
        symbol,
        remainder,
    })
}

#[must_use]
pub(in crate::code_lint::project) fn longest_known_module_prefix<'a>(
    path: &'a str,
    separator: &str,
    known_modules: &HashSet<&str>,
) -> Option<KnownModulePrefix<'a>> {
    let mut current = path;
    while let Some((prefix, _)) = current.rsplit_once(separator) {
        if known_modules.contains(prefix) {
            let remainder = &path[prefix.len() + separator.len()..];
            return Some(KnownModulePrefix {
                module: prefix,
                remainder,
            });
        }
        current = prefix;
    }
    None
}

#[must_use]
pub(in crate::code_lint::project) fn resolve_unique_module_paths(
    files: &[&ProjectSourceFile],
    initial: impl Fn(&ProjectSourceFile) -> String,
    disambiguate: impl Fn(&ProjectSourceFile) -> String,
) -> Vec<String> {
    let mut paths: Vec<String> = files.iter().copied().map(&initial).collect();
    let mut counts: HashMap<String, usize> = HashMap::new();
    for module_path in &paths {
        *counts.entry(module_path.clone()).or_insert(0) += 1;
    }
    for (module_path, file) in paths.iter_mut().zip(files) {
        if counts.get(module_path).copied().unwrap_or(0) > 1 {
            *module_path = disambiguate(file);
        }
    }
    paths
}
