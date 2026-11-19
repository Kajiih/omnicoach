//! Multi-file Rust project fact extraction and cross-module symbol resolution.

use super::index::RustIndex;
use super::layout::RustLayout;
use super::paths::{
    disambiguated_rust_module_path, is_rust_crate_root, resolve_inherited_component_with_root,
    resolve_rust_reference_path, rust_module_path, rust_subpath_under_root,
};
use crate::code_lint::ast::rust::{RustFileSummary, summarize_rust_file};
use crate::code_lint::ast::{
    IntraFileReference, ParsedFile, ScopeFacts, collect_intra_file_references, collect_scope_facts,
    collect_scope_symbol_names, enclosing_scope_symbol,
};
use crate::code_lint::project::foundation::{
    ArchitectureError, ArchitectureSpec, CrossModuleRefFact, ModuleFact, ProjectFacts,
    ProjectGraphScope, ProjectSourceFile, resolve_unique_module_paths,
};
use crate::diagnostic::Language;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

/// Extracts multi-file [`ProjectFacts`] for Rust `files`, seeding `index` before resolving the
/// workspace [`ArchitectureSpec`] so `RustIndex` reuses the already-parsed module summaries.
///
/// # Errors
///
/// Returns [`ArchitectureError`] when `resolve_spec` fails validation or detects conflicting
/// declarations.
pub(in crate::code_lint::project) fn collect_rust_project_facts<'a>(
    title: &str,
    files: &[ProjectSourceFile],
    index: &RustIndex<'_>,
    scope: ProjectGraphScope,
    resolve_spec: impl FnOnce() -> Result<Option<&'a ArchitectureSpec>, ArchitectureError>,
) -> Result<ProjectFacts, ArchitectureError> {
    let rust_files: Vec<&ProjectSourceFile> = files
        .iter()
        .filter(|file| file.lang == Language::Rust)
        .collect();
    let module_paths = resolved_rust_module_paths(&rust_files, index.layout());
    let entries: Vec<RustModuleEntry> = rust_files
        .into_iter()
        .zip(module_paths)
        .map(|(file, module_path)| parse_rust_module(file, module_path, index))
        .collect();

    let spec = resolve_spec()?;
    let cross_module_refs = resolve_rust_cross_module_refs(&entries, index);
    let mut modules = Vec::with_capacity(entries.len());
    for entry in entries {
        let inherited = spec.and_then(|resolved| {
            resolve_inherited_component_with_root(&entry.module_path, resolved.component_roots())
        });
        let declared_component = inherited.as_ref().map(|matched| matched.component.as_str());
        if declared_component.is_none()
            && entry.summary.non_namespace_items.is_empty()
            && entry.summary.macro_definitions.is_empty()
            && (entry.is_crate_root || spec.is_some())
        {
            continue;
        }
        let submodule_path = match (spec, inherited.as_ref()) {
            (Some(resolved), Some(matched)) => resolved
                .component_spec(matched.component)
                .map_or_else(Vec::new, |comp_spec| {
                    let rel_subpath =
                        rust_subpath_under_root(&entry.module_path, matched.root_module);
                    comp_spec.resolve_submodule_segments(rel_subpath, "::")
                }),
            _ => Vec::new(),
        };
        let component = declared_component.map_or_else(
            || {
                entry
                    .module_path
                    .split_once("::")
                    .map_or(entry.module_path.as_str(), |(head, _)| head)
                    .to_owned()
            },
            str::to_owned,
        );
        modules.push(ModuleFact {
            module_path: entry.module_path,
            path: entry.path,
            component,
            submodule_path,
            scopes: entry.scopes,
            intra_file_refs: entry.intra_file_refs,
        });
    }

    Ok(ProjectFacts::build(
        title,
        spec,
        modules,
        cross_module_refs,
        scope,
    ))
}

struct RustModuleEntry {
    path: String,
    module_path: String,
    is_crate_root: bool,
    summary: Arc<RustFileSummary>,
    scopes: Vec<ScopeFacts>,
    intra_file_refs: Vec<IntraFileReference>,
}

fn resolved_rust_module_paths(
    rust_files: &[&ProjectSourceFile],
    layout: &RustLayout,
) -> Vec<String> {
    resolve_unique_module_paths(
        rust_files,
        |file| {
            layout
                .module_of_path(&file.full_path)
                .unwrap_or_else(|| rust_module_path(&file.relative_path))
        },
        |file| disambiguated_rust_module_path(&file.relative_path),
    )
}

fn parse_rust_module(
    file: &ProjectSourceFile,
    module_path: String,
    index: &RustIndex<'_>,
) -> RustModuleEntry {
    let is_crate_root = is_rust_crate_root(&module_path);
    let parsed = ParsedFile::rust(&file.source);
    let summary = index.seed_summary(&module_path, summarize_rust_file(&parsed));
    let scopes = collect_scope_facts(&parsed);
    let intra_file_refs = collect_intra_file_references(&parsed, &scopes);
    RustModuleEntry {
        path: file.relative_path.clone(),
        module_path,
        is_crate_root,
        summary,
        scopes,
        intra_file_refs,
    }
}

fn resolve_rust_cross_module_refs(
    entries: &[RustModuleEntry],
    index: &RustIndex<'_>,
) -> Vec<CrossModuleRefFact> {
    let crate_name = index.layout().crate_name.as_deref();
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
        for reference in &entry.summary.referenced_paths {
            let resolved = resolve_rust_reference_path(
                &entry.module_path,
                &entry.summary.external_mods,
                &reference.raw_path,
                crate_name,
            );
            if let Some(target) = index.resolve_module_target(
                &resolved.canonical_path,
                &known_modules,
                &module_symbols,
            ) && target.module != entry.module_path
            {
                refs.push(CrossModuleRefFact {
                    source_module: entry.module_path.clone(),
                    source_symbol: enclosing_scope_symbol(&entry.scopes, reference.line),
                    target_module: target.module,
                    target_symbol: target.symbol,
                    line: reference.line,
                });
            }
        }
    }
    refs.sort();
    refs.dedup();
    refs
}
