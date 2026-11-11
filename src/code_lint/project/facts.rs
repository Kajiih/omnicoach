//! Multi-file project abstraction facts, source discovery, and cross-module symbol resolution (`CodeLintProject`).

use super::architecture::{ArchitectureError, ArchitectureSpec};
use super::config::{ProjectConfig, resolve_against_cwd};
use super::index::ProjectIndex;
use super::layout::{
    ProjectLayout, disambiguated_rust_module_path, is_python_package_init, is_rust_crate_root,
    normalize_relative_path, python_dotted_path, resolve_relative_python_module,
    resolve_rust_reference_path, rust_module_path,
};
use super::source::{DiskFileSource, FileSource};
use crate::code_lint::ast::python::collect_path_references;
use crate::code_lint::ast::rust::{ExternalModDeclaration, summarize_rust_file};
use crate::code_lint::ast::{
    DeclarationScopeKind, IntraFileReference, ParsedFile, ScopeFacts,
    collect_intra_file_references, collect_scope_facts, enclosing_scope_symbol,
};
use crate::diagnostic::Language;
use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};

const INIT_MODULE: &str = "__init__";
const CARGO_TOML_NAME: &str = "Cargo.toml";

/// A project source file discovered via [`FileSource`], paired with its full query path,
/// normalized display path, and UTF-8 source text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::code_lint) struct ProjectSourceFile {
    full_path: PathBuf,
    /// Forward-slash-normalized display path relative to the project root.
    pub(in crate::code_lint) relative_path: String,
    /// Detected source language (`Rust` or `Python`).
    pub(in crate::code_lint) lang: Language,
    /// Full UTF-8 source text.
    pub(in crate::code_lint) source: String,
}

/// Discovered source files paired with the shared [`ProjectIndex`] governing their repository.
pub(in crate::code_lint) struct DiscoveredProjectSources<'s> {
    index: ProjectIndex<'s>,
    /// Deterministic, deduplicated source files under the requested paths.
    pub(in crate::code_lint) files: Vec<ProjectSourceFile>,
    scope: ProjectGraphScope,
}

/// Multi-file project facts across a Rust crate or Python package.
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

impl DiscoveredProjectSources<'_> {
    /// Extracts multi-file [`ProjectFacts`] for `lang` from these discovered sources, seeding the
    /// shared [`ProjectIndex`] from each file's single AST parse and resolving architecture and
    /// cross-module re-exports against [`ProjectIndex`].
    ///
    /// # Errors
    ///
    /// Returns [`ArchitectureError`] when project architecture declarations conflict or fail
    /// validation.
    pub(in crate::code_lint) fn collect_project_facts(
        &self,
        title: &str,
        lang: Language,
    ) -> Result<ProjectFacts, ArchitectureError> {
        match lang {
            Language::Rust => {
                collect_rust_project_facts(title, &self.files, &self.index, self.scope)
            }
            Language::Python => {
                collect_python_project_facts(title, &self.files, &self.index, self.scope)
            }
        }
    }
}

/// Discovers non-excluded `.rs` and `.py` files under `paths` alongside a shared [`ProjectIndex`]
/// backed by [`DiskFileSource`].
///
/// # Errors
///
/// Returns an error if an explicitly passed file path cannot be read or if an external
/// `.omnicoach.toml` fails validation.
pub(in crate::code_lint) fn discover_project_sources(
    paths: &[PathBuf],
    config: &ProjectConfig,
) -> anyhow::Result<DiscoveredProjectSources<'static>> {
    discover_project_sources_in(paths, &DiskFileSource, config)
}

/// Discovers non-excluded `.rs` and `.py` files under `paths` in `source` alongside a shared
/// [`ProjectIndex`].
///
/// # Errors
///
/// Returns an error if an explicitly passed file path cannot be read from `source` or if an
/// external `.omnicoach.toml` in `source` fails validation.
pub(in crate::code_lint) fn discover_project_sources_in<'s>(
    paths: &[PathBuf],
    source: &'s dyn FileSource,
    config: &ProjectConfig,
) -> anyhow::Result<DiscoveredProjectSources<'s>> {
    let anchor = paths
        .first()
        .map_or_else(|| Path::new("."), PathBuf::as_path);
    let mut index = ProjectIndex::discover_with_config(anchor, source, config)?;
    let configured_roots = index.project_config().configured_source_roots();
    for extra_path in paths.iter().skip(1) {
        let extra_layout = ProjectLayout::discover(extra_path, source, &configured_roots);
        if extra_layout.project_root == index.layout().project_root {
            index.merge_python_source_roots(&extra_layout.python_source_roots);
        }
    }

    let mut files = Vec::new();
    for path in paths {
        if source.is_file(path) {
            if let Some(collected) = collect_single_file_source(path, source, index.layout())? {
                files.push(collected);
            }
        } else if source.is_dir(path) {
            collect_directory_source_files(
                path,
                source,
                index.layout(),
                index.project_config(),
                &mut files,
            );
        }
    }
    files.sort_by(|left, right| left.relative_path.cmp(&right.relative_path));
    files.dedup_by(|left, right| left.relative_path == right.relative_path);

    merge_discovered_python_subpackage_roots(&files, source, &mut index);
    let scope = determine_graph_scope(paths, source, &index);

    Ok(DiscoveredProjectSources {
        index,
        files,
        scope,
    })
}

/// Resolves `paths` (defaulting to `"."` when empty) and verifies that each path exists on disk.
///
/// # Errors
///
/// Returns an error if any resolved path does not exist.
pub(in crate::code_lint) fn resolve_effective_paths(
    paths: &[PathBuf],
) -> anyhow::Result<Vec<PathBuf>> {
    let effective = if paths.is_empty() {
        vec![PathBuf::from(".")]
    } else {
        paths.to_vec()
    };
    for path in &effective {
        if !DiskFileSource.is_file(path) && !DiskFileSource.is_dir(path) {
            anyhow::bail!("Path does not exist: {}", path.display());
        }
    }
    Ok(effective)
}

/// Derives a human-readable project title from `paths` (falling back to the current directory name).
#[must_use]
pub(in crate::code_lint) fn derive_project_title(paths: &[PathBuf]) -> String {
    derive_project_title_with_cwd(paths, DiskFileSource.current_dir().as_deref())
}

fn derive_project_title_with_cwd(paths: &[PathBuf], current_dir: Option<&Path>) -> String {
    if let [only] = paths {
        let resolved = resolve_against_cwd(only, current_dir);
        if let Some(name) = resolved.file_name().and_then(|os| os.to_str())
            && !name.is_empty()
        {
            return name.to_owned();
        }
    }
    "project".to_owned()
}

/// Whether project-fact extraction covers a whole project/source root or a narrower file/directory subset.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProjectGraphScope {
    /// Full project or source root: include all declared components from [`ArchitectureSpec`],
    /// preserving empty components and `Absent` Reflexion edges.
    FullProject,
    /// Subdirectory or explicit file subset: resolve each module's true declared component from
    /// [`ArchitectureSpec`] while retaining only components that own at least one collected module.
    Subset,
}

fn collect_rust_project_facts(
    title: &str,
    files: &[ProjectSourceFile],
    index: &ProjectIndex<'_>,
    scope: ProjectGraphScope,
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

    let spec = index.architecture_spec();
    if let Some(error) = index.architecture_error() {
        return Err(error.clone());
    }
    let mut modules = Vec::new();
    for entry in &entries {
        let declared_component = spec.and_then(|resolved| {
            resolved.resolve_component_of_module(&entry.module_path, Language::Rust)
        });
        if declared_component.is_none()
            && !entry.has_non_namespace_items
            && !entry.has_macro_definitions
            && (entry.is_crate_root || spec.is_some())
        {
            continue;
        }
        let component = declared_component.map_or_else(
            || {
                entry
                    .module_path
                    .split("::")
                    .next()
                    .unwrap_or("root")
                    .to_owned()
            },
            str::to_owned,
        );
        modules.push(ModuleFact {
            module_path: entry.module_path.clone(),
            path: entry.path.clone(),
            component,
            scopes: entry.scopes.clone(),
            intra_file_refs: entry.intra_file_refs.clone(),
        });
    }

    let finalized = finalize_components(spec, &modules, scope);
    let cross_module_refs = resolve_rust_cross_module_refs(&entries, index);
    Ok(ProjectFacts {
        title: title.to_owned(),
        has_declared_architecture: finalized.has_declared_architecture,
        components: finalized.components,
        modules,
        cross_module_refs,
    })
}

struct RustModuleEntry {
    path: String,
    module_path: String,
    is_crate_root: bool,
    external_mods: Vec<ExternalModDeclaration>,
    has_non_namespace_items: bool,
    has_macro_definitions: bool,
    references: Vec<RawSymbolReference>,
    scopes: Vec<ScopeFacts>,
    intra_file_refs: Vec<IntraFileReference>,
}

fn resolved_rust_module_paths(
    rust_files: &[&ProjectSourceFile],
    layout: &ProjectLayout,
) -> Vec<String> {
    resolve_unique_module_paths(
        rust_files,
        |file| {
            layout
                .rust_module_of_path(&file.full_path)
                .unwrap_or_else(|| rust_module_path(&file.relative_path))
        },
        |file| disambiguated_rust_module_path(&file.relative_path),
    )
}

fn parse_rust_module(
    file: &ProjectSourceFile,
    module_path: String,
    index: &ProjectIndex<'_>,
) -> RustModuleEntry {
    let is_crate_root = is_rust_crate_root(&module_path);
    let parsed = ParsedFile::rust(&file.source);
    let summary = summarize_rust_file(&parsed);
    index.seed_rust_summary(&module_path, summary.clone());
    let external_mods = summary.external_mods;
    let has_non_namespace_items = !summary.non_namespace_items.is_empty();
    let has_macro_definitions = !summary.macro_definitions.is_empty();
    let scopes = collect_scope_facts(&parsed);
    let intra_file_refs = collect_intra_file_references(&parsed, &scopes);
    let references = summary
        .referenced_paths
        .into_iter()
        .map(|reference| RawSymbolReference {
            line: reference.line,
            source_symbol: enclosing_scope_symbol(&scopes, reference.line),
            raw_path: reference.raw_path,
        })
        .collect();
    RustModuleEntry {
        path: file.relative_path.clone(),
        module_path,
        is_crate_root,
        external_mods,
        has_non_namespace_items,
        has_macro_definitions,
        references,
        scopes,
        intra_file_refs,
    }
}

fn resolve_rust_cross_module_refs(
    entries: &[RustModuleEntry],
    index: &ProjectIndex<'_>,
) -> Vec<CrossModuleRefFact> {
    let crate_name = index.layout().rust_crate_name.as_deref();
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
        for reference in &entry.references {
            let resolved = resolve_rust_reference_path(
                &entry.module_path,
                &entry.external_mods,
                &reference.raw_path,
                crate_name,
            );
            if let Some(target) = index.resolve_rust_module_target(
                &resolved.canonical_path,
                &known_modules,
                &module_symbols,
            ) && target.module != entry.module_path
            {
                refs.push(CrossModuleRefFact {
                    source_module: entry.module_path.clone(),
                    source_symbol: reference.source_symbol.clone(),
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

fn collect_python_project_facts(
    title: &str,
    files: &[ProjectSourceFile],
    index: &ProjectIndex<'_>,
    scope: ProjectGraphScope,
) -> Result<ProjectFacts, ArchitectureError> {
    let python_files: Vec<&ProjectSourceFile> = files
        .iter()
        .filter(|file| file.lang == Language::Python)
        .collect();
    let module_paths = resolved_python_module_paths(&python_files, index.layout());
    let entries: Vec<PythonModuleEntry> = python_files
        .into_iter()
        .zip(module_paths)
        .map(|(file, module_path)| parse_python_module(file, module_path, index))
        .collect();
    let spec = index.architecture_spec();
    if let Some(error) = index.architecture_error() {
        return Err(error.clone());
    }
    let cross_module_refs = resolve_python_cross_module_refs(&entries, index);
    let mut modules = Vec::with_capacity(entries.len());

    for entry in entries {
        let component = spec
            .and_then(|resolved| {
                resolved.resolve_component_of_module(&entry.module_path, Language::Python)
            })
            .map_or_else(
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

    let finalized = finalize_components(spec, &modules, scope);
    Ok(ProjectFacts {
        title: title.to_owned(),
        has_declared_architecture: finalized.has_declared_architecture,
        components: finalized.components,
        modules,
        cross_module_refs,
    })
}

struct PythonModuleEntry {
    path: String,
    module_path: String,
    is_package_init: bool,
    references: Vec<RawSymbolReference>,
    scopes: Vec<ScopeFacts>,
    intra_file_refs: Vec<IntraFileReference>,
}

fn resolved_python_module_paths(
    python_files: &[&ProjectSourceFile],
    layout: &ProjectLayout,
) -> Vec<String> {
    resolve_unique_module_paths(
        python_files,
        |file| {
            let is_package_init = is_python_package_init(&file.full_path);
            layout
                .python_module_of_path(&file.full_path)
                .unwrap_or_else(|| {
                    layout.fallback_python_module_path(
                        &file.full_path,
                        &file.relative_path,
                        is_package_init,
                    )
                })
        },
        |file| python_dotted_path(&file.relative_path, false),
    )
}

fn parse_python_module(
    file: &ProjectSourceFile,
    module_path: String,
    index: &ProjectIndex<'_>,
) -> PythonModuleEntry {
    let is_package_init = is_python_package_init(&file.full_path);
    let parsed = ParsedFile::new(&file.source, Language::Python);
    index.seed_python_module(&file.full_path, &module_path, &parsed);
    let scopes = collect_scope_facts(&parsed);
    let intra_file_refs = collect_intra_file_references(&parsed, &scopes);
    let references = collect_path_references(&parsed)
        .into_iter()
        .map(|(line, raw_path)| RawSymbolReference {
            line,
            source_symbol: enclosing_scope_symbol(&scopes, line),
            raw_path,
        })
        .collect();
    PythonModuleEntry {
        path: file.relative_path.clone(),
        module_path,
        is_package_init,
        references,
        scopes,
        intra_file_refs,
    }
}

fn resolve_python_cross_module_refs(
    entries: &[PythonModuleEntry],
    index: &ProjectIndex<'_>,
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
        for reference in &entry.references {
            let dots = reference
                .raw_path
                .chars()
                .take_while(|&character| character == '.')
                .count();
            let Some(normalized) = resolve_relative_python_module(
                &entry.module_path,
                entry.is_package_init,
                u32::try_from(dots).unwrap_or(u32::MAX),
                Some(&reference.raw_path[dots..]),
            ) else {
                continue;
            };
            if let Some(target) =
                index.resolve_python_module_target(&normalized, &known_modules, &module_symbols)
                && target.module != entry.module_path
            {
                refs.push(CrossModuleRefFact {
                    source_module: entry.module_path.clone(),
                    source_symbol: reference.source_symbol.clone(),
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

fn python_component_name(module_path: &str, fallback_title: &str) -> String {
    module_path
        .split('.')
        .next()
        .filter(|segment| !segment.is_empty() && *segment != INIT_MODULE)
        .unwrap_or(fallback_title)
        .to_owned()
}

struct RawSymbolReference {
    line: usize,
    source_symbol: Option<String>,
    raw_path: String,
}

fn resolve_unique_module_paths(
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

struct FinalizedComponents {
    components: Vec<ComponentFact>,
    has_declared_architecture: bool,
}

fn finalize_components(
    spec: Option<&ArchitectureSpec>,
    modules: &[ModuleFact],
    scope: ProjectGraphScope,
) -> FinalizedComponents {
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
    for module in modules {
        if seen_components.insert(module.component.clone()) {
            components.push(ComponentFact {
                name: module.component.clone(),
                declared_dependencies: None,
                allowed_dependencies: Vec::new(),
            });
        }
    }

    FinalizedComponents {
        components,
        has_declared_architecture,
    }
}

fn collect_scope_symbol_names(scopes: &[ScopeFacts], out: &mut HashSet<String>) {
    for scope in scopes {
        match &scope.scope {
            DeclarationScopeKind::Module => {
                for item in &scope.items {
                    out.insert(item.name.clone());
                }
            }
            DeclarationScopeKind::Class { name }
            | DeclarationScopeKind::Impl { type_name: name } => {
                for item in &scope.items {
                    out.insert(format!("{name}::{}", item.name));
                }
            }
        }
    }
}

fn merge_discovered_python_subpackage_roots(
    files: &[ProjectSourceFile],
    source: &dyn FileSource,
    index: &mut ProjectIndex<'_>,
) {
    if index.project_config().has_configured_python_roots() {
        return;
    }
    let project_root = index.layout().project_root.clone();
    let mut visited_directories = HashSet::new();
    let mut inferred_roots = Vec::new();
    for file in files {
        if file.lang != Language::Python {
            continue;
        }
        let mut cursor = file.full_path.parent();
        while let Some(directory) = cursor {
            if !directory.starts_with(&project_root)
                || directory == project_root
                || !visited_directories.insert(directory.to_path_buf())
            {
                break;
            }
            if let Some(root) = index.layout().infer_package_source_root(directory, source) {
                inferred_roots.push(root);
            }
            cursor = directory.parent();
        }
    }
    if !inferred_roots.is_empty() {
        index.merge_python_source_roots(&inferred_roots);
    }
}

fn determine_graph_scope(
    paths: &[PathBuf],
    source: &dyn FileSource,
    index: &ProjectIndex<'_>,
) -> ProjectGraphScope {
    let layout = index.layout();
    let resolved_dirs: Vec<PathBuf> = paths
        .iter()
        .filter(|path| source.is_dir(path))
        .map(|path| layout.resolve_query_path(path))
        .collect();
    let covers_project_or_rust_root = resolved_dirs.iter().any(|resolved_dir| {
        *resolved_dir == layout.project_root || *resolved_dir == layout.rust_source_root
    });
    let covers_configured_python_roots = index.project_config().has_configured_python_roots()
        && !layout.python_source_roots.is_empty()
        && layout.python_source_roots.iter().all(|python_root| {
            resolved_dirs
                .iter()
                .any(|resolved_dir| python_root.starts_with(resolved_dir))
        });
    if covers_project_or_rust_root || covers_configured_python_roots {
        ProjectGraphScope::FullProject
    } else {
        ProjectGraphScope::Subset
    }
}

fn collect_single_file_source(
    path: &Path,
    source: &dyn FileSource,
    layout: &ProjectLayout,
) -> anyhow::Result<Option<ProjectSourceFile>> {
    let Some(lang) = Language::from_path(path) else {
        return Ok(None);
    };
    let file_source = source.read_to_string(path)?;
    let full_path = layout.resolve_query_path(path);
    let relative_path =
        normalize_relative_path(full_path.strip_prefix(&layout.project_root).unwrap_or(path));
    Ok(Some(ProjectSourceFile {
        full_path,
        relative_path,
        lang,
        source: file_source,
    }))
}

fn collect_directory_source_files(
    dir: &Path,
    source: &dyn FileSource,
    layout: &ProjectLayout,
    config: &ProjectConfig,
    out: &mut Vec<ProjectSourceFile>,
) {
    let resolved_dir = layout.resolve_query_path(dir);
    let allow_test_files = config.is_test_directory(&resolved_dir);
    let restrict_rust_to_source_root = resolved_dir == layout.project_root
        && layout.rust_source_root != layout.project_root
        && source.is_file(&layout.project_root.join(CARGO_TOML_NAME));

    let Ok(walked_files) = source.walk_files(dir) else {
        return;
    };
    for entry_path in walked_files {
        let Some(lang) = Language::from_path(&entry_path) else {
            continue;
        };
        let full_path = layout.resolve_query_path(&entry_path);
        if !allow_test_files && config.is_test_path(&full_path) {
            continue;
        }
        if restrict_rust_to_source_root
            && lang == Language::Rust
            && !full_path.starts_with(&layout.rust_source_root)
        {
            continue;
        }
        let Ok(file_source) = source.read_to_string(&entry_path) else {
            continue;
        };
        let relative_path = normalize_relative_path(
            full_path
                .strip_prefix(&layout.project_root)
                .or_else(|_| full_path.strip_prefix(&resolved_dir))
                .unwrap_or(&entry_path),
        );
        out.push(ProjectSourceFile {
            full_path,
            relative_path,
            lang,
            source: file_source,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::super::config::ArchitectureConfig;
    use super::super::source::MemoryFileSource;
    use super::*;

    #[test]
    fn rust_project_facts_resolve_components_reexports_and_skip_pure_routers() {
        let files = [
            (
                "src/lib.rs",
                indoc::indoc! {r"
                    mod architecture;
                    mod core;
                    mod router;
                    mod runner;
                "},
            ),
            ("src/router.rs", "pub mod nested;\n"),
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

        let project = collect_virtual_facts(
            "sample_rs",
            Language::Rust,
            &files,
            &ProjectConfig::default(),
        );

        assert_eq!(project.components.len(), 3);
        assert!(project.cross_module_refs.iter().any(|reference| {
            reference.source_module == "runner"
                && reference.target_module == "core"
                && reference.target_symbol.as_deref() == Some("parse_input")
        }));
        assert!(project.cross_module_refs.iter().any(|reference| {
            reference.source_module == "runner"
                && reference.target_module == "core::detail"
                && reference.target_symbol.as_deref() == Some("Config")
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

        let project = collect_virtual_facts(
            "sample_py",
            Language::Python,
            &files,
            &ProjectConfig::default(),
        );

        assert!(!project.has_declared_architecture);
        assert_eq!(project.modules.len(), 3);
        assert!(project.cross_module_refs.iter().any(|reference| {
            reference.source_module == "pkg._core"
                && reference.target_module == "pkg._util"
                && reference.target_symbol.as_deref() == Some("compute_step")
        }));
    }

    #[test]
    fn rust_cross_module_refs_attribute_method_sources_and_associated_fn_targets() {
        let files = [
            (
                "src/core.rs",
                indoc::indoc! {r"
                    pub struct Parser;

                    impl Parser {
                        pub fn parse() -> Self {
                            Self
                        }
                    }
                "},
            ),
            (
                "src/runner.rs",
                indoc::indoc! {r"
                    use crate::core::Parser;

                    pub struct Runner;

                    impl Runner {
                        pub fn execute(&self) -> Parser {
                            Parser::parse()
                        }
                    }
                "},
            ),
        ];

        let project = collect_virtual_facts(
            "sample_methods",
            Language::Rust,
            &files,
            &ProjectConfig::default(),
        );

        assert!(project.cross_module_refs.iter().any(|reference| {
            reference.source_module == "runner"
                && reference.source_symbol.as_deref() == Some("Runner::execute")
                && reference.target_module == "core"
                && reference.target_symbol.as_deref() == Some("Parser::parse")
        }));
    }

    #[test]
    fn python_project_facts_honor_declared_toml_architecture_and_non_transitive_mode() {
        let files = [
            ("pkg/core.py", "def low() -> int:\n    return 1\n"),
            ("pkg/domain.py", "def compute() -> int:\n    return 1\n"),
            (
                "pkg/api.py",
                indoc::indoc! {"
                    from pkg.domain import compute

                    def serve() -> int:
                        return compute()
                "},
            ),
            ("pkg/orphan.py", "def stray() -> int:\n    return 0\n"),
        ];
        let architecture_config: ArchitectureConfig = toml::from_str(indoc::indoc! {r#"
            transitive-dependencies = false
            require-component-coverage = false

            [[components]]
            name = "Core"
            roots = ["pkg.core"]

            [[components]]
            name = "Domain"
            roots = ["pkg.domain"]
            depends-on = ["Core"]

            [[components]]
            name = "Api"
            roots = ["pkg.api"]
            depends-on = ["Domain"]
        "#})
        .expect("parse architecture config");
        let config = ProjectConfig {
            architecture: Some(architecture_config),
            ..ProjectConfig::default()
        };

        let project = collect_virtual_facts("sample_py", Language::Python, &files, &config);

        assert!(project.has_declared_architecture);
        assert_eq!(
            project.components,
            vec![
                ComponentFact {
                    name: "Core".to_owned(),
                    declared_dependencies: Some(Vec::new()),
                    allowed_dependencies: Vec::new(),
                },
                ComponentFact {
                    name: "Domain".to_owned(),
                    declared_dependencies: Some(vec!["Core".to_owned()]),
                    allowed_dependencies: vec!["Core".to_owned()],
                },
                ComponentFact {
                    name: "Api".to_owned(),
                    declared_dependencies: Some(vec!["Domain".to_owned()]),
                    allowed_dependencies: vec!["Domain".to_owned()],
                },
                ComponentFact {
                    name: "pkg".to_owned(),
                    declared_dependencies: None,
                    allowed_dependencies: Vec::new(),
                },
            ]
        );
    }

    #[test]
    fn python_project_facts_follow_reexports_and_reexported_submodules_via_project_index() {
        let files = [
            (
                "pkg/__init__.py",
                indoc::indoc! {"
                    from ._core import build_plan
                    from . import _core as core
                "},
            ),
            (
                "pkg/_core.py",
                indoc::indoc! {"
                    def build_plan() -> int:
                        return 1

                    class Parser:
                        @staticmethod
                        def parse() -> int:
                            return 2
                "},
            ),
            (
                "pkg/api.py",
                indoc::indoc! {"
                    import pkg
                    from pkg import build_plan

                    def handle() -> int:
                        return build_plan() + pkg.core.Parser.parse()
                "},
            ),
        ];

        let project = collect_virtual_facts(
            "sample_reexport",
            Language::Python,
            &files,
            &ProjectConfig::default(),
        );

        assert!(project.cross_module_refs.iter().any(|reference| {
            reference.source_module == "pkg.api"
                && reference.source_symbol.as_deref() == Some("handle")
                && reference.target_module == "pkg._core"
                && reference.target_symbol.as_deref() == Some("build_plan")
        }));
        assert!(project.cross_module_refs.iter().any(|reference| {
            reference.source_module == "pkg.api"
                && reference.source_symbol.as_deref() == Some("handle")
                && reference.target_module == "pkg._core"
                && reference.target_symbol.as_deref() == Some("Parser::parse")
        }));
    }

    #[test]
    fn subset_scope_separates_direct_declared_dependencies_from_transitive_allowed_dependencies() {
        let all_files = [
            (
                "src/architecture.rs",
                indoc::indoc! {r"
                    architecture_component!(Architecture);
                    define_architecture! {
                        Architecture => [],
                        Unused => [],
                        Core => [],
                        Runner => [Core],
                        Cli => [Runner],
                    }
                "},
            ),
            (
                "src/unused.rs",
                "architecture_component!(Unused);\npub fn idle() {}\n",
            ),
            (
                "src/core.rs",
                "architecture_component!(Core);\npub fn compute() -> usize { 1 }\n",
            ),
            (
                "src/runner.rs",
                "architecture_component!(Runner);\npub fn run() -> usize { crate::core::compute() }\n",
            ),
            (
                "src/cli.rs",
                "architecture_component!(Cli);\npub fn entry() -> usize { crate::core::compute() }\n",
            ),
        ];
        let root = Path::new(MemoryFileSource::VIRTUAL_ROOT);
        let memory = MemoryFileSource::from_relative_files(root, &all_files);
        let subset_paths = [root.join("src/core.rs"), root.join("src/cli.rs")];
        let discovered =
            discover_project_sources_in(&subset_paths, &memory, &ProjectConfig::default())
                .expect("discover subset sources");

        let project = discovered
            .collect_project_facts("subset_rs", Language::Rust)
            .expect("collect subset project facts");

        assert_eq!(
            project.components,
            vec![
                ComponentFact {
                    name: "Core".to_owned(),
                    declared_dependencies: Some(Vec::new()),
                    allowed_dependencies: Vec::new(),
                },
                ComponentFact {
                    name: "Cli".to_owned(),
                    declared_dependencies: Some(Vec::new()),
                    allowed_dependencies: vec!["Core".to_owned()],
                },
            ]
        );
    }

    #[test]
    fn rust_project_facts_normalize_self_crate_paths_from_cargo_toml() {
        let files = [
            (
                "Cargo.toml",
                indoc::indoc! {r#"
                    [package]
                    name = "my-sample"
                    version = "0.1.0"
                "#},
            ),
            ("src/lib.rs", "pub mod core;\npub mod api;\n"),
            ("src/core.rs", "pub fn compute() -> usize { 1 }\n"),
            (
                "src/api.rs",
                "pub fn serve() -> usize { my_sample::core::compute() }\n",
            ),
        ];

        let project = collect_virtual_facts(
            "my_sample",
            Language::Rust,
            &files,
            &ProjectConfig::default(),
        );

        assert_eq!(
            project.cross_module_refs,
            vec![CrossModuleRefFact {
                source_module: "api".to_owned(),
                source_symbol: Some("serve".to_owned()),
                target_module: "core".to_owned(),
                target_symbol: Some("compute".to_owned()),
                line: 1,
            }]
        );
    }

    #[test]
    fn python_project_facts_honor_source_roots_and_keep_distinct_out_of_root_scripts() {
        let files = [
            (
                ".omnicoach.toml",
                indoc::indoc! {r#"
                    [context]
                    source-roots = ["python"]
                "#},
            ),
            (
                "python/pkg/core.py",
                "def compute() -> int:\n    return 1\n",
            ),
            (
                "python/pkg/api.py",
                indoc::indoc! {"
                    from pkg.core import compute

                    def serve() -> int:
                        return compute()
                "},
            ),
            ("setup.py", "def build() -> int:\n    return 0\n"),
            (
                "scripts/noxfile.py",
                "def session() -> int:\n    return 0\n",
            ),
        ];

        let project = collect_virtual_facts(
            "custom_root_py",
            Language::Python,
            &files,
            &ProjectConfig::default(),
        );
        let module_paths: Vec<&str> = project
            .modules
            .iter()
            .map(|module| module.module_path.as_str())
            .collect();

        assert_eq!(
            module_paths,
            ["pkg.api", "pkg.core", "scripts.noxfile", "setup"]
        );
        assert_eq!(
            project.cross_module_refs,
            vec![
                CrossModuleRefFact {
                    source_module: "pkg.api".to_owned(),
                    source_symbol: None,
                    target_module: "pkg.core".to_owned(),
                    target_symbol: Some("compute".to_owned()),
                    line: 1,
                },
                CrossModuleRefFact {
                    source_module: "pkg.api".to_owned(),
                    source_symbol: Some("serve".to_owned()),
                    target_module: "pkg.core".to_owned(),
                    target_symbol: Some("compute".to_owned()),
                    line: 4,
                },
            ]
        );
    }

    #[test]
    fn collect_project_facts_propagates_conflicting_architecture_error() {
        let files = [
            (
                ".omnicoach.toml",
                indoc::indoc! {r#"
                    [[architecture.components]]
                    name = "Core"
                    roots = ["core"]
                "#},
            ),
            (
                "src/architecture.rs",
                indoc::indoc! {r"
                    architecture_component!(Architecture);
                    define_architecture! {
                        Architecture => [],
                        Core => [],
                    }
                "},
            ),
            (
                "src/core.rs",
                "architecture_component!(Core);\npub fn compute() -> usize { 1 }\n",
            ),
        ];
        let root = PathBuf::from(MemoryFileSource::VIRTUAL_ROOT);
        let memory = MemoryFileSource::from_relative_files(&root, &files);
        let discovered = discover_project_sources_in(
            std::slice::from_ref(&root),
            &memory,
            &ProjectConfig::default(),
        )
        .expect("discover sources without eager architecture evaluation");

        let error = discovered
            .collect_project_facts("conflict", Language::Rust)
            .expect_err("conflicting architecture should fail");

        assert!(
            error
                .to_string()
                .contains("conflicting architecture declarations")
        );
    }

    #[test]
    fn discover_project_sources_merges_nested_python_subpackage_roots() {
        let files = [
            (".omnicoach.toml", ""),
            (
                "services/auth/auth_svc/__init__.py",
                "from .tokens import issue_token\n",
            ),
            (
                "services/auth/auth_svc/tokens.py",
                "def issue_token() -> str:\n    return 'ok'\n",
            ),
        ];

        let project = collect_virtual_facts(
            "multi_pkg",
            Language::Python,
            &files,
            &ProjectConfig::default(),
        );
        let module_paths: Vec<&str> = project
            .modules
            .iter()
            .map(|module| module.module_path.as_str())
            .collect();

        assert_eq!(module_paths, ["auth_svc", "auth_svc.tokens"]);
    }

    #[test]
    fn discover_project_sources_includes_explicitly_targeted_test_directory() {
        let files = [
            (
                "Cargo.toml",
                indoc::indoc! {r#"
                    [package]
                    name = "demo"
                    version = "0.1.0"
                "#},
            ),
            ("src/lib.rs", "pub fn prod() -> usize { 1 }\n"),
            (
                "tests/integration_helper.rs",
                "pub fn helper() -> usize { 2 }\n",
            ),
        ];
        let root = PathBuf::from(MemoryFileSource::VIRTUAL_ROOT);
        let tests_dir = root.join("tests");
        let memory = MemoryFileSource::from_relative_files(&root, &files);
        let config = ProjectConfig::default();

        let root_discovered =
            discover_project_sources_in(std::slice::from_ref(&root), &memory, &config)
                .expect("discover root");
        let tests_discovered =
            discover_project_sources_in(std::slice::from_ref(&tests_dir), &memory, &config)
                .expect("discover explicit tests dir");
        let root_paths: Vec<&str> = root_discovered
            .files
            .iter()
            .map(|file| file.relative_path.as_str())
            .collect();
        let test_paths: Vec<&str> = tests_discovered
            .files
            .iter()
            .map(|file| file.relative_path.as_str())
            .collect();

        assert_eq!(root_paths, ["src/lib.rs"]);
        assert_eq!(test_paths, ["tests/integration_helper.rs"]);
    }

    #[test]
    fn derive_project_title_normalizes_parent_dir_components() {
        let nested_parent = [PathBuf::from("services/auth/..")];
        let absolute_parent = [PathBuf::from("/workspace/crates/engine/..")];
        let bare_parent = [PathBuf::from("..")];

        assert_eq!(derive_project_title(&nested_parent), "services");
        assert_eq!(derive_project_title(&absolute_parent), "crates");
        assert_eq!(
            derive_project_title_with_cwd(
                &bare_parent,
                Some(Path::new("/workspace/omnicoach/src"))
            ),
            "omnicoach"
        );
    }

    fn collect_virtual_facts(
        title: &str,
        lang: Language,
        files: &[(&str, &str)],
        config: &ProjectConfig,
    ) -> ProjectFacts {
        let root = PathBuf::from(MemoryFileSource::VIRTUAL_ROOT);
        let memory = MemoryFileSource::from_relative_files(&root, files);
        let discovered = discover_project_sources_in(std::slice::from_ref(&root), &memory, config)
            .expect("discover virtual project sources");
        discovered
            .collect_project_facts(title, lang)
            .expect("collect virtual project facts")
    }
}
