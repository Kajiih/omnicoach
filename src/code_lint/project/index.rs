//! Memoized cross-file module summary index (`ProjectIndex`).

use super::architecture::{
    ArchitectureError, ArchitectureSpec, FileArchitectureFindings, RUST_ARCHITECTURE_MODULE,
    evaluate_file_architecture_with_summary, evaluate_stray_rust_component_markers,
    resolve_architecture_spec,
};
use super::layout::{ProjectLayout, is_python_package_init, resolve_relative_python_module};
use super::source::FileSource;
use crate::code_lint::ast::ParsedFile;
use crate::code_lint::ast::python::{
    ExportedAll, OwnedDocstring, PythonClassInfo, PythonFileSummary, PythonImportBinding,
    summarize_python_file,
};
use crate::code_lint::ast::rust::{RustFileSummary, summarize_rust_module_header};
use crate::config::{ArchitectureConfig, CONFIG_FILE_NAME};
use crate::diagnostic::Language;
use rayon::prelude::*;
use serde::Deserialize;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

const PYTHON_EXTENSION: &str = "py";
const PYTHON_STUB_EXTENSION: &str = "pyi";
const RUST_EXTENSION: &str = "rs";
const MAX_REEXPORT_HOPS: usize = 8;
const MAX_STAR_IMPORTS_PER_QUERY: usize = 32;
const ARCHITECTURE_COMPONENT_CALL_PREFIX: &str = "architecture_component!";
const DEFINE_ARCHITECTURE_CALL_PREFIX: &str = "define_architecture!";

/// Unique identity of a top-level Python class within a project (`module` + source-order `class_index`).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PythonClassId {
    /// Canonical dotted module path (`"pkg.interfaces"`).
    pub module: String,
    /// 0-based index in [`PythonFileSummary::classes`].
    pub class_index: usize,
}

/// Deduplicated in-file and cross-module ancestor class identifiers resolved across the project MRO.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ResolvedClassAncestors {
    /// Indices of ancestor classes within the current file (`classes[..index]`), in MRO order.
    pub in_file: Vec<usize>,
    /// Cross-module ancestor class identifiers (`PythonClassId`) reached in the MRO.
    pub cross_module: HashSet<PythonClassId>,
    /// Union of public non-property method names declared on cross-module ancestor classes.
    pub cross_module_public_methods: HashSet<String>,
}

/// Inherited method docstring resolved in Python C3 MRO order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InheritedDocstringMatch {
    /// Name of the ancestor class declaring the docstring.
    pub ancestor_name: String,
    /// Owned docstring text of the ancestor method.
    pub docstring: OwnedDocstring,
}

/// Summary of a `Protocol` imported into a Python module.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportedProtocolSummary {
    /// Local bound name in the importing module.
    pub local_name: String,
    /// Canonical class identity of the `Protocol`.
    pub class_id: PythonClassId,
    /// Public non-property methods required by the `Protocol` and its extended `Protocol`s.
    pub counted_methods: HashSet<String>,
    /// Strict C3 MRO ancestor class identities of this `Protocol` (excluding `class_id` itself).
    pub ancestor_ids: HashSet<PythonClassId>,
}

/// Outcome of resolving a symbol reference in a Python module.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolvedPythonTarget {
    /// Resolved to a top-level symbol `name` defined in `module`.
    Symbol {
        /// Defining module path.
        module: String,
        /// Top-level symbol name in `module`.
        name: String,
    },
    /// Resolved to a project module or namespace package `module`.
    Module(String),
    /// Outside project source roots, cyclic, or exceeding resolution hop bounds.
    Unknown,
}

type PythonSlot = Arc<OnceLock<Option<Arc<PythonFileSummary>>>>;
type RustSlot = Arc<OnceLock<Option<Arc<RustFileSummary>>>>;

/// Memoized cross-file summary index for a single CLI, hook, or test invocation.
pub struct ProjectIndex<'s> {
    layout: ProjectLayout,
    source: &'s dyn FileSource,
    configured_architecture: ConfiguredArchitecture,
    python_slots: Mutex<HashMap<String, PythonSlot>>,
    rust_slots: Mutex<HashMap<String, RustSlot>>,
    all_rust_headers: OnceLock<BTreeMap<String, Arc<RustFileSummary>>>,
    resolved_architecture: OnceLock<Result<Option<ArchitectureSpec>, ArchitectureError>>,
    resolved_toml_architecture: OnceLock<Result<Option<ArchitectureSpec>, ArchitectureError>>,
    architecture_findings_cache: Mutex<HashMap<PathBuf, Arc<FileArchitectureFindings>>>,
}

impl<'s> ProjectIndex<'s> {
    /// Constructs an on-demand memoized [`ProjectIndex`] for `layout` and `source`.
    #[must_use]
    pub fn new(layout: ProjectLayout, source: &'s dyn FileSource) -> Self {
        Self {
            layout,
            source,
            configured_architecture: ConfiguredArchitecture::Unset,
            python_slots: Mutex::new(HashMap::new()),
            rust_slots: Mutex::new(HashMap::new()),
            all_rust_headers: OnceLock::new(),
            resolved_architecture: OnceLock::new(),
            resolved_toml_architecture: OnceLock::new(),
            architecture_findings_cache: Mutex::new(HashMap::new()),
        }
    }

    /// Attaches a pre-validated [`ArchitectureConfig`] from `.omnicoach.toml`.
    #[must_use]
    pub fn with_architecture_config(mut self, architecture: Option<ArchitectureConfig>) -> Self {
        self.configured_architecture = ConfiguredArchitecture::Loaded(architecture);
        self
    }

    /// Project layout used by this index.
    #[must_use]
    pub const fn layout(&self) -> &ProjectLayout {
        &self.layout
    }

    /// Merges additional Python source roots discovered from nested `pyproject.toml` files inside
    /// the same `project_root`.
    pub fn merge_python_source_roots(&mut self, additional: &[PathBuf]) {
        self.layout.merge_python_source_roots(additional);
    }

    /// Seeds the memoized summary for `path` from an already-parsed [`ParsedFile`], avoiding a
    /// second parse of linted target files when they are also imported by another module.
    pub fn seed_from_parsed(&self, path: &Path, parsed: &ParsedFile) {
        match parsed.lang() {
            Language::Python => {
                if let Some(module) = self.layout.python_module_of_path(path) {
                    let is_shadowed_by_stub = path
                        .extension()
                        .is_some_and(|extension| extension == PYTHON_EXTENSION)
                        && self.source.is_file(
                            &self
                                .layout
                                .resolve_query_path(path)
                                .with_extension(PYTHON_STUB_EXTENSION),
                        );
                    if !is_shadowed_by_stub {
                        let slot = self.python_slot(&module);
                        let _ = slot.get_or_init(|| Some(Arc::new(summarize_python_file(parsed))));
                    }
                }
            }
            Language::Rust => {
                if has_rust_architecture_macro_candidate(&parsed.source_text())
                    && let Some(module) = self.layout.rust_module_of_path(path)
                {
                    let slot = self.rust_slot(&module);
                    let _ =
                        slot.get_or_init(|| Some(Arc::new(summarize_rust_module_header(parsed))));
                }
            }
        }
    }

    /// Returns the memoized [`PythonFileSummary`] for `module`, loading it on demand if needed.
    #[must_use]
    pub fn python_summary(&self, module: &str) -> Option<Arc<PythonFileSummary>> {
        let slot = self.python_slot(module);
        slot.get_or_init(|| {
            let file_path = self.layout.python_module_file(module, self.source)?;
            let text = self.source.read_to_string(&file_path).ok()?;
            let parsed = ParsedFile::new(&text, Language::Python);
            Some(Arc::new(summarize_python_file(&parsed)))
        })
        .clone()
    }

    /// Returns the memoized [`RustFileSummary`] header for `module`, loading it on demand if needed.
    #[must_use]
    pub fn rust_summary(&self, module: &str) -> Option<Arc<RustFileSummary>> {
        let slot = self.rust_slot(module);
        slot.get_or_init(|| {
            let file_path = self.layout.rust_module_file(module, self.source)?;
            let text = self.source.read_to_string(&file_path).ok()?;
            let parsed = ParsedFile::rust(&text);
            Some(Arc::new(summarize_rust_module_header(&parsed)))
        })
        .clone()
    }

    /// Resolves a written Python expression (`Store`, `interfaces.Store`, `pkg.interfaces.Store`)
    /// from `from_module` across imports, submodules, `__all__`, and bounded `*` re-exports.
    #[must_use]
    pub fn resolve_python_written_name(
        &self,
        from_module: &str,
        written: &str,
    ) -> ResolvedPythonTarget {
        let mut visited = HashSet::new();
        self.resolve_python_name_bounded(from_module, written, 0, &mut visited)
    }

    /// Resolves a base-class expression of `from_module` (occurring at `before_class_index`, or `None`
    /// for top-level lookup) to a concrete [`PythonClassId`].
    #[must_use]
    pub fn resolve_python_class(
        &self,
        from_module: &str,
        written_base: &str,
        before_class_index: Option<usize>,
    ) -> Option<PythonClassId> {
        let summary = self.python_summary(from_module)?;
        if !written_base.contains('.') {
            let limit = before_class_index.unwrap_or(summary.classes.len());
            let clamped = limit.min(summary.classes.len());
            if let Some(position) = summary.classes[..clamped]
                .iter()
                .rposition(|class_summary| class_summary.name == written_base)
            {
                return Some(PythonClassId {
                    module: from_module.to_owned(),
                    class_index: position,
                });
            }
            if before_class_index.is_some()
                && summary.defined_names.contains(written_base)
                && !summary.imports.contains_key(written_base)
            {
                return None;
            }
        }
        let resolved_target = if written_base.contains('.') {
            self.resolve_python_written_name(from_module, written_base)
        } else {
            let mut visited = HashSet::new();
            if let Some(binding) = summary.imports.get(written_base) {
                self.resolve_import_binding(from_module, binding, 0, &mut visited)
            } else {
                self.resolve_via_star_imports(from_module, &summary, written_base, 0, &mut visited)
            }
        };
        match resolved_target {
            ResolvedPythonTarget::Symbol { module, name } => {
                let target_summary = self.python_summary(&module)?;
                let class_index = target_summary
                    .classes
                    .iter()
                    .rposition(|class_summary| class_summary.name == name)?;
                Some(PythonClassId {
                    module,
                    class_index,
                })
            }
            ResolvedPythonTarget::Module(_) | ResolvedPythonTarget::Unknown => None,
        }
    }

    /// Computes the Python C3 Method Resolution Order (`[self, ...ancestors]`) of `class_id`
    /// across modules.
    #[must_use]
    pub fn python_class_mro(&self, class_id: &PythonClassId) -> Vec<PythonClassId> {
        let mut active = HashSet::new();
        self.compute_c3_mro(class_id, &mut active)
    }

    /// Resolves the inherited docstring for `method_name` on `classes[index]` in C3 MRO order,
    /// combining in-file classes (`classes[..index]`) with cross-module classes resolved from
    /// `current_module`.
    #[must_use]
    pub fn resolve_inherited_method_docstring(
        &self,
        current_module: Option<&str>,
        classes: &[PythonClassInfo<'_>],
        index: usize,
        method_name: &str,
    ) -> Option<InheritedDocstringMatch> {
        for entry in self
            .class_mro_entries(current_module, classes, index)
            .into_iter()
            .skip(1)
        {
            match entry {
                ClassMroEntry::Local(ancestor_index) => {
                    let class_info = &classes[ancestor_index];
                    if let Some(docstring) = class_info.documented_method_docstring(method_name) {
                        return Some(InheritedDocstringMatch {
                            ancestor_name: class_info.name.clone(),
                            docstring: docstring.as_owned().clone(),
                        });
                    }
                }
                ClassMroEntry::Imported(ancestor_id) => {
                    if let Some(summary) = self.python_summary(&ancestor_id.module)
                        && let Some(class_summary) = summary.classes.get(ancestor_id.class_index)
                        && let Some(docstring) = class_summary.documented_methods.get(method_name)
                    {
                        return Some(InheritedDocstringMatch {
                            ancestor_name: class_summary.name.clone(),
                            docstring: docstring.clone(),
                        });
                    }
                }
            }
        }
        None
    }

    /// Resolves the ancestor class names of `classes[index]` in C3 MRO order across in-file
    /// classes (`classes[..index]`) and cross-module classes resolved from `current_module`.
    #[must_use]
    pub fn resolve_ancestor_class_names(
        &self,
        current_module: Option<&str>,
        classes: &[PythonClassInfo<'_>],
        index: usize,
    ) -> Vec<String> {
        self.class_mro_entries(current_module, classes, index)
            .into_iter()
            .skip(1)
            .filter_map(|entry| match entry {
                ClassMroEntry::Local(ancestor_index) => Some(classes[ancestor_index].name.clone()),
                ClassMroEntry::Imported(ancestor_id) => self
                    .python_summary(&ancestor_id.module)
                    .and_then(|summary| {
                        summary
                            .classes
                            .get(ancestor_id.class_index)
                            .map(|class_summary| class_summary.name.clone())
                    }),
            })
            .collect()
    }

    /// Resolves the strict C3 MRO ancestors of `classes[index]` across in-file classes
    /// (`classes[..index]`) and cross-module classes resolved from `current_module`.
    #[must_use]
    pub fn resolve_class_ancestors(
        &self,
        current_module: Option<&str>,
        classes: &[PythonClassInfo<'_>],
        index: usize,
    ) -> ResolvedClassAncestors {
        let mut ancestors = ResolvedClassAncestors::default();
        for entry in self
            .class_mro_entries(current_module, classes, index)
            .into_iter()
            .skip(1)
        {
            match entry {
                ClassMroEntry::Local(ancestor_index) => ancestors.in_file.push(ancestor_index),
                ClassMroEntry::Imported(ancestor_id) => {
                    if let Some(summary) = self.python_summary(&ancestor_id.module)
                        && let Some(class_summary) = summary.classes.get(ancestor_id.class_index)
                    {
                        ancestors
                            .cross_module_public_methods
                            .extend(class_summary.public_non_property_methods.iter().cloned());
                    }
                    ancestors.cross_module.insert(ancestor_id);
                }
            }
        }
        ancestors
    }

    /// Collects all `Protocol`s imported into `module` in deterministic local-name order.
    #[must_use]
    pub fn imported_protocols_in_module(&self, module: &str) -> Vec<ImportedProtocolSummary> {
        let Some(summary) = self.python_summary(module) else {
            return Vec::new();
        };
        let mut local_names: Vec<&String> = summary.imports.keys().collect();
        local_names.sort();
        let mut imported = Vec::new();
        for local_name in local_names {
            let Some(binding) = summary.imports.get(local_name) else {
                continue;
            };
            if binding.imported_name.is_none() {
                continue;
            }
            if let ResolvedPythonTarget::Symbol {
                module: target_module,
                name: target_name,
            } = self.resolve_import_binding(module, binding, 0, &mut HashSet::new())
                && let Some(target_summary) = self.python_summary(&target_module)
                && let Some(class_index) = target_summary
                    .classes
                    .iter()
                    .rposition(|class_summary| class_summary.name == target_name)
            {
                let class_summary = &target_summary.classes[class_index];
                if class_summary.is_protocol {
                    let class_id = PythonClassId {
                        module: target_module,
                        class_index,
                    };
                    let mro = self.python_class_mro(&class_id);
                    let counted_methods = self.protocol_counted_methods(&mro);
                    let ancestor_ids = mro.into_iter().skip(1).collect();
                    imported.push(ImportedProtocolSummary {
                        local_name: local_name.clone(),
                        class_id,
                        counted_methods,
                        ancestor_ids,
                    });
                }
            }
        }
        imported
    }

    /// Lazily resolves and caches the project's [`ArchitectureSpec`] (from `.omnicoach.toml`
    /// `[architecture]` or Rust `define_architecture!` / `architecture_component!` headers).
    #[must_use]
    pub fn architecture_spec(&self) -> Option<&ArchitectureSpec> {
        if let Some(resolved) = self.resolved_architecture.get() {
            return resolved.as_ref().ok()?.as_ref();
        }
        let resolved = (|| {
            let toml_config = self.toml_architecture_config()?;
            let rust_headers = self.rust_headers_map();
            resolve_architecture_spec(
                &self.layout.project_root,
                toml_config.as_ref(),
                rust_headers,
            )
        })();
        self.resolved_architecture
            .get_or_init(|| resolved)
            .as_ref()
            .ok()?
            .as_ref()
    }

    /// Returns the [`ArchitectureError`] encountered while resolving this project's
    /// [`ArchitectureSpec`], if an architecture evaluation was attempted and failed.
    #[must_use]
    pub fn architecture_error(&self) -> Option<&ArchitectureError> {
        if let Some(Err(error)) = self.resolved_architecture.get() {
            return Some(error);
        }
        if let Some(Err(error)) = self.resolved_toml_architecture.get() {
            return Some(error);
        }
        None
    }

    /// Evaluates (or returns cached) architectural conformance findings for `file` at `path`.
    #[must_use]
    pub fn file_architecture_findings(
        &self,
        file: &ParsedFile,
        path: &Path,
    ) -> Option<Arc<FileArchitectureFindings>> {
        let spec = match file.lang() {
            Language::Python => Some(self.python_architecture_spec()?),
            Language::Rust => {
                let resolved = self.architecture_spec();
                if self.architecture_error().is_some() {
                    return None;
                }
                if resolved.is_none()
                    && !file
                        .source_text()
                        .contains(ARCHITECTURE_COMPONENT_CALL_PREFIX)
                {
                    return None;
                }
                resolved
            }
        };
        {
            let guard = self
                .architecture_findings_cache
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if let Some(cached) = guard.get(path) {
                return Some(Arc::clone(cached));
            }
        }
        let normalized_path = self.layout.resolve_query_path(path);
        {
            let guard = self
                .architecture_findings_cache
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if let Some(cached) = guard.get(&normalized_path) {
                return Some(Arc::clone(cached));
            }
        }
        let evaluated = Arc::new(spec.map_or_else(
            || evaluate_stray_rust_component_markers(file, &normalized_path, &self.layout),
            |spec| {
                let cached_python_summary = if file.lang() == Language::Python {
                    self.layout
                        .python_module_of_path(&normalized_path)
                        .and_then(|module| {
                            let guard = self
                                .python_slots
                                .lock()
                                .unwrap_or_else(std::sync::PoisonError::into_inner);
                            guard
                                .get(&module)
                                .and_then(|slot| slot.get().and_then(Option::clone))
                        })
                } else {
                    None
                };
                evaluate_file_architecture_with_summary(
                    file,
                    &normalized_path,
                    &self.layout,
                    self.source,
                    spec,
                    cached_python_summary.as_deref(),
                )
            },
        ));
        {
            let mut guard = self
                .architecture_findings_cache
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if path != normalized_path {
                guard.insert(path.to_path_buf(), Arc::clone(&evaluated));
            }
            guard.insert(normalized_path, Arc::clone(&evaluated));
        }
        Some(evaluated)
    }

    fn python_architecture_spec(&self) -> Option<&ArchitectureSpec> {
        self.resolved_toml_architecture
            .get_or_init(|| {
                let Some(toml_config) = self.toml_architecture_config()? else {
                    return Ok(None);
                };
                let empty_headers = BTreeMap::new();
                resolve_architecture_spec(
                    &self.layout.project_root,
                    Some(&toml_config),
                    &empty_headers,
                )
            })
            .as_ref()
            .ok()?
            .as_ref()
    }

    /// Headers of the Rust modules the architecture spec reads: the `[architecture]` roots with
    /// `isolate-children`, plus, when the crate declares `define_architecture!`, every module
    /// that may declare `architecture_component!`.
    fn rust_headers_map(&self) -> &BTreeMap<String, Arc<RustFileSummary>> {
        if let Some(headers) = self.all_rust_headers.get() {
            return headers;
        }
        let headers = self.collect_rust_headers();
        self.all_rust_headers.get_or_init(|| headers)
    }

    fn collect_rust_headers(&self) -> BTreeMap<String, Arc<RustFileSummary>> {
        if !self.source.is_dir(&self.layout.rust_source_root) {
            return BTreeMap::new();
        }
        let mut headers: BTreeMap<String, Arc<RustFileSummary>> = self
            .toml_architecture_config()
            .ok()
            .flatten()
            .into_iter()
            .flat_map(|config| config.components)
            .filter(|component| component.isolate_children)
            .flat_map(|component| component.roots)
            .filter_map(|module| {
                let summary = self.rust_summary(&module)?;
                Some((module, summary))
            })
            .collect();
        if !self.declares_rust_architecture() {
            return headers;
        }
        let mut rust_files = Vec::new();
        collect_source_files_recursive(
            &self.layout.rust_source_root,
            RUST_EXTENSION,
            self.source,
            &mut rust_files,
        );
        rust_files.sort();
        rust_files.dedup();
        headers.par_extend(
            rust_files
                .par_iter()
                .filter_map(|path| self.parse_macro_candidate_header(path))
                .map(|entry| (entry.module, entry.summary)),
        );
        headers
    }

    fn parse_macro_candidate_header(&self, path: &Path) -> Option<ModuleHeaderEntry> {
        let module = self.layout.rust_module_of_path(path)?;
        let slot = self.rust_slot(&module);
        if let Some(cached) = slot.get() {
            return Some(ModuleHeaderEntry {
                module,
                summary: Arc::clone(cached.as_ref()?),
            });
        }
        let text = self.source.read_to_string(path).ok()?;
        if !has_rust_architecture_macro_candidate(&text) {
            return None;
        }
        let summary = slot
            .get_or_init(|| {
                let parsed = ParsedFile::rust(&text);
                Some(Arc::new(summarize_rust_module_header(&parsed)))
            })
            .clone()?;
        Some(ModuleHeaderEntry { module, summary })
    }

    /// Whether the crate declares `define_architecture!` in its `architecture` module
    /// (`src/architecture.rs` or `src/architecture/mod.rs`), the only module where it is
    /// recognized, so crates without one pay at most a single module probe instead of a crate-wide
    /// header scan.
    fn declares_rust_architecture(&self) -> bool {
        self.layout
            .rust_module_file(RUST_ARCHITECTURE_MODULE, self.source)
            .and_then(|path| self.source.read_to_string(&path).ok())
            .is_some_and(|text| text.contains(DEFINE_ARCHITECTURE_CALL_PREFIX))
            && self
                .rust_summary(RUST_ARCHITECTURE_MODULE)
                .is_some_and(|summary| !summary.defined_architecture.is_empty())
    }

    fn toml_architecture_config(&self) -> Result<Option<ArchitectureConfig>, ArchitectureError> {
        if let ConfiguredArchitecture::Loaded(ref configured) = self.configured_architecture {
            if let Some(config) = configured {
                config
                    .validate()
                    .map_err(|error| ArchitectureError::Parse(error.to_string()))?;
            }
            return Ok(configured.clone());
        }
        let config_path = self.layout.project_root.join(CONFIG_FILE_NAME);
        let Ok(content) = self.source.read_to_string(&config_path) else {
            return Ok(None);
        };
        let parsed: PartialConfigWithArchitecture = toml::from_str(&content)
            .map_err(|error| ArchitectureError::Parse(error.to_string()))?;
        if let Some(ref config) = parsed.architecture {
            config
                .validate()
                .map_err(|error| ArchitectureError::Parse(error.to_string()))?;
        }
        Ok(parsed.architecture)
    }

    fn protocol_counted_methods(&self, mro: &[PythonClassId]) -> HashSet<String> {
        let mut methods = HashSet::new();
        for ancestor_id in mro {
            if let Some(summary) = self.python_summary(&ancestor_id.module)
                && let Some(class_summary) = summary.classes.get(ancestor_id.class_index)
            {
                methods.extend(class_summary.public_non_property_methods.iter().cloned());
            }
        }
        methods
    }

    fn compute_c3_mro(
        &self,
        class_id: &PythonClassId,
        active: &mut HashSet<PythonClassId>,
    ) -> Vec<PythonClassId> {
        if !active.insert(class_id.clone()) {
            return Vec::new();
        }
        let Some(summary) = self.python_summary(&class_id.module) else {
            active.remove(class_id);
            return vec![class_id.clone()];
        };
        let Some(class_summary) = summary.classes.get(class_id.class_index) else {
            active.remove(class_id);
            return vec![class_id.clone()];
        };
        let direct_bases: Vec<PythonClassId> = class_summary
            .bases
            .iter()
            .filter(|base| !base.is_structural_marker)
            .filter_map(|base| {
                self.resolve_python_class(
                    &class_id.module,
                    &base.unsubscripted_name,
                    Some(class_id.class_index),
                )
            })
            .collect();
        if direct_bases.is_empty() {
            active.remove(class_id);
            return vec![class_id.clone()];
        }
        let mut sequences: Vec<Vec<PythonClassId>> = direct_bases
            .iter()
            .map(|base_id| {
                let mut mro = self.compute_c3_mro(base_id, active);
                mro.retain(|candidate| candidate != class_id);
                mro
            })
            .filter(|sequence| !sequence.is_empty())
            .collect();
        let filtered_direct: Vec<PythonClassId> = direct_bases
            .into_iter()
            .filter(|candidate| candidate != class_id)
            .collect();
        if !filtered_direct.is_empty() {
            sequences.push(filtered_direct);
        }
        active.remove(class_id);

        c3_merge(class_id.clone(), sequences)
    }

    fn class_mro_entries(
        &self,
        current_module: Option<&str>,
        classes: &[PythonClassInfo<'_>],
        index: usize,
    ) -> Vec<ClassMroEntry> {
        let summary_indices = self.summary_class_indices(current_module, classes);
        let mut active = HashSet::new();
        self.compute_local_class_mro(
            current_module,
            classes,
            &summary_indices,
            index,
            &mut active,
        )
    }

    /// Maps each of `classes` (every class of the file of `current_module`, in source order) to
    /// its index among the module-level classes of the module summary, or `None` for a class
    /// nested in a `def` or `class` body (or when the module has no summary).
    fn summary_class_indices(
        &self,
        current_module: Option<&str>,
        classes: &[PythonClassInfo<'_>],
    ) -> Vec<Option<usize>> {
        let summary = current_module.and_then(|module| self.python_summary(module));
        classes
            .iter()
            .map(|class| {
                summary
                    .as_ref()?
                    .classes
                    .iter()
                    .position(|candidate| candidate.name_span == class.name_node.span())
            })
            .collect()
    }

    /// Computes the C3 MRO of `classes[index]`. As at runtime, a simple base name refers to the
    /// last class of that name bound before the subclass; a module-level subclass only sees
    /// module-level classes, never classes nested in a `def` or `class` body.
    fn compute_local_class_mro(
        &self,
        current_module: Option<&str>,
        classes: &[PythonClassInfo<'_>],
        summary_indices: &[Option<usize>],
        index: usize,
        active: &mut HashSet<usize>,
    ) -> Vec<ClassMroEntry> {
        let self_entry = ClassMroEntry::Local(index);
        if index >= classes.len() || !active.insert(index) {
            return Vec::new();
        }
        let is_module_level = summary_indices[index].is_some();
        let module_level_classes_before = summary_indices[..index].iter().flatten().count();
        let mut direct_bases = Vec::new();
        for base in &classes[index].bases {
            if base.is_structural_marker() {
                continue;
            }
            let base_name = base.unsubscripted_name();
            if !base_name.contains('.')
                && let Some(local_index) = (0..index).rev().find(|&candidate| {
                    classes[candidate].name == base_name
                        && (!is_module_level || summary_indices[candidate].is_some())
                })
            {
                direct_bases.push(ClassMroEntry::Local(local_index));
                continue;
            }
            if let Some(module) = current_module
                && let Some(target_id) =
                    self.resolve_python_class(module, base_name, Some(module_level_classes_before))
                && (base_name.contains('.') || target_id.module != module)
            {
                direct_bases.push(ClassMroEntry::Imported(target_id));
            }
        }
        if direct_bases.is_empty() {
            active.remove(&index);
            return vec![self_entry];
        }
        let mut sequences: Vec<Vec<ClassMroEntry>> = direct_bases
            .iter()
            .map(|base_entry| {
                let mut mro = match base_entry {
                    ClassMroEntry::Local(base_index) => self.compute_local_class_mro(
                        current_module,
                        classes,
                        summary_indices,
                        *base_index,
                        active,
                    ),
                    ClassMroEntry::Imported(base_id) => self
                        .python_class_mro(base_id)
                        .into_iter()
                        .map(ClassMroEntry::Imported)
                        .collect(),
                };
                mro.retain(|entry| entry != &self_entry);
                mro
            })
            .filter(|sequence| !sequence.is_empty())
            .collect();
        let filtered_direct: Vec<ClassMroEntry> = direct_bases
            .into_iter()
            .filter(|entry| entry != &self_entry)
            .collect();
        if !filtered_direct.is_empty() {
            sequences.push(filtered_direct);
        }
        active.remove(&index);

        c3_merge(self_entry, sequences)
    }

    fn resolve_python_name_bounded(
        &self,
        from_module: &str,
        written: &str,
        depth: usize,
        visited: &mut HashSet<(String, String)>,
    ) -> ResolvedPythonTarget {
        if depth > MAX_REEXPORT_HOPS
            || !visited.insert((from_module.to_owned(), written.to_owned()))
        {
            return ResolvedPythonTarget::Unknown;
        }
        let Some(summary) = self.python_summary(from_module) else {
            return ResolvedPythonTarget::Unknown;
        };
        let mut parts = written.split('.');
        let Some(head) = parts.next() else {
            return ResolvedPythonTarget::Unknown;
        };
        let rest: Vec<&str> = parts.collect();

        if rest.is_empty() {
            if summary.defined_names.contains(head) {
                return ResolvedPythonTarget::Symbol {
                    module: from_module.to_owned(),
                    name: head.to_owned(),
                };
            }
            if let Some(binding) = summary.imports.get(head) {
                return self.resolve_import_binding(from_module, binding, depth + 1, visited);
            }
            return self.resolve_via_star_imports(from_module, &summary, head, depth + 1, visited);
        }

        let head_target = summary
            .imports
            .get(head)
            .map_or(ResolvedPythonTarget::Unknown, |binding| {
                self.resolve_import_binding(from_module, binding, depth + 1, visited)
            });
        let ResolvedPythonTarget::Module(base_module) = head_target else {
            return ResolvedPythonTarget::Unknown;
        };
        self.resolve_dotted_from_module(&summary, &base_module, &rest, depth + 1, visited)
    }

    fn resolve_import_binding(
        &self,
        from_module: &str,
        binding: &PythonImportBinding,
        depth: usize,
        visited: &mut HashSet<(String, String)>,
    ) -> ResolvedPythonTarget {
        if depth > MAX_REEXPORT_HOPS {
            return ResolvedPythonTarget::Unknown;
        }
        let is_package_init = self
            .layout
            .python_module_file(from_module, self.source)
            .is_some_and(|file_path| is_python_package_init(&file_path));
        let Some(target_module) = resolve_relative_python_module(
            from_module,
            is_package_init,
            binding.level,
            binding.module.as_deref(),
        ) else {
            return ResolvedPythonTarget::Unknown;
        };

        let Some(imported_name) = &binding.imported_name else {
            if self.python_summary(&target_module).is_some()
                || self
                    .layout
                    .is_python_namespace_dir(&target_module, self.source)
            {
                return ResolvedPythonTarget::Module(target_module);
            }
            return ResolvedPythonTarget::Unknown;
        };

        let candidate_submodule = if target_module.is_empty() {
            imported_name.clone()
        } else {
            format!("{target_module}.{imported_name}")
        };
        if let Some(module_summary) = self.python_summary(&target_module) {
            if module_summary.defined_names.contains(imported_name) {
                return ResolvedPythonTarget::Symbol {
                    module: target_module,
                    name: imported_name.clone(),
                };
            }
            if let Some(reexport) = module_summary.imports.get(imported_name) {
                return self.resolve_import_binding(&target_module, reexport, depth + 1, visited);
            }
            let via_star = self.resolve_via_star_imports(
                &target_module,
                &module_summary,
                imported_name,
                depth + 1,
                visited,
            );
            if via_star != ResolvedPythonTarget::Unknown {
                return via_star;
            }
        }
        if self.python_summary(&candidate_submodule).is_some()
            || self
                .layout
                .is_python_namespace_dir(&candidate_submodule, self.source)
        {
            return ResolvedPythonTarget::Module(candidate_submodule);
        }
        ResolvedPythonTarget::Unknown
    }

    fn resolve_dotted_from_module(
        &self,
        from_summary: &PythonFileSummary,
        base_module: &str,
        segments: &[&str],
        depth: usize,
        visited: &mut HashSet<(String, String)>,
    ) -> ResolvedPythonTarget {
        if segments.is_empty() {
            return ResolvedPythonTarget::Module(base_module.to_owned());
        }
        let mut cursor = base_module.to_owned();
        for (index, segment) in segments.iter().enumerate() {
            let next_module = format!("{cursor}.{segment}");
            let is_last = index + 1 == segments.len();
            let cursor_summary = self.python_summary(&cursor);
            let submodule_imported = from_summary.imported_submodules.contains(&next_module)
                || cursor_summary
                    .as_ref()
                    .is_some_and(|summary| summary.imported_submodules.contains(&next_module));
            let next_is_module = self.python_summary(&next_module).is_some()
                || self
                    .layout
                    .is_python_namespace_dir(&next_module, self.source);
            if !is_last {
                if let Some(ref module_summary) = cursor_summary {
                    if let Some(reexport) = module_summary.imports.get(*segment)
                        && let ResolvedPythonTarget::Module(target_mod) =
                            self.resolve_import_binding(&cursor, reexport, depth + 1, visited)
                    {
                        cursor = target_mod;
                        continue;
                    }
                    if let ResolvedPythonTarget::Module(target_mod) = self.resolve_via_star_imports(
                        &cursor,
                        module_summary,
                        segment,
                        depth + 1,
                        visited,
                    ) {
                        cursor = target_mod;
                        continue;
                    }
                }
                if submodule_imported && next_is_module {
                    cursor = next_module;
                    continue;
                }
                return ResolvedPythonTarget::Unknown;
            }
            if let Some(module_summary) = cursor_summary {
                if module_summary.defined_names.contains(*segment) {
                    return ResolvedPythonTarget::Symbol {
                        module: cursor,
                        name: (*segment).to_owned(),
                    };
                }
                if let Some(reexport) = module_summary.imports.get(*segment) {
                    return self.resolve_import_binding(&cursor, reexport, depth + 1, visited);
                }
                let via_star = self.resolve_via_star_imports(
                    &cursor,
                    &module_summary,
                    segment,
                    depth + 1,
                    visited,
                );
                if via_star != ResolvedPythonTarget::Unknown {
                    return via_star;
                }
            }
            if submodule_imported && next_is_module {
                return ResolvedPythonTarget::Module(next_module);
            }
            return ResolvedPythonTarget::Unknown;
        }
        ResolvedPythonTarget::Unknown
    }

    fn resolve_via_star_imports(
        &self,
        from_module: &str,
        summary: &PythonFileSummary,
        symbol: &str,
        depth: usize,
        visited: &mut HashSet<(String, String)>,
    ) -> ResolvedPythonTarget {
        if depth > MAX_REEXPORT_HOPS || symbol.starts_with('_') {
            return ResolvedPythonTarget::Unknown;
        }
        let is_package_init = self
            .layout
            .python_module_file(from_module, self.source)
            .is_some_and(|file_path| is_python_package_init(&file_path));
        for star in summary
            .star_imports
            .iter()
            .take(MAX_STAR_IMPORTS_PER_QUERY)
            .rev()
        {
            let Some(star_module) = resolve_relative_python_module(
                from_module,
                is_package_init,
                star.level,
                star.module.as_deref(),
            ) else {
                continue;
            };
            let Some(star_summary) = self.python_summary(&star_module) else {
                continue;
            };
            if let ExportedAll::Explicit(allowed) = &star_summary.exported_all
                && !allowed.iter().any(|item| item == symbol)
            {
                continue;
            }
            let resolved =
                self.resolve_python_name_bounded(&star_module, symbol, depth + 1, visited);
            if resolved != ResolvedPythonTarget::Unknown {
                return resolved;
            }
        }
        ResolvedPythonTarget::Unknown
    }

    fn python_slot(&self, module: &str) -> PythonSlot {
        let mut guard = self
            .python_slots
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(existing) = guard.get(module) {
            return Arc::clone(existing);
        }
        let slot = Arc::new(OnceLock::new());
        guard.insert(module.to_owned(), Arc::clone(&slot));
        slot
    }

    fn rust_slot(&self, module: &str) -> RustSlot {
        let mut guard = self
            .rust_slots
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(existing) = guard.get(module) {
            return Arc::clone(existing);
        }
        let slot = Arc::new(OnceLock::new());
        guard.insert(module.to_owned(), Arc::clone(&slot));
        slot
    }
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
struct PartialConfigWithArchitecture {
    architecture: Option<ArchitectureConfig>,
}

fn collect_source_files_recursive(
    directory: &Path,
    expected_extension: &str,
    source: &dyn FileSource,
    out: &mut Vec<PathBuf>,
) {
    let Ok(entries) = source.read_dir(directory) else {
        return;
    };
    for entry in entries {
        let skip = entry
            .file_name()
            .and_then(|os_name| os_name.to_str())
            .is_some_and(|name| {
                name.starts_with('.')
                    || matches!(name, "target" | "__pycache__" | "node_modules" | "venv")
            });
        if skip {
            continue;
        }
        if source.is_dir(&entry) {
            collect_source_files_recursive(&entry, expected_extension, source, out);
        } else if entry
            .extension()
            .is_some_and(|extension| extension == expected_extension)
            && source.is_file(&entry)
        {
            out.push(entry);
        }
    }
}

enum ConfiguredArchitecture {
    Unset,
    Loaded(Option<ArchitectureConfig>),
}

struct ModuleHeaderEntry {
    module: String,
    summary: Arc<RustFileSummary>,
}

/// Cheap lexical prefilter for Rust files that may declare architecture macros: `true` whenever
/// the text mentions `architecture_component!` or `define_architecture!` anywhere. The body-free
/// header parse (`summarize_rust_module_header`) then decides which mentions are real top-level,
/// non-test declarations, so the prefilter never drops a declaring file.
fn has_rust_architecture_macro_candidate(text: &str) -> bool {
    text.contains(ARCHITECTURE_COMPONENT_CALL_PREFIX)
        || text.contains(DEFINE_ARCHITECTURE_CALL_PREFIX)
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum ClassMroEntry {
    Local(usize),
    Imported(PythonClassId),
}

/// Linearizes `head` followed by the C3 merge of `sequences` (the MROs of the direct bases, then
/// the direct bases themselves). When no consistent order exists, Python raises `TypeError` and
/// the class cannot exist, so the result is `head` alone: no ancestor, hence no inherited claim.
fn c3_merge<T: Clone + PartialEq>(head: T, mut sequences: Vec<Vec<T>>) -> Vec<T> {
    let mut out = vec![head];
    loop {
        sequences.retain(|sequence| !sequence.is_empty());
        if sequences.is_empty() {
            return out;
        }
        let Some(chosen) = sequences.iter().find_map(|sequence| {
            let head = &sequence[0];
            let in_any_tail = sequences.iter().any(|other| other[1..].contains(head));
            (!in_any_tail).then(|| head.clone())
        }) else {
            out.truncate(1);
            return out;
        };
        for sequence in &mut sequences {
            if sequence.first() == Some(&chosen) {
                sequence.remove(0);
            }
        }
        out.push(chosen);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::code_lint::ast::python::extract_classes;
    use crate::code_lint::project::source::MemoryFileSource;

    #[test]
    fn architecture_spec_finds_component_declared_after_a_test_gated_item() {
        let root = Path::new("/repo");
        let source = MemoryFileSource::from_relative_files(
            root,
            &[
                ("Cargo.toml", "[package]\nname = \"demo\"\n"),
                ("src/lib.rs", "mod architecture;\nmod core;\n"),
                (
                    "src/architecture.rs",
                    indoc::indoc! {r"
                        define_architecture! {
                            Core => [],
                        }
                    "},
                ),
                (
                    "src/core.rs",
                    indoc::indoc! {r"
                        #[cfg(test)]
                        mod tests;

                        architecture_component!(Core);
                    "},
                ),
            ],
        );
        let index = ProjectIndex::new(ProjectLayout::discover(root, &source, &[]), &source);

        let spec = index
            .architecture_spec()
            .expect("define_architecture! should yield a spec");

        assert_eq!(
            spec.components
                .iter()
                .map(|component| (component.name.as_str(), component.roots.clone()))
                .collect::<Vec<_>>(),
            [("Core", vec!["core".to_owned()])]
        );
    }

    #[rstest::rstest]
    #[case::component_macros_without_define_architecture(
        "mod core;\n",
        &[("src/core.rs", "architecture_component!(Core);\n")]
    )]
    #[case::define_architecture_in_crate_root_lib_rs(
        indoc::indoc! {r"
            define_architecture! {
                Core => [],
            }
            mod core;
        "},
        &[("src/core.rs", "architecture_component!(Core);\n")]
    )]
    #[case::define_architecture_in_non_architecture_child_module(
        "mod layers;\n",
        &[(
            "src/layers.rs",
            indoc::indoc! {r"
                define_architecture! {
                    Core => [],
                }
            "},
        )]
    )]
    #[case::define_architecture_below_nested_submodule(
        "mod app;\n",
        &[
            ("src/app.rs", "mod architecture;\n"),
            (
                "src/app/architecture.rs",
                indoc::indoc! {r"
                    define_architecture! {
                        Core => [],
                    }
                "},
            ),
        ]
    )]
    fn architecture_spec_reads_define_architecture_only_from_architecture_module(
        #[case] lib_rs: &str,
        #[case] modules: &[(&str, &str)],
    ) {
        let root = Path::new("/repo");
        let mut files = vec![
            ("Cargo.toml", "[package]\nname = \"demo\"\n"),
            ("src/lib.rs", lib_rs),
        ];
        files.extend_from_slice(modules);
        let source = MemoryFileSource::from_relative_files(root, &files);
        let index = ProjectIndex::new(ProjectLayout::discover(root, &source, &[]), &source);

        assert!(index.architecture_spec().is_none());
    }

    #[test]
    fn architecture_spec_accepts_directory_style_architecture_mod_rs() {
        let root = Path::new("/repo");
        let source = MemoryFileSource::from_relative_files(
            root,
            &[
                ("Cargo.toml", "[package]\nname = \"demo\"\n"),
                ("src/lib.rs", "mod architecture;\nmod core;\n"),
                (
                    "src/architecture/mod.rs",
                    indoc::indoc! {r"
                        define_architecture! {
                            Core => [],
                        }
                    "},
                ),
                ("src/core.rs", "architecture_component!(Core);\n"),
            ],
        );
        let index = ProjectIndex::new(ProjectLayout::discover(root, &source, &[]), &source);

        let spec = index
            .architecture_spec()
            .expect("src/architecture/mod.rs should yield a spec");
        assert_eq!(spec.components.len(), 1);
        assert_eq!(spec.components[0].roots, vec!["core".to_owned()]);
    }

    #[test]
    fn duplicate_component_macro_keeps_first_root_and_flags_duplicate_without_unmapped_cascade() {
        let root = Path::new("/repo");
        let core_src = indoc::indoc! {r"
            architecture_component!(Core);
            architecture_component!(App);

            pub fn run() {}
        "};
        let source = MemoryFileSource::from_relative_files(
            root,
            &[
                ("Cargo.toml", "[package]\nname = \"demo\"\n"),
                ("src/lib.rs", "mod architecture;\npub mod core;\n"),
                (
                    "src/architecture.rs",
                    indoc::indoc! {r"
                        define_architecture! {
                            Core => [],
                            App => [Core],
                        }
                    "},
                ),
                ("src/core.rs", core_src),
            ],
        );
        let index = ProjectIndex::new(ProjectLayout::discover(root, &source, &[]), &source);
        let spec = index
            .architecture_spec()
            .expect("src/architecture.rs should yield a spec");
        let parsed = ParsedFile::rust(core_src);
        let findings = index
            .file_architecture_findings(&parsed, &root.join("src/core.rs"))
            .expect("Rust file with architecture spec should produce findings");

        assert_eq!(
            spec.component_roots().get("core").map(String::as_str),
            Some("Core")
        );
        assert_eq!(findings.unmapped_architecture_modules, Vec::new());
        assert_eq!(findings.invalid_architecture_components.len(), 1);
        assert_eq!(
            findings.invalid_architecture_components[0].kind,
            crate::code_lint::project::InvalidArchitectureComponentKind::DuplicateInModule {
                first_component: "Core".to_owned(),
            }
        );
    }

    #[test]
    fn stray_component_macro_without_architecture_spec_is_reported_only_when_present() {
        let root = Path::new("/repo");
        let core_src = "architecture_component!(Core);\npub fn run() {}\n";
        let util_src = "pub fn helper() {}\n";
        let source = MemoryFileSource::from_relative_files(
            root,
            &[
                ("Cargo.toml", "[package]\nname = \"demo\"\n"),
                ("src/lib.rs", "pub mod core;\npub mod util;\n"),
                ("src/core.rs", core_src),
                ("src/util.rs", util_src),
            ],
        );
        let index = ProjectIndex::new(ProjectLayout::discover(root, &source, &[]), &source);
        let parsed_util = ParsedFile::rust(util_src);
        let util_findings =
            index.file_architecture_findings(&parsed_util, &root.join("src/util.rs"));
        let parsed_core = ParsedFile::rust(core_src);
        let core_findings = index
            .file_architecture_findings(&parsed_core, &root.join("src/core.rs"))
            .expect("stray architecture_component! should produce findings");

        assert!(index.architecture_spec().is_none());
        assert!(util_findings.is_none());
        assert_eq!(core_findings.invalid_architecture_components.len(), 1);
        assert_eq!(
            core_findings.invalid_architecture_components[0].kind,
            crate::code_lint::project::InvalidArchitectureComponentKind::MissingDefineArchitecture
        );
    }

    fn ancestors_of_child_class(child_module_source: &str) -> ResolvedClassAncestors {
        let root = Path::new("/repo");
        let source = MemoryFileSource::from_relative_files(
            root,
            &[
                ("pyproject.toml", "[project]\nname = \"demo\"\n"),
                ("pkg/__init__.py", ""),
                (
                    "pkg/base.py",
                    "class Base:\n    def run(self) -> None: ...\n",
                ),
                ("pkg/child.py", child_module_source),
            ],
        );
        let index = ProjectIndex::new(ProjectLayout::discover(root, &source, &[]), &source);
        let file = ParsedFile::new(child_module_source, Language::Python);
        let classes = extract_classes(&file);
        let child = classes
            .iter()
            .rposition(|class| class.name == "Child")
            .expect("fixture defines `Child`");
        index.resolve_class_ancestors(Some("pkg.child"), &classes, child)
    }

    #[rstest::rstest]
    #[case::function_local_class_shadowing_the_imported_base(indoc::indoc! {r"
        from pkg.base import Base

        def build():
            class Base: ...
            return Base

        class Child(Base): ...
    "})]
    #[case::nested_classes_before_the_subclass_and_a_later_same_named_class(indoc::indoc! {r"
        from pkg.base import Base

        class Outer:
            class InnerA: ...
            class InnerB: ...

        class Child(Base): ...

        class Base: ...
    "})]
    fn module_level_class_resolves_its_base_from_module_scope_bindings_before_it(
        #[case] child_module_source: &str,
    ) {
        let ancestors = ancestors_of_child_class(child_module_source);

        assert_eq!(ancestors.in_file, Vec::<usize>::new());
        assert_eq!(
            ancestors.cross_module,
            HashSet::from([PythonClassId {
                module: "pkg.base".to_owned(),
                class_index: 0,
            }])
        );
    }

    #[test]
    fn inconsistent_hierarchy_has_no_ancestors() {
        let ancestors = ancestors_of_child_class(indoc::indoc! {r"
            class Z: ...
            class A: ...
            class B(A): ...
            class Child(Z, A, B): ...
        "});

        assert_eq!(ancestors, ResolvedClassAncestors::default());
    }

    fn rust_architecture_stress_source(root: &Path) -> MemoryFileSource {
        use std::fmt::Write as _;

        let mut owned_files = vec![
            (
                "Cargo.toml".to_owned(),
                "[package]\nname = \"demo\"\n".to_owned(),
            ),
            (
                "src/architecture.rs".to_owned(),
                indoc::indoc! {r"
                    architecture_component!(Architecture);
                    define_architecture! {
                        Architecture => [],
                        Core => [],
                    }
                "}
                .to_owned(),
            ),
            (
                "src/core.rs".to_owned(),
                "architecture_component!(Core);\n".to_owned(),
            ),
        ];
        let mut mod_decls = String::from("mod architecture;\npub mod core;\n");
        for item_index in 0..48 {
            let mod_name = format!("unit_{item_index}");
            let _ = writeln!(mod_decls, "pub mod {mod_name};");
            owned_files.push((
                format!("src/{mod_name}.rs"),
                format!(
                    "// Mentions architecture_component! in comment {item_index}\npub fn run() {{}}\n"
                ),
            ));
        }
        owned_files.push(("src/lib.rs".to_owned(), mod_decls));
        let refs: Vec<(&str, &str)> = owned_files
            .iter()
            .map(|(path, content)| (path.as_str(), content.as_str()))
            .collect();
        MemoryFileSource::from_relative_files(root, &refs)
    }

    #[test]
    fn concurrent_rayon_and_external_thread_architecture_spec_queries_do_not_deadlock() {
        let root = Path::new("/repo");
        let source = rust_architecture_stress_source(root);
        let layout = ProjectLayout::discover(root, &source, &[]);
        let indices: Vec<ProjectIndex<'_>> = (0..16)
            .map(|_| ProjectIndex::new(layout.clone(), &source))
            .collect();

        let (external_counts, rayon_counts) = std::thread::scope(|scope| {
            let external = scope.spawn(|| {
                indices
                    .iter()
                    .map(|index| {
                        index
                            .architecture_spec()
                            .map_or(0, |spec| spec.components.len())
                    })
                    .collect::<Vec<_>>()
            });
            let pool_counts: Vec<usize> = (0..256)
                .into_par_iter()
                .map(|slot| {
                    indices[slot % indices.len()]
                        .architecture_spec()
                        .map_or(0, |spec| spec.components.len())
                })
                .collect();
            (
                external.join().expect("external thread must not panic"),
                pool_counts,
            )
        });

        assert_eq!(external_counts, vec![2; 16]);
        assert_eq!(rayon_counts, vec![2; 256]);
    }
}
