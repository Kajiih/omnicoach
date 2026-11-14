//! Memoized Rust module header index and re-export resolution (`CodeLintProjectRust`).

use super::architecture::{
    RUST_ARCHITECTURE_MODULE, SiblingIsolationRule, architecture_config_from_rust_headers,
    build_rust_sibling_isolation_rules, evaluate_rust_file_architecture,
    evaluate_stray_rust_component_markers,
};
use super::layout::RustLayout;
use super::paths::{LIB_CRATE_ROOT, resolve_rust_reference_path};
use crate::code_lint::ast::ParsedFile;
use crate::code_lint::ast::rust::{RustFileSummary, summarize_rust_module_header};
use crate::code_lint::project::foundation::{
    ArchitectureConfig, ArchitectureSpec, FileArchitectureFindings, FileSource,
    ResolvedModuleTarget, SplitModulePath, split_module_and_segments,
};
use rayon::prelude::*;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

const RUST_EXTENSION: &str = "rs";
const MAX_REEXPORT_HOPS: usize = 8;
const ARCHITECTURE_COMPONENT_CALL_PREFIX: &str = "architecture_component!";
const DEFINE_ARCHITECTURE_CALL_PREFIX: &str = "define_architecture!";

type RustSlot = Arc<OnceLock<Option<Arc<RustFileSummary>>>>;

/// Memoized Rust module summary index for a single crate.
pub(in crate::code_lint::project) struct RustIndex<'s> {
    layout: RustLayout,
    source: &'s dyn FileSource,
    slots: Mutex<HashMap<String, RustSlot>>,
    all_headers: OnceLock<BTreeMap<String, Arc<RustFileSummary>>>,
    sibling_rules: OnceLock<Vec<SiblingIsolationRule>>,
}

impl<'s> RustIndex<'s> {
    /// Constructs a memoized [`RustIndex`] for `layout` and `source`.
    #[must_use]
    pub(in crate::code_lint::project) fn new(
        layout: RustLayout,
        source: &'s dyn FileSource,
    ) -> Self {
        Self {
            layout,
            source,
            slots: Mutex::new(HashMap::new()),
            all_headers: OnceLock::new(),
            sibling_rules: OnceLock::new(),
        }
    }

    /// Rust layout used by this index.
    #[must_use]
    pub(in crate::code_lint::project) const fn layout(&self) -> &RustLayout {
        &self.layout
    }

    /// Seeds the memoized header summary for `path` from an already-parsed Rust [`ParsedFile`]
    /// when `parsed` mentions an architecture macro candidate.
    pub(in crate::code_lint::project) fn seed_from_parsed(&self, path: &Path, parsed: &ParsedFile) {
        if has_rust_architecture_macro_candidate(&parsed.source_text())
            && let Some(module) = self.layout.module_of_path(path)
        {
            let slot = self.slot(&module);
            let _ = slot.get_or_init(|| Some(Arc::new(summarize_rust_module_header(parsed))));
        }
    }

    /// Seeds the memoized header slot for `module` (if unpopulated) and returns `summary` wrapped
    /// in an [`Arc`] so callers retain full-body `referenced_paths` even when `headers_map`
    /// previously cached a header-only summary in the slot.
    pub(super) fn seed_summary(
        &self,
        module: &str,
        summary: RustFileSummary,
    ) -> Arc<RustFileSummary> {
        let shared = Arc::new(summary);
        let slot = self.slot(module);
        let _ = slot.get_or_init(|| Some(Arc::clone(&shared)));
        shared
    }

    /// Synthesizes an [`ArchitectureConfig`] when the crate declares `define_architecture!` in its
    /// `architecture` module (`src/architecture.rs` or `src/architecture/mod.rs`), returning `None`
    /// without scanning `src/` when no macro declaration exists.
    #[must_use]
    pub(in crate::code_lint::project) fn macro_architecture_config(
        &self,
    ) -> Option<ArchitectureConfig> {
        if !self.declares_architecture() {
            return None;
        }
        Some(architecture_config_from_rust_headers(
            self.macro_headers_map(),
        ))
    }

    /// Returns true if a Rust file `source_text` may produce architectural findings even when no
    /// [`ArchitectureSpec`] is declared (stray `architecture_component!`, `pub use`, or
    /// `#[macro_export]`).
    #[must_use]
    pub(in crate::code_lint::project) fn has_unconfigured_architecture_candidates(
        source_text: &str,
    ) -> bool {
        source_text.contains(ARCHITECTURE_COMPONENT_CALL_PREFIX)
            || source_text.contains("use")
            || source_text.contains("macro_export")
    }

    /// Evaluates Rust architectural conformance findings for `file` (`path`), falling back to
    /// spec-independent stray-marker and re-export checks when `spec` is `None`.
    #[must_use]
    pub(in crate::code_lint::project) fn evaluate_file_architecture(
        &self,
        file: &ParsedFile,
        path: &Path,
        spec: Option<&ArchitectureSpec>,
    ) -> FileArchitectureFindings {
        spec.map_or_else(
            || evaluate_stray_rust_component_markers(file, path, &self.layout),
            |resolved| {
                let sibling_rules = self.sibling_rules.get_or_init(|| {
                    build_rust_sibling_isolation_rules(&resolved.components, |root| {
                        self.summary(root)
                    })
                });
                evaluate_rust_file_architecture(file, path, &self.layout, resolved, sibling_rules)
            },
        )
    }

    /// Resolves a canonical crate-relative Rust reference path (`"core::Config"`, `"core::Parser::parse"`)
    /// across `known_modules`, their `module_symbols`, and bounded `pub use` re-export chains.
    #[must_use]
    pub(super) fn resolve_module_target(
        &self,
        canonical: &str,
        known_modules: &HashSet<&str>,
        module_symbols: &HashMap<&str, HashSet<String>>,
    ) -> Option<ResolvedModuleTarget> {
        if canonical.is_empty() {
            return None;
        }
        let initial = split_module_and_segments(canonical, "::", known_modules).or_else(|| {
            if known_modules.contains(LIB_CRATE_ROOT) {
                let mut segments = canonical.split("::").filter(|segment| !segment.is_empty());
                let first = segments.next().map(str::to_owned)?;
                let remainder = {
                    let rest: Vec<&str> = segments.collect();
                    (!rest.is_empty()).then(|| rest.join("::"))
                };
                let is_known_in_lib = module_symbols
                    .get(LIB_CRATE_ROOT)
                    .is_some_and(|set| set.contains(&first))
                    || self.resolve_reexport(LIB_CRATE_ROOT, &first).is_some();
                is_known_in_lib.then(|| SplitModulePath {
                    module: LIB_CRATE_ROOT.to_owned(),
                    symbol: Some(first),
                    remainder,
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
        let mut remaining_tail = initial.remainder;
        for _ in 0..MAX_REEXPORT_HOPS {
            let has_symbol = module_symbols
                .get(resolved_module.as_str())
                .is_some_and(|set| set.contains(&resolved_first));
            if has_symbol {
                break;
            }
            let Some(reexported) = self.resolve_reexport(&resolved_module, &resolved_first) else {
                break;
            };
            let combined = match remaining_tail.as_deref() {
                Some(tail) => format!("{reexported}::{tail}"),
                None => reexported,
            };
            let Some(next) = split_module_and_segments(&combined, "::", known_modules) else {
                break;
            };
            resolved_module = next.module;
            remaining_tail = next.remainder;
            if let Some(next_symbol) = next.symbol {
                resolved_first = next_symbol;
            } else {
                return Some(ResolvedModuleTarget {
                    module: resolved_module,
                    symbol: None,
                });
            }
        }
        if let Some(member) = remaining_tail
            .as_deref()
            .and_then(|tail| tail.split("::").next())
        {
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

    fn macro_headers_map(&self) -> &BTreeMap<String, Arc<RustFileSummary>> {
        if let Some(headers) = self.all_headers.get() {
            return headers;
        }
        // Compute outside `OnceLock::get_or_init` so Rayon work-stealing inside
        // `collect_macro_headers` cannot re-enter and deadlock on `self.all_headers` while another
        // thread waits on it.
        let headers = self.collect_macro_headers();
        self.all_headers.get_or_init(|| headers)
    }

    fn collect_macro_headers(&self) -> BTreeMap<String, Arc<RustFileSummary>> {
        if !self.source.is_dir(&self.layout.source_root) {
            return BTreeMap::new();
        }
        let rust_files: Vec<PathBuf> = self
            .source
            .walk_files(&self.layout.source_root)
            .unwrap_or_default()
            .into_iter()
            .filter(|path| {
                path.extension()
                    .is_some_and(|extension| extension == RUST_EXTENSION)
            })
            .collect();
        rust_files
            .par_iter()
            .filter_map(|path| self.parse_macro_candidate_header(path))
            .map(|entry| (entry.module, entry.summary))
            .collect()
    }

    /// Whether the crate declares `define_architecture!` in its `architecture` module
    /// (`src/architecture.rs` or `src/architecture/mod.rs`), the only module where it is
    /// recognized, so crates without one pay at most a single module probe instead of a crate-wide
    /// header scan.
    fn declares_architecture(&self) -> bool {
        let slot = self.slot(RUST_ARCHITECTURE_MODULE);
        if let Some(cached) = slot.get() {
            return cached
                .as_ref()
                .is_some_and(|summary| !summary.defined_architecture.is_empty());
        }
        self.layout
            .module_file(RUST_ARCHITECTURE_MODULE, self.source)
            .and_then(|path| self.source.read_to_string(&path).ok())
            .filter(|text| text.contains(DEFINE_ARCHITECTURE_CALL_PREFIX))
            .is_some_and(|text| {
                let summary = slot.get_or_init(|| {
                    let parsed = ParsedFile::rust(&text);
                    Some(Arc::new(summarize_rust_module_header(&parsed)))
                });
                summary
                    .as_ref()
                    .is_some_and(|header| !header.defined_architecture.is_empty())
            })
    }

    fn parse_macro_candidate_header(&self, path: &Path) -> Option<ModuleHeaderEntry> {
        let module = self.layout.module_of_path(path)?;
        let slot = self.slot(&module);
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

    fn resolve_reexport(&self, module: &str, symbol: &str) -> Option<String> {
        let summary = self.summary(module)?;
        let crate_name = self.layout.crate_name.as_deref();
        for declaration in &summary.visible_uses {
            for (exported_name, target) in &declaration.reexports {
                if exported_name == symbol {
                    return Some(
                        resolve_rust_reference_path(
                            module,
                            &summary.external_mods,
                            target,
                            crate_name,
                        )
                        .canonical_path,
                    );
                }
            }
        }
        None
    }

    fn summary(&self, module: &str) -> Option<Arc<RustFileSummary>> {
        let slot = self.slot(module);
        slot.get_or_init(|| {
            let file_path = self.layout.module_file(module, self.source)?;
            let text = self.source.read_to_string(&file_path).ok()?;
            let parsed = ParsedFile::rust(&text);
            Some(Arc::new(summarize_rust_module_header(&parsed)))
        })
        .clone()
    }

    fn slot(&self, module: &str) -> RustSlot {
        let mut guard = self
            .slots
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

struct ModuleHeaderEntry {
    module: String,
    summary: Arc<RustFileSummary>,
}

/// Cheap lexical prefilter for Rust files that may declare architecture macros: `true` whenever
/// the text mentions `architecture_component!` or `define_architecture!` anywhere.
fn has_rust_architecture_macro_candidate(text: &str) -> bool {
    text.contains(ARCHITECTURE_COMPONENT_CALL_PREFIX)
        || text.contains(DEFINE_ARCHITECTURE_CALL_PREFIX)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::code_lint::project::foundation::MemoryFileSource;

    #[test]
    fn resolve_module_target_follows_module_and_alias_reexports_with_methods() {
        let root = Path::new("/repo");
        let source = MemoryFileSource::from_relative_files(
            root,
            &[
                (
                    "Cargo.toml",
                    indoc::indoc! {r#"
                        [package]
                        name = "demo"
                    "#},
                ),
                ("src/lib.rs", "pub mod core;\n"),
                (
                    "src/core.rs",
                    indoc::indoc! {r"
                        mod detail;
                        pub use self::detail::InternalConfig as PublicConfig;
                        pub use self::detail::submod;
                    "},
                ),
                (
                    "src/core/detail.rs",
                    indoc::indoc! {r"
                        pub mod submod;
                        pub struct InternalConfig;
                    "},
                ),
                ("src/core/detail/submod.rs", "pub struct Thing;\n"),
            ],
        );
        let layout = RustLayout::discover(
            crate::code_lint::project::foundation::WorkspaceAnchor::new(root.to_path_buf(), None),
            &source,
        );
        let index = RustIndex::new(layout, &source);
        let known_modules = HashSet::from(["lib", "core", "core::detail", "core::detail::submod"]);
        let module_symbols = HashMap::from([
            (
                "core::detail",
                HashSet::from([
                    "InternalConfig".to_owned(),
                    "InternalConfig::load".to_owned(),
                ]),
            ),
            (
                "core::detail::submod",
                HashSet::from(["Thing".to_owned(), "Thing::run".to_owned()]),
            ),
        ]);

        let alias_target = index.resolve_module_target(
            "core::PublicConfig::load",
            &known_modules,
            &module_symbols,
        );
        let submod_target = index.resolve_module_target(
            "core::submod::Thing::run",
            &known_modules,
            &module_symbols,
        );

        assert_eq!(
            alias_target,
            Some(ResolvedModuleTarget {
                module: "core::detail".to_owned(),
                symbol: Some("InternalConfig::load".to_owned()),
            })
        );
        assert_eq!(
            submod_target,
            Some(ResolvedModuleTarget {
                module: "core::detail::submod".to_owned(),
                symbol: Some("Thing::run".to_owned()),
            })
        );
    }
}
