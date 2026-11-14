//! Rust architectural conformance evaluation and `define_architecture!` / `architecture_component!`
//! synthesis.

use super::layout::RustLayout;
use super::paths::{
    PathOrigin, is_in_rust_module_subtree, is_rust_crate_root,
    resolve_inherited_component_with_root, resolve_rust_reference_path,
};
use crate::code_lint::ast::ParsedFile;
use crate::code_lint::ast::rust::{DefinedComponentEntry, RustFileSummary, summarize_rust_file};
use crate::code_lint::project::foundation::{
    ArchitectureComponentConfig, ArchitectureConfig, ArchitectureSpec, ArchitectureSubmoduleConfig,
    ComponentSpec, CrossComponentRelativePathFinding, ExportPattern, FileArchitectureFindings,
    InvalidArchitectureComponentFinding, InvalidArchitectureComponentKind,
    PrivateComponentImportFinding, ReexportedPublicItemFinding, SiblingModuleDependencyFinding,
    SubmoduleSpec, UndeclaredComponentDependencyFinding, UnmappedArchitectureModuleFinding,
    deduplicate_findings,
};
use crate::diagnostic::SourceSpan;
use std::collections::{BTreeMap, HashSet};
use std::path::Path;
use std::sync::Arc;

/// Canonical Rust module name (`src/architecture.rs` or `src/architecture/mod.rs`) where
/// `define_architecture!` is recognized.
pub(super) const RUST_ARCHITECTURE_MODULE: &str = "architecture";

/// Maximum number of `pub use` re-export hops resolved across module headers.
pub(super) const MAX_REEXPORT_HOPS: usize = 8;

const RUST_MODULE_SEPARATOR: &str = "::";

/// Precomputed Rust sibling-module isolation rule for a multi-root component or `pub mod` root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct SiblingIsolationRule {
    component_name: String,
    subject_prefix: String,
    forbidden_siblings: Vec<String>,
}

/// Evaluates Rust architectural conformance findings for `file` (`path`).
#[must_use]
pub(super) fn evaluate_rust_file_architecture(
    file: &ParsedFile,
    path: &Path,
    layout: &RustLayout,
    spec: &ArchitectureSpec,
    sibling_rules: &[SiblingIsolationRule],
    resolve_reexport: impl Fn(&str, &str) -> Option<String>,
) -> FileArchitectureFindings {
    let Some(module_path) = layout.module_of_path(path) else {
        return FileArchitectureFindings::default();
    };
    let summary = summarize_rust_file(file);
    let mut findings = FileArchitectureFindings {
        reexported_public_items: collect_rust_reexported_public_items(&module_path, &summary),
        ..FileArchitectureFindings::default()
    };
    if spec.components.is_empty() && !spec.from_macro_declarations {
        deduplicate_findings(&mut findings);
        return findings;
    }

    let enclosing = resolve_inherited_component_with_root(&module_path, spec.component_roots());
    let has_invalid_macro = check_rust_invalid_component_markers(
        &module_path,
        &summary,
        spec,
        &mut findings.invalid_architecture_components,
    );
    if !(spec.from_macro_declarations && has_invalid_macro && enclosing.is_none()) {
        check_rust_file_coverage_and_routers(
            file,
            &module_path,
            &summary,
            spec,
            &mut findings.unmapped_architecture_modules,
        );
    }

    for reference in &summary.referenced_paths {
        let resolved = resolve_rust_reference_path(
            &module_path,
            &summary.external_mods,
            &reference.raw_path,
            layout.crate_name.as_deref(),
        );

        if let PathOrigin::Relative {
            shallowest_ancestor,
        } = &resolved.origin
            && let Some(ref inherited) = enclosing
            && !shallowest_ancestor.as_deref().is_some_and(|ancestor| {
                is_in_rust_module_subtree(ancestor, inherited.root_module)
                    && is_in_rust_module_subtree(&resolved.canonical_path, inherited.root_module)
            })
        {
            findings
                .cross_component_relative_paths
                .push(CrossComponentRelativePathFinding {
                    span: reference.span,
                    dependency: resolved.canonical_path.clone(),
                    component: inherited.component.clone(),
                });
        }

        if !resolved.is_crate_local() {
            spec.collect_unowned_external_dependencies(
                &module_path,
                enclosing
                    .as_ref()
                    .map(|inherited| inherited.component.as_str()),
                reference.span,
                |package_name| is_in_rust_module_subtree(&resolved.canonical_path, package_name),
                &mut findings.unowned_external_dependencies,
            );
        }

        if let Some(ref from_component) = enclosing
            && let Some(target_component) = resolve_inherited_component_with_root(
                &resolved.canonical_path,
                spec.component_roots(),
            )
            && from_component.component != target_component.component
        {
            check_rust_cross_component_edge(
                from_component.component,
                target_component.component,
                target_component.root_module,
                &resolved.canonical_path,
                reference.is_glob,
                reference.span,
                spec,
                &mut findings,
            );
        }

        let sibling_target_path = resolve_intra_component_reexports(
            &resolved.canonical_path,
            enclosing.as_ref().map(|inherited| inherited.root_module),
            &resolve_reexport,
        );
        check_rust_sibling_isolation(
            &module_path,
            &sibling_target_path,
            reference.span,
            sibling_rules,
            &mut findings.sibling_module_dependencies,
        );
    }

    deduplicate_findings(&mut findings);
    findings
}

/// Evaluates spec-independent Rust architectural findings (stray `architecture_component!(...)`
/// macro calls and non-root `pub use` re-exports) in `file` (`path`) when the project declares no
/// [`ArchitectureSpec`].
#[must_use]
pub(super) fn evaluate_stray_rust_component_markers(
    file: &ParsedFile,
    path: &Path,
    layout: &RustLayout,
) -> FileArchitectureFindings {
    let Some(module_path) = layout.module_of_path(path) else {
        return FileArchitectureFindings::default();
    };
    let summary = summarize_rust_file(file);
    let reexported_public_items = collect_rust_reexported_public_items(&module_path, &summary);
    let mut findings = FileArchitectureFindings {
        invalid_architecture_components: summary
            .architecture_components
            .into_iter()
            .map(|(span, component)| InvalidArchitectureComponentFinding {
                span,
                component,
                module: module_path.clone(),
                kind: InvalidArchitectureComponentKind::MissingDefineArchitecture,
            })
            .collect(),
        reexported_public_items,
        ..FileArchitectureFindings::default()
    };
    deduplicate_findings(&mut findings);
    findings
}

/// Synthesizes the component DAG of a crate that declares `define_architecture!` in its
/// `architecture` module: components and dependencies from `define_architecture!`, roots from the
/// modules declaring `architecture_component!(...)`.
#[must_use]
pub(super) fn architecture_config_from_rust_headers(
    rust_headers: &BTreeMap<String, Arc<RustFileSummary>>,
) -> ArchitectureConfig {
    let defined_entries: &[DefinedComponentEntry] = rust_headers
        .get(RUST_ARCHITECTURE_MODULE)
        .map_or(&[], |summary| summary.defined_architecture.as_slice());
    let defined_names: HashSet<&str> = defined_entries
        .iter()
        .map(|entry| entry.name.as_str())
        .collect();

    let mut accepted_roots: BTreeMap<String, String> = BTreeMap::new();
    let mut roots_by_component: BTreeMap<String, Vec<String>> = BTreeMap::new();

    for (module_path, summary) in rust_headers {
        let Some((_, component_name)) = summary
            .architecture_components
            .iter()
            .find(|(span, _)| !summary.nested_architecture_components.contains(span))
        else {
            continue;
        };
        let has_ancestor_root = module_path
            .rsplit_once(RUST_MODULE_SEPARATOR)
            .and_then(|(parent, _)| resolve_inherited_component_with_root(parent, &accepted_roots))
            .is_some();
        if !defined_names.contains(component_name.as_str()) || has_ancestor_root {
            continue;
        }
        accepted_roots.insert(module_path.clone(), component_name.clone());
        roots_by_component
            .entry(component_name.clone())
            .or_default()
            .push(module_path.clone());
    }

    let components = defined_entries
        .iter()
        .map(|entry| {
            let roots = roots_by_component.remove(&entry.name).unwrap_or_default();
            ArchitectureComponentConfig {
                name: entry.name.clone(),
                roots,
                depends_on: entry.depends_on.clone(),
                isolate_children: true,
                owns_external: entry.owns_external.clone(),
                public_exports: None,
                submodules: submodule_configs_from_defined(&entry.submodules),
            }
        })
        .collect();

    ArchitectureConfig {
        source_roots: vec!["src".to_owned()],
        transitive_dependencies: true,
        require_component_coverage: true,
        ignore_type_checking_imports: false,
        components,
    }
}

fn submodule_configs_from_defined(
    entries: &[DefinedComponentEntry],
) -> Vec<ArchitectureSubmoduleConfig> {
    entries
        .iter()
        .map(|entry| ArchitectureSubmoduleConfig {
            name: entry.name.clone(),
            depends_on: entry.depends_on.clone(),
            submodules: submodule_configs_from_defined(&entry.submodules),
        })
        .collect()
}

/// Builds Rust sibling-isolation rules across multi-root components, declared intra-component
/// `submodules` DAGs (including private `mod` children at all depths), and public `pub mod`
/// children of component roots without an explicit `submodules` DAG.
#[must_use]
pub(super) fn build_rust_sibling_isolation_rules(
    components: &[ComponentSpec],
    mut summary_of: impl FnMut(&str) -> Option<Arc<RustFileSummary>>,
) -> Vec<SiblingIsolationRule> {
    let mut rules = Vec::new();
    for component in components {
        if !component.isolate_children && component.submodules.is_empty() {
            continue;
        }
        if component.isolate_children && component.roots.len() > 1 {
            for root in &component.roots {
                let forbidden_siblings: Vec<String> = component
                    .roots
                    .iter()
                    .filter(|other| *other != root)
                    .cloned()
                    .collect();
                rules.push(SiblingIsolationRule {
                    component_name: component.name.clone(),
                    subject_prefix: root.clone(),
                    forbidden_siblings,
                });
            }
        }
        for root in &component.roots {
            if !component.submodules.is_empty() {
                append_declared_submodule_isolation_rules(
                    &component.name,
                    root,
                    &component.submodules,
                    &mut rules,
                );
            } else if let Some(root_summary) = summary_of(root) {
                let direct_children: Vec<String> = root_summary
                    .external_mods
                    .iter()
                    .filter(|declaration| !declaration.is_private)
                    .map(|declaration| format!("{root}::{}", declaration.name))
                    .collect();
                if direct_children.len() > 1 {
                    for child in &direct_children {
                        let forbidden_siblings: Vec<String> = direct_children
                            .iter()
                            .filter(|other| *other != child)
                            .cloned()
                            .collect();
                        rules.push(SiblingIsolationRule {
                            component_name: component.name.clone(),
                            subject_prefix: child.clone(),
                            forbidden_siblings,
                        });
                    }
                }
            }
        }
    }
    rules
}

fn append_declared_submodule_isolation_rules(
    component_name: &str,
    parent_prefix: &str,
    declared_submodules: &[SubmoduleSpec],
    rules: &mut Vec<SiblingIsolationRule>,
) {
    for sub in declared_submodules {
        let child_prefix = format!("{parent_prefix}::{}", sub.name);
        let forbidden_siblings: Vec<String> = declared_submodules
            .iter()
            .filter(|other| {
                other.name != sub.name && !sub.allows_sibling_dependency(other.name.as_str())
            })
            .map(|other| format!("{parent_prefix}::{}", other.name))
            .collect();
        if !forbidden_siblings.is_empty() {
            rules.push(SiblingIsolationRule {
                component_name: component_name.to_owned(),
                subject_prefix: child_prefix.clone(),
                forbidden_siblings,
            });
        }
        if !sub.submodules.is_empty() {
            append_declared_submodule_isolation_rules(
                component_name,
                &child_prefix,
                &sub.submodules,
                rules,
            );
        }
    }
}

#[expect(
    clippy::too_many_arguments,
    reason = "internal helper threading source/target component metadata and glob flag"
)]
fn check_rust_cross_component_edge(
    from_component: &str,
    target_component: &str,
    target_root_module: &str,
    canonical_path: &str,
    is_glob: bool,
    span: SourceSpan,
    spec: &ArchitectureSpec,
    findings: &mut FileArchitectureFindings,
) {
    if !spec.allows_dependency(from_component, target_component) {
        findings
            .undeclared_component_dependencies
            .push(UndeclaredComponentDependencyFinding {
                span,
                component: from_component.to_owned(),
                dependency: target_component.to_owned(),
            });
    } else if let Some(target_spec) = spec.component_spec(target_component)
        && let Some(ref exports) = target_spec.public_exports
        && !is_rust_path_allowed_by_exports(target_root_module, canonical_path, is_glob, exports)
    {
        let symbol_name = if is_glob || canonical_path == target_root_module {
            None
        } else {
            Some(
                canonical_path
                    .rsplit(RUST_MODULE_SEPARATOR)
                    .next()
                    .unwrap_or(canonical_path)
                    .to_owned(),
            )
        };
        findings
            .private_component_imports
            .push(PrivateComponentImportFinding {
                span,
                component: from_component.to_owned(),
                name: symbol_name,
                module: target_root_module.to_owned(),
                dependency: target_component.to_owned(),
            });
    }
}

fn is_rust_path_allowed_by_exports(
    target_root: &str,
    canonical_path: &str,
    is_glob: bool,
    exports: &[ExportPattern],
) -> bool {
    if canonical_path == target_root {
        return exports.iter().any(|export| export.matches("*"));
    }
    let Some(relative_symbol) = canonical_path
        .strip_prefix(target_root)
        .and_then(|rest| rest.strip_prefix(RUST_MODULE_SEPARATOR))
    else {
        return false;
    };
    if is_glob {
        let glob_probe = format!("{relative_symbol}::*");
        return exports.iter().any(|export| export.matches(&glob_probe));
    }
    exports.iter().any(|export| export.matches(relative_symbol))
}

fn check_rust_invalid_component_markers(
    module_path: &str,
    summary: &RustFileSummary,
    spec: &ArchitectureSpec,
    out: &mut Vec<InvalidArchitectureComponentFinding>,
) -> bool {
    if summary.architecture_components.is_empty() {
        return false;
    }
    if !spec.from_macro_declarations {
        for (span, component) in &summary.architecture_components {
            out.push(InvalidArchitectureComponentFinding {
                span: *span,
                component: component.clone(),
                module: module_path.to_owned(),
                kind: InvalidArchitectureComponentKind::TomlArchitectureActive,
            });
        }
        return true;
    }

    let ancestor_root = module_path
        .rsplit_once(RUST_MODULE_SEPARATOR)
        .and_then(|(parent, _)| {
            resolve_inherited_component_with_root(parent, spec.component_roots())
        });
    let mut first_top_level_component: Option<&str> = None;
    let before_len = out.len();

    for (span, component_name) in &summary.architecture_components {
        if summary.nested_architecture_components.contains(span) {
            out.push(InvalidArchitectureComponentFinding {
                span: *span,
                component: component_name.clone(),
                module: module_path.to_owned(),
                kind: InvalidArchitectureComponentKind::NotAtModuleRoot,
            });
            continue;
        }
        if let Some(first_component) = first_top_level_component {
            out.push(InvalidArchitectureComponentFinding {
                span: *span,
                component: component_name.clone(),
                module: module_path.to_owned(),
                kind: InvalidArchitectureComponentKind::DuplicateInModule {
                    first_component: first_component.to_owned(),
                },
            });
            continue;
        }
        first_top_level_component = Some(component_name.as_str());

        if !spec.contains_component(component_name.as_str()) {
            out.push(InvalidArchitectureComponentFinding {
                span: *span,
                component: component_name.clone(),
                module: module_path.to_owned(),
                kind: InvalidArchitectureComponentKind::UnknownComponent,
            });
        } else if let Some(ref inherited) = ancestor_root {
            out.push(InvalidArchitectureComponentFinding {
                span: *span,
                component: component_name.clone(),
                module: module_path.to_owned(),
                kind: InvalidArchitectureComponentKind::NestedUnderComponent {
                    ancestor_module: inherited.root_module.to_owned(),
                    ancestor_component: inherited.component.clone(),
                },
            });
        }
    }

    out.len() > before_len
}

fn check_rust_file_coverage_and_routers(
    file: &ParsedFile,
    module_path: &str,
    summary: &RustFileSummary,
    spec: &ArchitectureSpec,
    out: &mut Vec<UnmappedArchitectureModuleFinding>,
) {
    if !spec.require_component_coverage {
        return;
    }

    let component_roots = spec.component_roots();
    if let Some(inherited) = resolve_inherited_component_with_root(module_path, component_roots) {
        if let Some(comp_spec) = spec.component_spec(inherited.component) {
            let rel_subpath = module_path
                .strip_prefix(inherited.root_module)
                .and_then(|rest| rest.strip_prefix(RUST_MODULE_SEPARATOR));
            let declared_submodules =
                comp_spec.declared_submodules_at(rel_subpath, RUST_MODULE_SEPARATOR);
            if !declared_submodules.is_empty() {
                for external_mod in &summary.external_mods {
                    if !declared_submodules
                        .iter()
                        .any(|sub| sub.name == external_mod.name)
                    {
                        out.push(UnmappedArchitectureModuleFinding {
                            span: external_mod.span,
                            name: external_mod.name.clone(),
                            module: module_path.to_owned(),
                        });
                    }
                }
            }
        }
        return;
    }

    let is_crate_root = is_rust_crate_root(module_path);
    let header_span = file.first_line_span();
    let before_count = out.len();
    for external_mod in &summary.external_mods {
        let child_prefix = if is_crate_root {
            external_mod.name.clone()
        } else {
            format!("{module_path}::{}", external_mod.name)
        };
        let routes_child = (spec.from_macro_declarations
            && child_prefix == RUST_ARCHITECTURE_MODULE)
            || component_roots
                .keys()
                .any(|root| is_in_rust_module_subtree(root, &child_prefix));
        if !routes_child {
            out.push(UnmappedArchitectureModuleFinding {
                span: external_mod.span,
                name: external_mod.name.clone(),
                module: module_path.to_owned(),
            });
        }
    }

    let disallowed_spans = summary.non_namespace_items.iter().chain(if is_crate_root {
        &[][..]
    } else {
        summary.macro_definitions.as_slice()
    });
    for (span, item_name) in disallowed_spans {
        out.push(UnmappedArchitectureModuleFinding {
            span: *span,
            name: item_name.clone(),
            module: module_path.to_owned(),
        });
    }

    let is_pure_architecture_declaration =
        module_path == RUST_ARCHITECTURE_MODULE && !summary.defined_architecture.is_empty();
    let routes_to_declared = is_crate_root
        || is_pure_architecture_declaration
        || component_roots
            .keys()
            .any(|root| is_in_rust_module_subtree(root, module_path));
    if out.len() == before_count
        && !is_pure_architecture_declaration
        && (summary.external_mods.is_empty() || !routes_to_declared)
    {
        out.push(UnmappedArchitectureModuleFinding {
            span: header_span,
            name: module_path.to_owned(),
            module: module_path.to_owned(),
        });
    }
}

fn resolve_intra_component_reexports(
    canonical_path: &str,
    root_module: Option<&str>,
    resolve_reexport: &impl Fn(&str, &str) -> Option<String>,
) -> String {
    let Some(root) = root_module else {
        return canonical_path.to_owned();
    };
    let mut current = canonical_path.to_owned();
    for _ in 0..MAX_REEXPORT_HOPS {
        let Some(next) = step_reexport_within_root(&current, root, resolve_reexport) else {
            break;
        };
        if next == current {
            break;
        }
        current = next;
    }
    current
}

fn step_reexport_within_root(
    current: &str,
    root_module: &str,
    resolve_reexport: &impl Fn(&str, &str) -> Option<String>,
) -> Option<String> {
    let suffix = current
        .strip_prefix(root_module)?
        .strip_prefix(RUST_MODULE_SEPARATOR)?;
    let mut module_prefix = root_module.to_owned();
    let mut remaining = suffix;
    loop {
        let (symbol, tail) = remaining
            .split_once(RUST_MODULE_SEPARATOR)
            .map_or((remaining, None), |(head, rest)| (head, Some(rest)));
        if let Some(target) = resolve_reexport(&module_prefix, symbol) {
            return Some(match tail {
                Some(rest) => format!("{target}::{rest}"),
                None => target,
            });
        }
        let rest = tail?;
        module_prefix = format!("{module_prefix}::{symbol}");
        remaining = rest;
    }
}

fn check_rust_sibling_isolation(
    module_path: &str,
    canonical_path: &str,
    span: SourceSpan,
    sibling_rules: &[SiblingIsolationRule],
    out: &mut Vec<SiblingModuleDependencyFinding>,
) {
    for sibling_rule in sibling_rules {
        if is_in_rust_module_subtree(module_path, &sibling_rule.subject_prefix)
            && let Some(forbidden_sibling) = sibling_rule
                .forbidden_siblings
                .iter()
                .find(|sibling| is_in_rust_module_subtree(canonical_path, sibling))
        {
            out.push(SiblingModuleDependencyFinding {
                span,
                module: module_path.to_owned(),
                dependency: forbidden_sibling.clone(),
                component: sibling_rule.component_name.clone(),
            });
        }
    }
}

fn collect_rust_reexported_public_items(
    module_path: &str,
    summary: &RustFileSummary,
) -> Vec<ReexportedPublicItemFinding> {
    if is_rust_crate_root(module_path) {
        return Vec::new();
    }
    let mut items: Vec<ReexportedPublicItemFinding> = summary
        .visible_uses
        .iter()
        .filter(|reexport| !summary.is_private_child_facade_reexport(reexport))
        .map(|reexport| ReexportedPublicItemFinding {
            span: reexport.span,
            expression: reexport.declaration_text.clone(),
            module: module_path.to_owned(),
        })
        .collect();
    for &span in &summary.macro_exports {
        items.push(ReexportedPublicItemFinding {
            span,
            expression: "#[macro_export]".to_owned(),
            module: module_path.to_owned(),
        });
    }
    items
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::code_lint::project::foundation::{
        MemoryFileSource, WorkspaceAnchor, build_architecture_spec,
    };

    #[test]
    fn rust_leading_colon_paths_enforce_external_owner_and_component_dependencies() {
        let root = Path::new("/repo");
        let app_source = indoc::indoc! {r"
            use ::ra_ap_syntax::SyntaxNode;
            use ::demo::secret::Token;
        "};
        let source = MemoryFileSource::from_relative_files(
            root,
            &[
                (
                    "Cargo.toml",
                    indoc::indoc! {r#"
                        [package]
                        name = "demo"
                        version = "0.1.0"
                    "#},
                ),
                (
                    "src/lib.rs",
                    indoc::indoc! {r"
                        mod ast;
                        mod secret;
                        mod app;
                    "},
                ),
                ("src/ast.rs", ""),
                ("src/secret.rs", "pub struct Token;\n"),
                ("src/app.rs", app_source),
            ],
        );
        let config: ArchitectureConfig = toml::from_str(indoc::indoc! {r#"
            [[components]]
            name = "Ast"
            roots = ["ast"]
            owns-external = ["ra_ap_syntax"]

            [[components]]
            name = "Secret"
            roots = ["secret"]

            [[components]]
            name = "App"
            roots = ["app"]
            depends-on = ["Ast"]
        "#})
        .unwrap();
        let spec = build_architecture_spec(&config, false).unwrap();
        let layout = RustLayout::discover(WorkspaceAnchor::new(root.to_path_buf(), None), &source);
        let parsed = ParsedFile::rust(app_source);
        let findings = evaluate_rust_file_architecture(
            &parsed,
            &root.join("src/app.rs"),
            &layout,
            &spec,
            &[],
            |_, _| None,
        );

        assert_eq!(findings.unowned_external_dependencies.len(), 1);
        assert_eq!(
            findings.unowned_external_dependencies[0].dependency,
            "ra_ap_syntax"
        );
        assert_eq!(findings.undeclared_component_dependencies.len(), 1);
        assert_eq!(
            findings.undeclared_component_dependencies[0].dependency,
            "Secret"
        );
    }
}
