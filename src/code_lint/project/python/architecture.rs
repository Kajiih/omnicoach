//! Python architectural conformance evaluation (`CodeLintProjectPython`).

use super::layout::PythonLayout;
use super::paths::{
    INIT_STEM, is_in_python_module_subtree, is_python_package_init, resolve_relative_python_package,
};
use crate::code_lint::ast::ParsedFile;
use crate::code_lint::ast::python::{
    PythonImportReference, collect_python_import_references, summarize_python_file,
};
use crate::code_lint::project::foundation::{
    ArchitectureSpec, CrossComponentRelativePathFinding, ExportPattern, FileArchitectureFindings,
    FileSource, PrivateComponentImportFinding, ROOT_MODULE_TAG, SiblingModuleDependencyFinding,
    UndeclaredComponentDependencyFinding, UnmappedArchitectureModuleFinding, deduplicate_findings,
};
use crate::diagnostic::SourceSpan;
use std::path::Path;

const PYTHON_MODULE_SEPARATOR: &str = ".";
const PREFIX_MATCH_RANK: usize = 3;

/// Matched Python component and the specific root pattern that matched.
#[derive(Debug, Clone, Copy)]
pub(super) struct MatchedPythonComponent<'a> {
    pub(super) component_name: &'a str,
    pub(super) matched_root: &'a str,
}

/// Evaluates Python architectural conformance findings for `file` (`path`).
#[must_use]
pub(super) fn evaluate_python_file_architecture(
    file: &ParsedFile,
    path: &Path,
    layout: &PythonLayout,
    source: &dyn FileSource,
    spec: &ArchitectureSpec,
) -> FileArchitectureFindings {
    if spec.from_macro_declarations {
        return FileArchitectureFindings::default();
    }
    let Some(module_path) = layout.architecture_module_of_path(path) else {
        return FileArchitectureFindings::default();
    };
    let mut findings = FileArchitectureFindings::default();

    let enclosing = resolve_python_component(&module_path, spec);
    if spec.require_component_coverage && enclosing.is_none() {
        collect_python_unmapped_findings(
            file,
            path,
            &module_path,
            spec,
            &mut findings.unmapped_architecture_modules,
        );
    }

    let is_package_init = is_python_package_init(path);
    for import_ref in collect_python_import_references(file) {
        if import_ref.in_type_checking && spec.ignore_type_checking_imports {
            continue;
        }
        let resolved_imports = resolve_python_import_targets_with_layout(
            &module_path,
            is_package_init,
            &import_ref,
            layout,
            source,
        );
        for resolved in resolved_imports.targets {
            if let Some(ref enclosing_match) = enclosing
                && let Some(ref ancestor) = resolved_imports.shallowest_relative_ancestor
                && !resolved.stays_within_root(ancestor, enclosing_match.matched_root)
            {
                let dependency = if resolved.module_path == ROOT_MODULE_TAG {
                    resolved.full_target_path.clone()
                } else {
                    resolved.module_path.clone()
                };
                findings
                    .cross_component_relative_paths
                    .push(CrossComponentRelativePathFinding {
                        span: import_ref.span,
                        dependency,
                        component: enclosing_match.component_name.to_owned(),
                    });
            }

            if !is_first_party_python_import(&resolved.module_path, layout, source, spec) {
                if import_ref.level == 0 {
                    spec.collect_unowned_external_dependencies(
                        &module_path,
                        enclosing.as_ref().map(|matched| matched.component_name),
                        import_ref.span,
                        |package_name| {
                            is_in_python_module_subtree(&resolved.full_target_path, package_name)
                        },
                        &mut findings.unowned_external_dependencies,
                    );
                }
                continue;
            }

            let Some(ref from_match) = enclosing else {
                continue;
            };
            check_python_first_party_edge(
                &module_path,
                from_match,
                &resolved,
                import_ref.span,
                spec,
                &mut findings,
            );
        }
    }

    deduplicate_findings(&mut findings);
    findings
}

/// Resolves the most-specific owning Python component of a dotted `module_path`, if mapped by `spec`.
#[must_use]
pub(super) fn resolve_python_component<'a>(
    module_path: &str,
    spec: &'a ArchitectureSpec,
) -> Option<MatchedPythonComponent<'a>> {
    let mut best: Option<(SpecificityRank, MatchedPythonComponent<'a>)> = None;
    for component in &spec.components {
        for root in &component.roots {
            if let Some(rank) = python_root_specificity(module_path, root) {
                let candidate = MatchedPythonComponent {
                    component_name: component.name.as_str(),
                    matched_root: root.as_str(),
                };
                if best.as_ref().is_none_or(|(best_rank, _)| rank > *best_rank) {
                    best = Some((rank, candidate));
                }
            }
        }
    }
    best.map(|(_, matched)| matched)
}

/// Returns true if `module_path` is a root initializer (`"<root>"` or `"__init__"`) or a
/// package ancestor of any declared Python component root.
#[must_use]
pub(super) fn routes_to_python_component(module_path: &str, spec: &ArchitectureSpec) -> bool {
    if spec.from_macro_declarations || spec.components.is_empty() {
        return false;
    }
    module_path == ROOT_MODULE_TAG
        || module_path == INIT_STEM
        || spec.components.iter().any(|component| {
            component
                .roots
                .iter()
                .any(|root| is_in_python_module_subtree(root, module_path))
        })
}

#[derive(Debug, Clone)]
struct ResolvedPythonImports {
    targets: Vec<ResolvedPythonImport>,
    shallowest_relative_ancestor: Option<String>,
}

#[derive(Debug, Clone)]
struct ResolvedPythonImport {
    module_path: String,
    full_target_path: String,
    imported_member: Option<String>,
}

impl ResolvedPythonImport {
    fn stays_within_root(&self, ancestor: &str, component_root: &str) -> bool {
        if component_root == ROOT_MODULE_TAG || component_root == "*" {
            return true;
        }
        let target_in_root = python_root_specificity(&self.module_path, component_root).is_some();
        let floor_base = python_root_base(component_root);
        let floor_in_root = python_root_specificity(ancestor, component_root).is_some()
            || (!component_root.ends_with(".*")
                && is_in_python_module_subtree(ancestor, floor_base));
        target_in_root && floor_in_root
    }
}

fn check_python_first_party_edge(
    module_path: &str,
    from_match: &MatchedPythonComponent<'_>,
    resolved: &ResolvedPythonImport,
    span: SourceSpan,
    spec: &ArchitectureSpec,
    findings: &mut FileArchitectureFindings,
) {
    let Some(target_match) = resolve_python_component(&resolved.module_path, spec) else {
        if spec.require_component_coverage {
            findings
                .undeclared_component_dependencies
                .push(UndeclaredComponentDependencyFinding {
                    span,
                    component: from_match.component_name.to_owned(),
                    dependency: resolved.full_target_path.clone(),
                });
        }
        return;
    };

    if from_match.component_name == target_match.component_name {
        if let Some(comp_spec) = spec.component_spec(from_match.component_name)
            && comp_spec.isolate_children
            && let Some(sibling_target) = python_sibling_violation(
                module_path,
                from_match.matched_root,
                &resolved.module_path,
                target_match.matched_root,
            )
        {
            findings
                .sibling_module_dependencies
                .push(SiblingModuleDependencyFinding {
                    span,
                    module: module_path.to_owned(),
                    dependency: sibling_target,
                    component: from_match.component_name.to_owned(),
                });
        }
        return;
    }

    if !spec.allows_dependency(from_match.component_name, target_match.component_name) {
        findings
            .undeclared_component_dependencies
            .push(UndeclaredComponentDependencyFinding {
                span,
                component: from_match.component_name.to_owned(),
                dependency: target_match.component_name.to_owned(),
            });
    } else if let Some(target_spec) = spec.component_spec(target_match.component_name)
        && let Some(ref exports) = target_spec.public_exports
    {
        let concrete_root = concrete_python_target_root(target_match.matched_root, resolved);
        if !is_python_import_allowed_by_exports(&concrete_root, resolved, exports) {
            let symbol_name = if resolved.imported_member.as_deref() == Some("*")
                || (resolved.module_path == concrete_root && resolved.imported_member.is_none())
            {
                None
            } else {
                Some(resolved.imported_member.clone().unwrap_or_else(|| {
                    resolved
                        .full_target_path
                        .rsplit(PYTHON_MODULE_SEPARATOR)
                        .next()
                        .unwrap_or(&resolved.full_target_path)
                        .to_owned()
                }))
            };
            findings
                .private_component_imports
                .push(PrivateComponentImportFinding {
                    span,
                    component: from_match.component_name.to_owned(),
                    name: symbol_name,
                    module: concrete_root,
                    dependency: target_match.component_name.to_owned(),
                });
        }
    }
}

fn python_sibling_violation(
    source_module: &str,
    source_root: &str,
    target_module: &str,
    target_root: &str,
) -> Option<String> {
    let source_base = python_root_base(source_root);
    let target_base = python_root_base(target_root);
    if source_base != target_base {
        if target_base == "*" || target_base == ROOT_MODULE_TAG {
            return (target_module != ROOT_MODULE_TAG).then(|| target_module.to_owned());
        }
        return Some(target_base.to_owned());
    }
    let source_child = direct_child_in_python_root(source_module, source_root)?;
    let target_child = direct_child_in_python_root(target_module, target_root)?;
    (source_child != target_child).then_some(target_child)
}

fn concrete_python_target_root(matched_root: &str, resolved: &ResolvedPythonImport) -> String {
    if matched_root == ROOT_MODULE_TAG || matched_root.contains('*') {
        direct_child_in_python_root(&resolved.module_path, matched_root)
            .unwrap_or_else(|| PYTHON_MODULE_SEPARATOR.to_owned())
    } else {
        matched_root.to_owned()
    }
}

fn resolve_python_import_targets_with_layout(
    enclosing_module: &str,
    is_package_init: bool,
    import_ref: &PythonImportReference,
    layout: &PythonLayout,
    source: &dyn FileSource,
) -> ResolvedPythonImports {
    if import_ref.level == 0 {
        let Some(base_module) = import_ref.module.as_deref() else {
            return ResolvedPythonImports {
                targets: Vec::new(),
                shallowest_relative_ancestor: None,
            };
        };
        return ResolvedPythonImports {
            targets: resolve_imported_members_on_base(
                base_module.to_owned(),
                &import_ref.imported_names,
                layout,
                source,
            ),
            shallowest_relative_ancestor: None,
        };
    }

    let Some(resolved_package) =
        resolve_relative_python_package(enclosing_module, is_package_init, import_ref.level)
    else {
        return ResolvedPythonImports {
            targets: Vec::new(),
            shallowest_relative_ancestor: None,
        };
    };
    let shallowest = Some(resolved_package.shallowest_ancestor);
    let Some(base_module) = (match (
        resolved_package.package_prefix,
        import_ref.module.as_deref(),
    ) {
        (Some(prefix), Some(suffix)) => Some(format!("{prefix}.{suffix}")),
        (Some(prefix), None) => Some(prefix),
        (None, Some(suffix)) => Some(suffix.to_owned()),
        (None, None) => None,
    }) else {
        let targets = import_ref
            .imported_names
            .iter()
            .map(|member| {
                if layout.module_file(member, source).is_some()
                    || layout.is_namespace_dir(member, source)
                {
                    ResolvedPythonImport {
                        module_path: member.clone(),
                        full_target_path: member.clone(),
                        imported_member: None,
                    }
                } else {
                    ResolvedPythonImport {
                        module_path: ROOT_MODULE_TAG.to_owned(),
                        full_target_path: member.clone(),
                        imported_member: Some(member.clone()),
                    }
                }
            })
            .collect();
        return ResolvedPythonImports {
            targets,
            shallowest_relative_ancestor: shallowest,
        };
    };

    ResolvedPythonImports {
        targets: resolve_imported_members_on_base(
            base_module,
            &import_ref.imported_names,
            layout,
            source,
        ),
        shallowest_relative_ancestor: shallowest,
    }
}

fn direct_child_in_python_root(module_path: &str, root: &str) -> Option<String> {
    if module_path == ROOT_MODULE_TAG {
        return None;
    }
    let base = python_root_base(root);
    if base == "*" || base == ROOT_MODULE_TAG {
        let child = module_path.split(PYTHON_MODULE_SEPARATOR).next()?;
        return (!child.is_empty()).then(|| child.to_owned());
    }
    let rest = module_path
        .strip_prefix(base)?
        .strip_prefix(PYTHON_MODULE_SEPARATOR)?;
    let child = rest.split(PYTHON_MODULE_SEPARATOR).next()?;
    (!child.is_empty()).then(|| format!("{base}.{child}"))
}

fn is_python_import_allowed_by_exports(
    target_root: &str,
    resolved: &ResolvedPythonImport,
    exports: &[ExportPattern],
) -> bool {
    if resolved.module_path == target_root {
        return match resolved.imported_member.as_deref() {
            None | Some("*") => exports
                .iter()
                .any(|export_pattern| export_pattern.matches("*")),
            Some(member) => exports
                .iter()
                .any(|export_pattern| export_pattern.matches(member)),
        };
    }
    let Some(subpath) = resolved
        .full_target_path
        .strip_prefix(target_root)
        .and_then(|rest| rest.strip_prefix(PYTHON_MODULE_SEPARATOR))
    else {
        return false;
    };
    if resolved.imported_member.as_deref() == Some("*") {
        let star_probe = format!("{subpath}.*");
        return exports
            .iter()
            .any(|export_pattern| export_pattern.matches(&star_probe));
    }
    exports
        .iter()
        .any(|export_pattern| export_pattern.matches(subpath))
}

fn resolve_imported_members_on_base(
    base_module: String,
    imported_names: &[String],
    layout: &PythonLayout,
    source: &dyn FileSource,
) -> Vec<ResolvedPythonImport> {
    if imported_names.is_empty() {
        return vec![resolve_imported_member_on_base(
            base_module,
            None,
            layout,
            source,
        )];
    }
    imported_names
        .iter()
        .map(|imported_name| {
            resolve_imported_member_on_base(
                base_module.clone(),
                Some(imported_name.as_str()),
                layout,
                source,
            )
        })
        .collect()
}

fn python_root_base(root: &str) -> &str {
    root.strip_suffix(".**")
        .or_else(|| root.strip_suffix(".*"))
        .unwrap_or(root)
}

fn resolve_imported_member_on_base(
    base_module: String,
    imported_name: Option<&str>,
    layout: &PythonLayout,
    source: &dyn FileSource,
) -> ResolvedPythonImport {
    if imported_name == Some("*") {
        return ResolvedPythonImport {
            module_path: base_module.clone(),
            full_target_path: base_module,
            imported_member: Some("*".to_owned()),
        };
    }
    if let Some(member) = imported_name {
        let candidate_submodule = format!("{base_module}.{member}");
        if layout.module_file(&candidate_submodule, source).is_some()
            || layout.is_namespace_dir(&candidate_submodule, source)
        {
            return ResolvedPythonImport {
                module_path: candidate_submodule.clone(),
                full_target_path: candidate_submodule,
                imported_member: None,
            };
        }
        return ResolvedPythonImport {
            module_path: base_module,
            full_target_path: candidate_submodule,
            imported_member: Some(member.to_owned()),
        };
    }
    ResolvedPythonImport {
        module_path: base_module.clone(),
        full_target_path: base_module,
        imported_member: None,
    }
}

fn is_first_party_python_import(
    resolved_module_path: &str,
    layout: &PythonLayout,
    source: &dyn FileSource,
    spec: &ArchitectureSpec,
) -> bool {
    let top_package = resolved_module_path
        .split(PYTHON_MODULE_SEPARATOR)
        .next()
        .unwrap_or(resolved_module_path);
    layout.module_file(top_package, source).is_some()
        || layout.is_namespace_dir(top_package, source)
        || spec.components.iter().any(|comp| {
            comp.roots
                .iter()
                .any(|root| root.split(PYTHON_MODULE_SEPARATOR).next() == Some(top_package))
        })
}

fn collect_python_unmapped_findings(
    file: &ParsedFile,
    path: &Path,
    module_path: &str,
    spec: &ArchitectureSpec,
    out: &mut Vec<UnmappedArchitectureModuleFinding>,
) {
    let py_summary = summarize_python_file(file);
    if py_summary.non_routing_items.is_empty() {
        let is_pure_package_router =
            is_python_package_init(path) && routes_to_python_component(module_path, spec);
        if !is_pure_package_router {
            out.push(UnmappedArchitectureModuleFinding {
                span: file.first_line_span(),
                name: module_path.to_owned(),
                module: module_path.to_owned(),
            });
        }
    } else {
        for item in &py_summary.non_routing_items {
            out.push(UnmappedArchitectureModuleFinding {
                span: item.span,
                name: item.name.clone(),
                module: module_path.to_owned(),
            });
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct SpecificityRank {
    kind_rank: usize,
    segment_count: usize,
}

fn python_root_specificity(module_path: &str, root_pattern: &str) -> Option<SpecificityRank> {
    if root_pattern == ROOT_MODULE_TAG {
        return (module_path == ROOT_MODULE_TAG || !module_path.contains(PYTHON_MODULE_SEPARATOR))
            .then_some(SpecificityRank {
                kind_rank: PREFIX_MATCH_RANK,
                segment_count: 1,
            });
    }
    if root_pattern == "*" {
        return (!module_path.contains(PYTHON_MODULE_SEPARATOR)).then_some(SpecificityRank {
            kind_rank: 1,
            segment_count: 1,
        });
    }
    if let Some(prefix) = root_pattern.strip_suffix(".**") {
        let segment_count = prefix.split(PYTHON_MODULE_SEPARATOR).count();
        return (module_path != prefix && is_in_python_module_subtree(module_path, prefix))
            .then_some(SpecificityRank {
                kind_rank: 2,
                segment_count,
            });
    }
    if let Some(prefix) = root_pattern.strip_suffix(".*") {
        let segment_count = prefix.split(PYTHON_MODULE_SEPARATOR).count();
        let rest = module_path
            .strip_prefix(prefix)?
            .strip_prefix(PYTHON_MODULE_SEPARATOR)?;
        return (!rest.contains(PYTHON_MODULE_SEPARATOR)).then_some(SpecificityRank {
            kind_rank: 2,
            segment_count: segment_count + 1,
        });
    }
    if is_in_python_module_subtree(module_path, root_pattern) {
        let segment_count = root_pattern.split(PYTHON_MODULE_SEPARATOR).count();
        let exact_bonus = usize::from(module_path == root_pattern);
        return Some(SpecificityRank {
            kind_rank: PREFIX_MATCH_RANK + exact_bonus,
            segment_count,
        });
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::code_lint::project::foundation::{
        ArchitectureConfig, MemoryFileSource, build_architecture_spec,
    };
    use crate::diagnostic::Language;

    #[test]
    fn wildcard_and_rootless_components_format_owners_and_concrete_export_roots() {
        let root = Path::new("/repo");
        let api_source = indoc::indoc! {"
            import httpx
            import redis
            from pkg.plugins.loader import load
            from pkg.plugins.loader.internal import raw
        "};
        let source = MemoryFileSource::from_relative_files(
            root,
            &[
                ("pkg/__init__.py", ""),
                ("pkg/plugins/__init__.py", ""),
                (
                    "pkg/plugins/loader/__init__.py",
                    "def load() -> None: pass\n",
                ),
                (
                    "pkg/plugins/loader/internal.py",
                    "def raw() -> None: pass\n",
                ),
                ("pkg/api.py", api_source),
            ],
        );
        let config: ArchitectureConfig = toml::from_str(indoc::indoc! {r#"
            [[components]]
            name = "RootlessHttp"
            roots = []
            owns-external = ["httpx"]

            [[components]]
            name = "WildcardCache"
            roots = ["pkg.cache.*"]
            owns-external = ["redis"]

            [[components]]
            name = "Plugins"
            roots = ["pkg.plugins.**"]
            public-exports = ["load"]

            [[components]]
            name = "Api"
            roots = ["pkg.api"]
            depends-on = ["RootlessHttp", "WildcardCache", "Plugins"]
        "#})
        .unwrap();
        let spec = build_architecture_spec(&config, false).unwrap();
        let layout = PythonLayout::discover(
            crate::code_lint::project::foundation::WorkspaceAnchor::new(root.to_path_buf(), None),
            root,
            None,
            &source,
            &[],
        );
        let parsed = ParsedFile::new(api_source, Language::Python);
        let findings = evaluate_python_file_architecture(
            &parsed,
            &root.join("pkg/api.py"),
            &layout,
            &source,
            &spec,
        );

        assert_eq!(
            findings.unowned_external_dependencies[0].owners,
            vec!["RootlessHttp".to_owned()]
        );
        assert_eq!(
            findings.unowned_external_dependencies[1].owners,
            vec!["WildcardCache".to_owned()]
        );
        assert_eq!(findings.private_component_imports.len(), 1);
        assert_eq!(
            findings.private_component_imports[0].module,
            "pkg.plugins.loader"
        );
    }
}
