//! Multi-file project abstraction facts, source discovery, and cross-module symbol resolution (`CodeLintGraph`).

use crate::code_lint::ast::python::collect_path_references;
use crate::code_lint::ast::rust::{ExternalModDeclaration, summarize_rust_file};
use crate::code_lint::ast::{
    DeclarationScopeKind, IntraFileReference, ParsedFile, ScopeFacts,
    collect_intra_file_references, collect_scope_facts, enclosing_scope_symbol,
};
use crate::code_lint::project::{
    ArchitectureConfig, ArchitectureSpec, DiskFileSource, FileSource as _, MemoryFileSource,
    ProjectIndex, ProjectLayout, ResolvedPythonTarget, is_python_package_init, is_rust_crate_root,
    normalize_relative_path, resolve_relative_python_module, resolve_rust_reference_path,
    rust_module_path,
};
use crate::config::Config;
use crate::diagnostic::Language;
use std::collections::{BTreeSet, HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

const INIT_MODULE: &str = "__init__";
const LIB_MODULE: &str = "lib";
const MAX_REEXPORT_HOPS: usize = 4;
/// Multi-file project facts across a Rust crate or Python package.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ProjectFacts {
    /// Human-readable project or package name.
    pub(super) title: String,
    /// True if architectural components and their allowed dependencies are explicitly declared
    /// (for example via `define_architecture!` + `architecture_component!`).
    pub(super) has_declared_architecture: bool,
    /// Architectural components (declared or inferred from top-level directory / package structure).
    pub(super) components: Vec<ComponentFact>,
    /// Parsed modules in deterministic path order.
    pub(super) modules: Vec<ModuleFact>,
    /// Resolved cross-module reference edges (`source_module -> target_module`).
    pub(super) cross_module_refs: Vec<CrossModuleRefFact>,
}

/// An architectural component and its declared dependency list (empty when inferred).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ComponentFact {
    /// Component identifier (e.g. `"CodeLintAst"` or `"zoneinfo"`).
    pub(super) name: String,
    /// Directly declared dependency component names (when [`ProjectFacts::has_declared_architecture`]
    /// is `true`).
    pub(super) declared_dependencies: Vec<String>,
}

/// One module file within [`ProjectFacts`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ModuleFact {
    /// Canonical module path (`"code_lint::ast"` or `"zoneinfo._tzpath"`).
    pub(super) module_path: String,
    /// Relative source file path (`"src/code_lint/ast.rs"`).
    pub(super) path: String,
    /// Owning component name.
    pub(super) component: String,
    /// Per-scope facts inside this module (`scopes[0]` is the module scope).
    pub(super) scopes: Vec<ScopeFacts>,
    /// Intra-file references between module declarations and methods in this module.
    pub(super) intra_file_refs: Vec<IntraFileReference>,
}

/// A resolved reference from one module (and optional top-level item) to another module in the
/// same project.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct CrossModuleRefFact {
    /// Referring module path.
    pub(super) source_module: String,
    /// Referring top-level symbol in `source_module`, when the reference lies inside a top-level
    /// declaration.
    pub(super) source_symbol: Option<String>,
    /// Target module path.
    pub(super) target_module: String,
    /// Referenced top-level symbol in `target_module`, when resolved to a specific item.
    pub(super) target_symbol: Option<String>,
    /// 1-based line number of the reference in `source_module`.
    pub(super) line: usize,
}

/// Collects [`ProjectFacts`] from an in-memory slice of `(relative_path, source_text)` pairs.
#[cfg(test)]
#[must_use]
pub(super) fn collect_project_facts_from_sources(
    title: &str,
    lang: Language,
    files: &[(&str, &str)],
) -> ProjectFacts {
    collect_project_facts_with_config(title, lang, files, None)
}

/// Collects [`ProjectFacts`] from an in-memory slice of `(relative_path, source_text)` pairs,
/// honoring an optional `.omnicoach.toml` [`ArchitectureConfig`].
#[must_use]
pub(super) fn collect_project_facts_with_config(
    title: &str,
    lang: Language,
    files: &[(&str, &str)],
    architecture: Option<&ArchitectureConfig>,
) -> ProjectFacts {
    let root = Path::new(MemoryFileSource::VIRTUAL_ROOT);
    let configured_roots: Vec<PathBuf> = architecture
        .map(|config| config.source_roots.iter().map(PathBuf::from).collect())
        .unwrap_or_default();
    let memory = MemoryFileSource::from_relative_files(root, files);
    let layout = ProjectLayout::discover(root, &memory, &configured_roots);
    let base_index = ProjectIndex::new(layout.clone(), &memory);
    let index = match architecture {
        Some(configured) => base_index.with_architecture_config(Some(configured.clone())),
        None => base_index,
    };
    match lang {
        Language::Rust => collect_rust_project_facts(title, files, &layout, &index),
        Language::Python => collect_python_project_facts(title, files, &layout, &index),
    }
}

fn collect_rust_project_facts(
    title: &str,
    files: &[(&str, &str)],
    layout: &ProjectLayout,
    index: &ProjectIndex<'_>,
) -> ProjectFacts {
    let entries: Vec<RustModuleEntry> = files
        .iter()
        .map(|&(path, source)| parse_rust_module(path, source, layout))
        .collect();

    let spec = index.architecture_spec();
    let InitialComponents {
        mut components,
        mut seen_components,
        has_declared_architecture,
    } = initial_declared_components(spec);
    let mut modules = Vec::new();

    for entry in &entries {
        if entry.is_crate_root && !entry.has_non_namespace_items {
            continue;
        }
        let component = spec
            .and_then(|resolved| {
                resolved.resolve_component_of_module(&entry.module_path, Language::Rust)
            })
            .map_or_else(
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
            intra_file_refs: entry.intra_file_refs.clone(),
        });
    }

    let cross_module_refs =
        resolve_rust_cross_module_refs(&entries, layout.rust_crate_name.as_deref());
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
    external_mods: Vec<ExternalModDeclaration>,
    visible_reexports: Vec<String>,
    has_non_namespace_items: bool,
    references: Vec<(usize, Option<String>, String)>,
    scopes: Vec<ScopeFacts>,
    intra_file_refs: Vec<IntraFileReference>,
}

fn parse_rust_module(path: &str, source: &str, layout: &ProjectLayout) -> RustModuleEntry {
    let full_path = Path::new(MemoryFileSource::VIRTUAL_ROOT).join(path);
    let module_path = layout
        .rust_module_of_path(&full_path)
        .unwrap_or_else(|| rust_module_path(path));
    let is_crate_root = is_rust_crate_root(&module_path);
    let parsed = ParsedFile::rust(source);
    let summary = summarize_rust_file(&parsed);
    let external_mods = summary.external_mods;
    let visible_reexports = summary
        .visible_uses
        .into_iter()
        .flat_map(|decl| decl.target_paths)
        .collect();
    let has_non_namespace_items = summary
        .non_namespace_items
        .iter()
        .any(|(_, text)| !text.starts_with("architecture_component!"));
    let scopes = collect_scope_facts(&parsed);
    let intra_file_refs = collect_intra_file_references(&parsed, &scopes);
    let references = summary
        .referenced_paths
        .into_iter()
        .map(|reference| {
            let enclosing = enclosing_scope_symbol(&parsed, &scopes, reference.line);
            (reference.line, enclosing, reference.raw_path)
        })
        .collect();
    RustModuleEntry {
        path: path.to_owned(),
        module_path,
        is_crate_root,
        external_mods,
        visible_reexports,
        has_non_namespace_items,
        references,
        scopes,
        intra_file_refs,
    }
}

fn resolve_rust_cross_module_refs(
    entries: &[RustModuleEntry],
    crate_name: Option<&str>,
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
    let lib_root_names: HashSet<&str> = entries
        .iter()
        .find(|entry| entry.module_path == LIB_MODULE)
        .into_iter()
        .flat_map(|lib_entry| {
            lib_entry
                .external_mods
                .iter()
                .map(|decl| decl.name.as_str())
                .chain(
                    lib_entry
                        .visible_reexports
                        .iter()
                        .filter_map(|target| target.rsplit("::").next()),
                )
                .chain(
                    module_symbols
                        .get(LIB_MODULE)
                        .into_iter()
                        .flatten()
                        .map(String::as_str),
                )
        })
        .collect();
    let reexports = collect_rust_reexports(entries, &lib_root_names, crate_name);

    let mut refs = Vec::new();
    for entry in entries {
        for &(line, ref source_symbol, ref raw_path) in &entry.references {
            let canonical = normalize_rust_reference_path(
                &entry.module_path,
                &entry.external_mods,
                &lib_root_names,
                raw_path,
                crate_name,
            );
            if let Some(target) =
                resolve_rust_target(&canonical, &known_modules, &module_symbols, &reexports)
                && target.module != entry.module_path
            {
                refs.push(CrossModuleRefFact {
                    source_module: entry.module_path.clone(),
                    source_symbol: source_symbol.clone(),
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
    crate_name: Option<&str>,
) -> HashMap<(String, String), String> {
    let mut reexports = HashMap::new();
    for entry in entries {
        for target in &entry.visible_reexports {
            let canonical = normalize_rust_reference_path(
                &entry.module_path,
                &entry.external_mods,
                lib_root_names,
                target,
                crate_name,
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
    external_mods: &[ExternalModDeclaration],
    lib_root_names: &HashSet<&str>,
    raw_path: &str,
    crate_name: Option<&str>,
) -> String {
    let resolved =
        resolve_rust_reference_path(enclosing_module, external_mods, raw_path, crate_name);
    if resolved.canonical_path != raw_path {
        return resolved.canonical_path;
    }
    let first_segment = raw_path.split("::").next().unwrap_or(raw_path);
    if crate_name.is_none()
        && let Some((_, rest)) = raw_path.split_once("::")
    {
        let next_segment = rest.split("::").next().unwrap_or(rest);
        if !matches!(first_segment, "std" | "core" | "alloc")
            && !external_mods.iter().any(|decl| decl.name == first_segment)
            && lib_root_names.contains(next_segment)
        {
            return rest.to_owned();
        }
    }
    resolved.canonical_path
}

fn collect_python_project_facts(
    title: &str,
    files: &[(&str, &str)],
    layout: &ProjectLayout,
    index: &ProjectIndex<'_>,
) -> ProjectFacts {
    let entries: Vec<PythonModuleEntry> = files
        .iter()
        .map(|&(path, source)| parse_python_module(path, source, layout))
        .collect();
    let cross_module_refs = resolve_python_cross_module_refs(&entries, index);
    let spec = index.architecture_spec();
    let InitialComponents {
        mut components,
        mut seen_components,
        has_declared_architecture,
    } = initial_declared_components(spec);
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
            intra_file_refs: entry.intra_file_refs,
        });
    }

    ProjectFacts {
        title: title.to_owned(),
        has_declared_architecture,
        components,
        modules,
        cross_module_refs,
    }
}

struct PythonModuleEntry {
    path: String,
    module_path: String,
    is_package_init: bool,
    references: Vec<(usize, Option<String>, String)>,
    scopes: Vec<ScopeFacts>,
    intra_file_refs: Vec<IntraFileReference>,
}

fn parse_python_module(path: &str, source: &str, layout: &ProjectLayout) -> PythonModuleEntry {
    let is_package_init = is_python_package_init(Path::new(path));
    let full_path = Path::new(MemoryFileSource::VIRTUAL_ROOT).join(path);
    let module_path = layout
        .python_module_of_path(&full_path)
        .unwrap_or_else(|| INIT_MODULE.to_owned());
    let parsed = ParsedFile::new(source, Language::Python);
    let scopes = collect_scope_facts(&parsed);
    let intra_file_refs = collect_intra_file_references(&parsed, &scopes);
    let references = collect_path_references(&parsed)
        .into_iter()
        .map(|(line, raw_path)| {
            let enclosing = enclosing_scope_symbol(&parsed, &scopes, line);
            (line, enclosing, raw_path)
        })
        .collect();
    PythonModuleEntry {
        path: path.to_owned(),
        module_path,
        is_package_init,
        references,
        scopes,
        intra_file_refs,
    }
}

struct InitialComponents {
    components: Vec<ComponentFact>,
    seen_components: BTreeSet<String>,
    has_declared_architecture: bool,
}

fn initial_declared_components(spec: Option<&ArchitectureSpec>) -> InitialComponents {
    let components: Vec<ComponentFact> = spec
        .into_iter()
        .flat_map(|resolved| &resolved.components)
        .map(|comp| ComponentFact {
            name: comp.name.clone(),
            declared_dependencies: comp.depends_on.clone(),
        })
        .collect();
    let has_declared_architecture = !components.is_empty();
    let seen_components: BTreeSet<String> =
        components.iter().map(|comp| comp.name.clone()).collect();
    InitialComponents {
        components,
        seen_components,
        has_declared_architecture,
    }
}

fn python_component_name(module_path: &str, fallback_title: &str) -> String {
    module_path
        .split('.')
        .next()
        .filter(|segment| !segment.is_empty() && *segment != INIT_MODULE)
        .unwrap_or(fallback_title)
        .to_owned()
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
        for &(line, ref source_symbol, ref raw_path) in &entry.references {
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
                resolve_python_target(&normalized, &known_modules, &module_symbols, index)
                && target.module != entry.module_path
            {
                refs.push(CrossModuleRefFact {
                    source_module: entry.module_path.clone(),
                    source_symbol: source_symbol.clone(),
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

struct ResolvedModuleTarget {
    module: String,
    symbol: Option<String>,
}

fn resolve_rust_target(
    canonical: &str,
    known_modules: &HashSet<&str>,
    module_symbols: &HashMap<&str, HashSet<String>>,
    reexports: &HashMap<(String, String), String>,
) -> Option<ResolvedModuleTarget> {
    if canonical.is_empty() {
        return None;
    }
    let initial = split_module_and_segments(canonical, "::", known_modules).or_else(|| {
        if known_modules.contains(LIB_MODULE) {
            let mut segments = canonical.split("::").filter(|segment| !segment.is_empty());
            let first = segments.next().map(str::to_owned)?;
            let member = segments.next().map(str::to_owned);
            let is_known_in_lib = module_symbols
                .get(LIB_MODULE)
                .is_some_and(|set| set.contains(&first))
                || reexports.contains_key(&(LIB_MODULE.to_owned(), first.clone()));
            is_known_in_lib.then(|| SplitModulePath {
                module: LIB_MODULE.to_owned(),
                symbol: Some(first),
                member,
            })
        } else {
            None
        }
    })?;
    let mut resolved_module = initial.module;
    let Some(mut resolved_first) = initial.symbol else {
        return Some(ResolvedModuleTarget {
            module: resolved_module,
            symbol: None,
        });
    };
    for _ in 0..MAX_REEXPORT_HOPS {
        let has_symbol = module_symbols
            .get(resolved_module.as_str())
            .is_some_and(|set| set.contains(&resolved_first));
        if has_symbol {
            break;
        }
        let Some(reexported) = reexports.get(&(resolved_module.clone(), resolved_first.clone()))
        else {
            break;
        };
        let Some(next) = split_module_and_segments(reexported, "::", known_modules) else {
            break;
        };
        resolved_module = next.module;
        if let Some(next_symbol) = next.symbol {
            resolved_first = next_symbol;
        } else {
            return Some(ResolvedModuleTarget {
                module: resolved_module,
                symbol: None,
            });
        }
    }
    if let Some(member) = initial.member {
        let method_candidate = format!("{resolved_first}::{member}");
        if module_symbols
            .get(resolved_module.as_str())
            .is_some_and(|set| set.contains(&method_candidate))
        {
            return Some(ResolvedModuleTarget {
                module: resolved_module,
                symbol: Some(method_candidate),
            });
        }
    }
    Some(ResolvedModuleTarget {
        module: resolved_module,
        symbol: Some(resolved_first),
    })
}

fn resolve_python_target(
    normalized: &str,
    known_modules: &HashSet<&str>,
    module_symbols: &HashMap<&str, HashSet<String>>,
    index: &ProjectIndex<'_>,
) -> Option<ResolvedModuleTarget> {
    let mut target = split_module_and_segments(normalized, ".", known_modules)?;
    if let Some(ref first) = target.symbol {
        let defined_in_module = module_symbols
            .get(target.module.as_str())
            .is_some_and(|set| set.contains(first));
        if !defined_in_module
            && let ResolvedPythonTarget::Symbol { module, name } =
                index.resolve_python_written_name(&target.module, first)
        {
            target.module = module;
            target.symbol = Some(name);
        }
    }
    if let (Some(first), Some(member)) = (&target.symbol, &target.member) {
        let method_candidate = format!("{first}::{member}");
        if module_symbols
            .get(target.module.as_str())
            .is_some_and(|set| set.contains(&method_candidate))
        {
            return Some(ResolvedModuleTarget {
                module: target.module,
                symbol: Some(method_candidate),
            });
        }
    }
    Some(ResolvedModuleTarget {
        module: target.module,
        symbol: target.symbol,
    })
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

struct SplitModulePath {
    module: String,
    symbol: Option<String>,
    member: Option<String>,
}

fn split_module_and_segments(
    path: &str,
    separator: &str,
    known_modules: &HashSet<&str>,
) -> Option<SplitModulePath> {
    if known_modules.contains(path) {
        return Some(SplitModulePath {
            module: path.to_owned(),
            symbol: None,
            member: None,
        });
    }
    let mut current = path;
    while let Some((prefix, _)) = current.rsplit_once(separator) {
        if known_modules.contains(prefix) {
            let remainder = &path[prefix.len() + separator.len()..];
            let mut segments = remainder
                .split(separator)
                .filter(|segment| !segment.is_empty());
            let symbol = segments.next().map(str::to_owned);
            let member = segments.next().map(str::to_owned);
            return Some(SplitModulePath {
                module: prefix.to_owned(),
                symbol,
                member,
            });
        }
        current = prefix;
    }
    None
}

const CURRENT_DIR_PREFIX: &str = "./";
const SRC_DIR_NAME: &str = "src";

/// A project source file discovered on disk, paired with its normalized relative path and language.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct CollectedSourceFile {
    /// Forward-slash-normalized path relative to the project root.
    pub(super) relative_path: String,
    /// Detected source language (`Rust` or `Python`).
    pub(super) lang: Language,
    /// Full UTF-8 source text.
    pub(super) source: String,
}

/// Collects all non-excluded `.rs` and `.py` source files under `paths` in deterministic path order.
///
/// # Errors
///
/// Returns an error if an explicitly passed file path cannot be read from disk.
pub(super) fn collect_project_source_files(
    paths: &[PathBuf],
    config: &Config,
) -> anyhow::Result<Vec<CollectedSourceFile>> {
    let mut collected = Vec::new();
    for path in paths {
        if path.is_file() {
            let Some(lang) = Language::from_path(path) else {
                continue;
            };
            let source = fs::read_to_string(path)?;
            let configured_roots = config.project.configured_source_roots();
            let layout = ProjectLayout::discover(path, &DiskFileSource, &configured_roots);
            let resolved_path = layout.resolve_query_path(path);
            let relative_path = resolved_path
                .strip_prefix(&layout.project_root)
                .map_or_else(
                    |_| normalize_relative_path(path, None),
                    |stripped| stripped.to_string_lossy().replace('\\', "/"),
                );
            collected.push(CollectedSourceFile {
                relative_path,
                lang,
                source,
            });
        } else if path.is_dir() {
            collect_directory_source_files(path, config, &mut collected);
        }
    }
    collected.sort_by(|left, right| left.relative_path.cmp(&right.relative_path));
    collected.dedup_by(|left, right| left.relative_path == right.relative_path);
    Ok(collected)
}

/// Resolves `paths` (defaulting to `"."` when empty) and verifies that each path exists on disk.
///
/// # Errors
///
/// Returns an error if any resolved path does not exist.
pub(super) fn resolve_effective_paths(paths: &[PathBuf]) -> anyhow::Result<Vec<PathBuf>> {
    let effective = if paths.is_empty() {
        vec![PathBuf::from(".")]
    } else {
        paths.to_vec()
    };
    for path in &effective {
        if !path.exists() {
            anyhow::bail!("Path does not exist: {}", path.display());
        }
    }
    Ok(effective)
}

/// Derives a human-readable project title from `paths` (falling back to the current directory name).
#[must_use]
pub(super) fn derive_project_title(paths: &[PathBuf]) -> String {
    if let [only] = paths {
        let clean = only.strip_prefix(CURRENT_DIR_PREFIX).unwrap_or(only);
        if clean != Path::new(".")
            && let Some(name) = clean.file_name().and_then(|os| os.to_str())
            && !name.is_empty()
        {
            return name.to_owned();
        }
        if let Ok(cwd) = std::env::current_dir()
            && let Some(name) = cwd.file_name().and_then(|os| os.to_str())
            && !name.is_empty()
        {
            return name.to_owned();
        }
    }
    "project".to_owned()
}

fn collect_directory_source_files(dir: &Path, config: &Config, out: &mut Vec<CollectedSourceFile>) {
    let configured_roots = config.project.configured_source_roots();
    let layout = ProjectLayout::discover(dir, &DiskFileSource, &configured_roots);
    let resolved_dir = layout.resolve_query_path(dir);
    let walk_root = if resolved_dir == layout.project_root
        && layout.rust_source_root != layout.project_root
        && DiskFileSource.is_file(&layout.project_root.join("Cargo.toml"))
    {
        dir.join(SRC_DIR_NAME)
    } else {
        dir.to_path_buf()
    };
    let normalization_base = layout
        .python_source_roots
        .first()
        .filter(|py_root| resolved_dir.starts_with(py_root.as_path()) && resolved_dir != **py_root)
        .map_or(layout.project_root.as_path(), PathBuf::as_path);

    for entry in ignore::WalkBuilder::new(&walk_root)
        .require_git(false)
        .build()
        .flatten()
    {
        let entry_path = entry.path();
        if !entry_path.is_file() || config.is_test_path(entry_path) {
            continue;
        }
        let Some(lang) = Language::from_path(entry_path) else {
            continue;
        };
        let Ok(source) = fs::read_to_string(entry_path) else {
            continue;
        };
        let resolved_entry = layout.resolve_query_path(entry_path);
        let relative_path = resolved_entry.strip_prefix(normalization_base).map_or_else(
            |_| normalize_relative_path(entry_path, Some(dir)),
            |stripped| stripped.to_string_lossy().replace('\\', "/"),
        );
        out.push(CollectedSourceFile {
            relative_path,
            lang,
            source,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rust_project_facts_resolve_components_and_cross_module_symbols() {
        let files = [
            (
                "src/lib.rs",
                indoc::indoc! {r"
                    mod architecture;
                    mod core;
                    mod runner;
                "},
            ),
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

        let project = collect_project_facts_from_sources("sample_methods", Language::Rust, &files);

        assert!(project.cross_module_refs.iter().any(|reference| {
            reference.source_module == "runner"
                && reference.source_symbol.as_deref() == Some("Runner::execute")
                && reference.target_module == "core"
                && reference.target_symbol.as_deref() == Some("Parser::parse")
        }));
    }

    #[test]
    fn python_project_facts_honor_declared_toml_architecture() {
        let files = [
            (
                "pkg/domain.py",
                indoc::indoc! {"
                    def compute() -> int:
                        return 1
                "},
            ),
            (
                "pkg/api.py",
                indoc::indoc! {"
                    from pkg.domain import compute

                    def serve() -> int:
                        return compute()
                "},
            ),
        ];
        let architecture_config: ArchitectureConfig = toml::from_str(indoc::indoc! {r#"
            [[components]]
            name = "Domain"
            roots = ["pkg.domain"]
            depends-on = []

            [[components]]
            name = "Api"
            roots = ["pkg.api"]
            depends-on = ["Domain"]
        "#})
        .expect("parse architecture config");

        let project = collect_project_facts_with_config(
            "sample_py",
            Language::Python,
            &files,
            Some(&architecture_config),
        );

        assert!(project.has_declared_architecture);
        assert_eq!(
            project.components,
            vec![
                ComponentFact {
                    name: "Domain".to_owned(),
                    declared_dependencies: Vec::new(),
                },
                ComponentFact {
                    name: "Api".to_owned(),
                    declared_dependencies: vec!["Domain".to_owned()],
                },
            ]
        );
        assert_eq!(project.modules[0].component, "Domain");
        assert_eq!(project.modules[1].component, "Api");
    }

    #[test]
    fn python_project_facts_follow_reexports_via_project_index() {
        let files = [
            (
                "pkg/__init__.py",
                indoc::indoc! {"
                    from ._core import build_plan
                "},
            ),
            (
                "pkg/_core.py",
                indoc::indoc! {"
                    def build_plan() -> int:
                        return 1
                "},
            ),
            (
                "pkg/api.py",
                indoc::indoc! {"
                    from pkg import build_plan

                    def handle() -> int:
                        return build_plan()
                "},
            ),
        ];

        let project =
            collect_project_facts_from_sources("sample_reexport", Language::Python, &files);

        assert!(project.cross_module_refs.iter().any(|reference| {
            reference.source_module == "pkg.api"
                && reference.source_symbol.as_deref() == Some("handle")
                && reference.target_module == "pkg._core"
                && reference.target_symbol.as_deref() == Some("build_plan")
        }));
    }
}
