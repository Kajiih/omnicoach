//! Multi-file Python project fact extraction and cross-module symbol resolution (`CodeLintProjectPython`).

use super::architecture::{resolve_python_component, routes_to_python_component};
use super::index::PythonIndex;
use super::layout::PythonLayout;
use super::paths::{
    INIT_STEM, is_python_package_init, python_dotted_path, resolve_relative_python_module,
};
use crate::code_lint::ast::python::collect_path_references;
use crate::code_lint::ast::{
    IntraFileReference, ParsedFile, ScopeFacts, collect_intra_file_references, collect_scope_facts,
    collect_scope_symbol_names, enclosing_scope_symbol,
};
use crate::code_lint::project::foundation::{
    ArchitectureSpec, CrossModuleRefFact, ModuleFact, ProjectFacts, ProjectGraphScope,
    ProjectSourceFile, resolve_unique_module_paths,
};
use crate::diagnostic::Language;
use std::collections::{HashMap, HashSet};

/// Extracts multi-file [`ProjectFacts`] for Python `files`, seeding `index` and resolving
/// component assignments and cross-module references.
#[must_use]
pub(in crate::code_lint::project) fn collect_python_project_facts(
    title: &str,
    files: &[ProjectSourceFile],
    index: &PythonIndex<'_>,
    spec: Option<&ArchitectureSpec>,
    scope: ProjectGraphScope,
) -> ProjectFacts {
    let layout = index.layout();
    let python_files: Vec<&ProjectSourceFile> = files
        .iter()
        .filter(|file| {
            file.lang == Language::Python
                && !(scope == ProjectGraphScope::FullProject
                    && layout
                        .architecture_module_of_path(&file.full_path)
                        .is_none()
                    && spec.is_some_and(|resolved| {
                        let fallback = layout.fallback_module_path(
                            &file.full_path,
                            &file.relative_path,
                            is_python_package_init(&file.full_path),
                        );
                        resolve_python_component(&fallback, resolved).is_none()
                    }))
        })
        .collect();
    let module_paths = resolved_python_module_paths(&python_files, layout);
    let entries: Vec<PythonModuleEntry> = python_files
        .into_iter()
        .zip(module_paths)
        .map(|(file, module_path)| parse_python_module(file, module_path, index))
        .collect();
    let cross_module_refs = resolve_python_cross_module_refs(&entries, index);
    let mut modules = Vec::with_capacity(entries.len());

    for entry in entries {
        let declared_component = spec
            .and_then(|resolved| resolve_python_component(&entry.module_path, resolved))
            .map(|matched| matched.component_name);
        let is_unmapped_pure_router = declared_component.is_none()
            && entry.is_package_init
            && !entry.has_non_routing_items
            && spec
                .is_some_and(|resolved| routes_to_python_component(&entry.module_path, resolved));
        if !is_unmapped_pure_router {
            let component = declared_component.map_or_else(
                || python_component_name(&entry.module_path, title),
                str::to_owned,
            );
            modules.push(ModuleFact {
                module_path: entry.module_path,
                path: entry.path,
                component,
                scopes: entry.scopes,
                intra_file_refs: entry.intra_file_refs,
            });
        }
    }

    ProjectFacts::build(title, spec, modules, cross_module_refs, scope)
}

struct PythonModuleEntry {
    path: String,
    module_path: String,
    is_package_init: bool,
    has_non_routing_items: bool,
    references: Vec<(usize, String)>,
    scopes: Vec<ScopeFacts>,
    intra_file_refs: Vec<IntraFileReference>,
}

fn resolved_python_module_paths(
    python_files: &[&ProjectSourceFile],
    layout: &PythonLayout,
) -> Vec<String> {
    resolve_unique_module_paths(
        python_files,
        |file| {
            let is_package_init = is_python_package_init(&file.full_path);
            layout.module_of_path(&file.full_path).unwrap_or_else(|| {
                layout.fallback_module_path(&file.full_path, &file.relative_path, is_package_init)
            })
        },
        |file| python_dotted_path(&file.relative_path, false),
    )
}

fn parse_python_module(
    file: &ProjectSourceFile,
    module_path: String,
    index: &PythonIndex<'_>,
) -> PythonModuleEntry {
    let is_package_init = is_python_package_init(&file.full_path);
    let parsed = ParsedFile::new(&file.source, Language::Python);
    let summary = index.seed_module(&file.full_path, &module_path, &parsed);
    let has_non_routing_items = !summary.non_routing_items.is_empty();
    let scopes = collect_scope_facts(&parsed);
    let intra_file_refs = collect_intra_file_references(&parsed, &scopes);
    let references = collect_path_references(&parsed);
    PythonModuleEntry {
        path: file.relative_path.clone(),
        module_path,
        is_package_init,
        has_non_routing_items,
        references,
        scopes,
        intra_file_refs,
    }
}

fn resolve_python_cross_module_refs(
    entries: &[PythonModuleEntry],
    index: &PythonIndex<'_>,
) -> Vec<CrossModuleRefFact> {
    let known_modules: HashSet<&str> = entries
        .iter()
        .map(|entry| entry.module_path.as_str())
        .collect();
    let mut module_symbols: HashMap<&str, HashSet<String>> = HashMap::new();
    for entry in entries {
        let symbols = module_symbols
            .entry(entry.module_path.as_str())
            .or_default();
        collect_scope_symbol_names(&entry.scopes, symbols);
    }
    let mut refs = Vec::new();
    for entry in entries {
        for &(line, ref raw_path) in &entry.references {
            let dots = raw_path
                .chars()
                .take_while(|&character| character == '.')
                .count();
            let Some(normalized) = resolve_relative_python_module(
                &entry.module_path,
                entry.is_package_init,
                u32::try_from(dots).unwrap_or(u32::MAX),
                Some(&raw_path[dots..]),
            ) else {
                continue;
            };
            if let Some(target) =
                index.resolve_module_target(&normalized, &known_modules, &module_symbols)
                && target.module != entry.module_path
            {
                refs.push(CrossModuleRefFact {
                    source_module: entry.module_path.clone(),
                    source_symbol: enclosing_scope_symbol(&entry.scopes, line),
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

fn python_component_name(module_path: &str, fallback_title: &str) -> String {
    module_path
        .split('.')
        .next()
        .filter(|segment| !segment.is_empty() && *segment != INIT_STEM)
        .unwrap_or(fallback_title)
        .to_owned()
}
