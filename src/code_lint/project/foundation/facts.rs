//! Language-agnostic multi-file project fact models and module-graph helpers.

use super::architecture::{ArchitectureSpec, SubmoduleSpec};
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
                let active_prefixes: HashSet<Vec<&str>> = modules
                    .iter()
                    .filter(|module| module.component == component_spec.name)
                    .flat_map(|module| {
                        (1..=module.submodule_path.len()).map(|depth| {
                            module.submodule_path[..depth]
                                .iter()
                                .map(String::as_str)
                                .collect::<Vec<&str>>()
                        })
                    })
                    .collect();
                let submodules =
                    build_submodule_facts(&component_spec.submodules, &active_prefixes, &[], scope);
                seen_components.insert(component_spec.name.clone());
                components.push(ComponentFact {
                    name: component_spec.name.clone(),
                    declared_dependencies: Some(declared_dependencies),
                    allowed_dependencies,
                    submodules,
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
                    submodules: Vec::new(),
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

fn build_submodule_facts(
    specs: &[SubmoduleSpec],
    active_prefixes: &HashSet<Vec<&str>>,
    parent_prefix: &[&str],
    scope: ProjectGraphScope,
) -> Vec<ComponentFact> {
    if specs.is_empty() {
        return Vec::new();
    }
    let included: HashSet<&str> = specs
        .iter()
        .filter(|sub| match scope {
            ProjectGraphScope::FullProject => true,
            ProjectGraphScope::Subset => {
                let mut candidate_prefix = parent_prefix.to_vec();
                candidate_prefix.push(sub.name.as_str());
                active_prefixes.contains(&candidate_prefix)
            }
        })
        .map(|sub| sub.name.as_str())
        .collect();

    specs
        .iter()
        .filter(|sub| included.contains(sub.name.as_str()))
        .map(|sub| {
            let mut next_prefix = parent_prefix.to_vec();
            next_prefix.push(sub.name.as_str());
            let declared_dependencies = sub
                .depends_on
                .iter()
                .filter(|dep| included.contains(dep.as_str()))
                .cloned()
                .collect();
            let allowed_dependencies = specs
                .iter()
                .map(|candidate| candidate.name.as_str())
                .filter(|&candidate| {
                    candidate != sub.name
                        && included.contains(candidate)
                        && sub.allows_sibling_dependency(candidate)
                })
                .map(str::to_owned)
                .collect();
            ComponentFact {
                name: sub.name.clone(),
                declared_dependencies: Some(declared_dependencies),
                allowed_dependencies,
                submodules: build_submodule_facts(
                    &sub.submodules,
                    active_prefixes,
                    &next_prefix,
                    scope,
                ),
            }
        })
        .collect()
}

/// An architectural component or recursive submodule container, its direct `depends_on` list
/// (when declared), and its allowed dependency targets within its parent scope.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::code_lint) struct ComponentFact {
    /// Component or submodule identifier (e.g. `"CodeLintAst"`, `"foundation"`, or `"zoneinfo"`).
    pub(in crate::code_lint) name: String,
    /// Directly declared dependency names present in this graph (`Some(...)` when explicitly
    /// declared in architecture, `None` when inferred or unmapped).
    pub(in crate::code_lint) declared_dependencies: Option<Vec<String>>,
    /// Active sibling names in this graph to which architecture permits references from `name`
    /// (honoring `transitive-dependencies` and elided subset intermediates).
    pub(in crate::code_lint) allowed_dependencies: Vec<String>,
    /// Recursive submodule DAG declared inside this component or submodule.
    pub(in crate::code_lint) submodules: Vec<Self>,
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
    /// Matched declared submodule segment chain inside `component` (e.g. `["python", "architecture"]`).
    pub(in crate::code_lint) submodule_path: Vec<String>,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::code_lint::project::foundation::architecture::build_architecture_spec;
    use crate::code_lint::project::foundation::config::ArchitectureConfig;

    #[test]
    fn project_facts_subset_filters_submodule_allowed_dependencies_to_collected_siblings() {
        let config: ArchitectureConfig = toml::from_str(indoc::indoc! {r#"
            [[components]]
            name = "Project"
            roots = ["proj"]
            submodules = [
              { name = "foundation" },
              { name = "python", depends-on = ["foundation"] },
              { name = "rust", depends-on = ["foundation"] },
            ]
        "#})
        .expect("valid config");
        let spec = build_architecture_spec(&config, false).expect("valid spec");
        let py_mod = ModuleFact {
            module_path: "proj::python".to_owned(),
            path: "src/proj/python.rs".to_owned(),
            component: "Project".to_owned(),
            submodule_path: vec!["python".to_owned()],
            scopes: Vec::new(),
            intra_file_refs: Vec::new(),
        };
        let rs_mod = ModuleFact {
            module_path: "proj::rust".to_owned(),
            path: "src/proj/rust.rs".to_owned(),
            component: "Project".to_owned(),
            submodule_path: vec!["rust".to_owned()],
            scopes: Vec::new(),
            intra_file_refs: Vec::new(),
        };

        let subset_facts = ProjectFacts::build(
            "subset",
            Some(&spec),
            vec![py_mod.clone(), rs_mod.clone()],
            Vec::new(),
            ProjectGraphScope::Subset,
        );
        let full_facts = ProjectFacts::build(
            "full",
            Some(&spec),
            vec![py_mod, rs_mod],
            Vec::new(),
            ProjectGraphScope::FullProject,
        );

        assert_eq!(
            subset_facts.components[0].submodules[0].allowed_dependencies,
            Vec::<String>::new()
        );
        assert_eq!(
            full_facts.components[0].submodules[1].allowed_dependencies,
            vec!["foundation".to_owned()]
        );
    }
}
