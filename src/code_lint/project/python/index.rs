//! Memoized Python cross-file module summary index and symbol/re-export resolution.

use super::architecture::evaluate_python_file_architecture;
use super::layout::PythonLayout;
use super::paths::{is_python_package_init, resolve_relative_python_module};
use crate::code_lint::ast::ParsedFile;
use crate::code_lint::ast::python::{
    ExportedAll, PythonFileSummary, PythonImportBinding, summarize_python_file,
};
use crate::code_lint::project::foundation::{
    ArchitectureSpec, FileArchitectureFindings, FileSource, KnownModulePrefix,
    ResolvedModuleTarget, longest_known_module_prefix,
};
use crate::diagnostic::Language;
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::{Arc, Mutex, OnceLock};

const MAX_REEXPORT_HOPS: usize = 8;
const MAX_STAR_IMPORTS_PER_QUERY: usize = 32;

/// Unique identity of a top-level Python class within a project (`module` + source-order `class_index`).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PythonClassId {
    /// Canonical dotted module path (`"pkg.interfaces"`).
    pub module: String,
    /// 0-based index in [`PythonFileSummary::classes`].
    pub class_index: usize,
}

/// Outcome of resolving a symbol or module reference in a Python module.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum ResolvedPythonTarget {
    Symbol { module: String, name: String },
    Module(String),
    Unknown,
}

type PythonSlot = Arc<OnceLock<Option<Arc<PythonFileSummary>>>>;

/// Memoized Python module summary index for a single project.
pub(in crate::code_lint::project) struct PythonIndex<'s> {
    layout: PythonLayout,
    source: &'s dyn FileSource,
    slots: Mutex<HashMap<String, PythonSlot>>,
}

impl<'s> PythonIndex<'s> {
    /// Constructs a memoized [`PythonIndex`] for `layout` and `source`.
    #[must_use]
    pub(in crate::code_lint::project) fn new(
        layout: PythonLayout,
        source: &'s dyn FileSource,
    ) -> Self {
        Self {
            layout,
            source,
            slots: Mutex::new(HashMap::new()),
        }
    }

    /// Python layout used by this index.
    #[must_use]
    pub(in crate::code_lint::project) const fn layout(&self) -> &PythonLayout {
        &self.layout
    }

    /// Mutable access to the Python layout for merging inferred source roots.
    pub(in crate::code_lint::project) const fn layout_mut(&mut self) -> &mut PythonLayout {
        &mut self.layout
    }

    /// Evaluates Python architectural conformance findings for `file` (`path`).
    #[must_use]
    pub(in crate::code_lint::project) fn evaluate_file_architecture(
        &self,
        file: &ParsedFile,
        path: &Path,
        spec: &ArchitectureSpec,
    ) -> FileArchitectureFindings {
        evaluate_python_file_architecture(
            file,
            path,
            &self.layout,
            self.source,
            spec,
            |module, symbol| match self.resolve_written_name(module, symbol) {
                ResolvedPythonTarget::Symbol { module, .. }
                | ResolvedPythonTarget::Module(module) => Some(module),
                ResolvedPythonTarget::Unknown => None,
            },
        )
    }

    /// Seeds the memoized summary for `path` from an already-parsed Python [`ParsedFile`].
    pub(in crate::code_lint::project) fn seed_from_parsed(&self, path: &Path, parsed: &ParsedFile) {
        if let Some(module_path) = self.layout.context_module_of_path(path) {
            self.seed_summary_slots(path, &module_path, || {
                Arc::new(summarize_python_file(parsed))
            });
        }
    }

    /// Seeds and returns the memoized [`PythonFileSummary`] for a parsed Python module
    /// (`full_path` with resolved `module_path`), preserving PEP 561 `.pyi` stub precedence.
    pub(super) fn seed_module(
        &self,
        full_path: &Path,
        module_path: &str,
        parsed: &ParsedFile,
    ) -> Arc<PythonFileSummary> {
        let summary = Arc::new(summarize_python_file(parsed));
        self.seed_summary_slots(full_path, module_path, || Arc::clone(&summary));
        summary
    }

    /// Resolves a normalized dotted Python reference path (`"pkg.build_plan"`, `"pkg.core.Parser.parse"`)
    /// across `known_modules`, their `module_symbols`, and package/submodule re-exports.
    #[must_use]
    pub(super) fn resolve_module_target(
        &self,
        normalized: &str,
        known_modules: &HashSet<&str>,
        module_symbols: &HashMap<&str, HashSet<String>>,
    ) -> Option<ResolvedModuleTarget> {
        if known_modules.contains(normalized) {
            return Some(ResolvedModuleTarget {
                module: normalized.to_owned(),
                symbol: None,
            });
        }
        let KnownModulePrefix {
            module: base_module,
            remainder,
        } = longest_known_module_prefix(normalized, ".", known_modules)?;
        if let Some(resolved) =
            self.resolve_symbol_or_module(base_module, remainder, known_modules, module_symbols)
        {
            return Some(resolved);
        }
        if let Some((symbol_prefix, member)) = remainder.rsplit_once('.')
            && let Some(prefix_target) = self.resolve_symbol_or_module(
                base_module,
                symbol_prefix,
                known_modules,
                module_symbols,
            )
            && let Some(first_symbol) = prefix_target.symbol
        {
            let method_candidate = format!("{first_symbol}::{member}");
            let resolved_symbol = if module_symbols
                .get(prefix_target.module.as_str())
                .is_some_and(|set| set.contains(&method_candidate))
            {
                method_candidate
            } else {
                first_symbol
            };
            return Some(ResolvedModuleTarget {
                module: prefix_target.module,
                symbol: Some(resolved_symbol),
            });
        }
        let mut segments = remainder.split('.').filter(|segment| !segment.is_empty());
        let first = segments.next().map(str::to_owned)?;
        if let Some(member) = segments.next() {
            let method_candidate = format!("{first}::{member}");
            if module_symbols
                .get(base_module)
                .is_some_and(|set| set.contains(&method_candidate))
            {
                return Some(ResolvedModuleTarget {
                    module: base_module.to_owned(),
                    symbol: Some(method_candidate),
                });
            }
        }
        Some(ResolvedModuleTarget {
            module: base_module.to_owned(),
            symbol: Some(first),
        })
    }

    /// Returns the memoized [`PythonFileSummary`] for `module`, loading it on demand if needed.
    #[must_use]
    pub(in crate::code_lint::project) fn summary(
        &self,
        module: &str,
    ) -> Option<Arc<PythonFileSummary>> {
        let slot = self.slot(module);
        slot.get_or_init(|| {
            let file_path = self.layout.module_file(module, self.source)?;
            let text = self.source.read_to_string(&file_path).ok()?;
            let parsed = ParsedFile::new(&text, Language::Python);
            Some(Arc::new(summarize_python_file(&parsed)))
        })
        .clone()
    }

    /// Resolves a base-class expression of `from_module` (occurring at `before_class_index`, or `None`
    /// for top-level lookup) to a concrete [`PythonClassId`].
    #[must_use]
    pub(super) fn resolve_class(
        &self,
        from_module: &str,
        written_base: &str,
        before_class_index: Option<usize>,
    ) -> Option<PythonClassId> {
        let summary = self.summary(from_module)?;
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
            self.resolve_written_name(from_module, written_base)
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
                let target_summary = self.summary(&module)?;
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

    #[must_use]
    pub(super) fn resolve_import_binding(
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
            .module_file(from_module, self.source)
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
            if self.summary(&target_module).is_some()
                || self.layout.is_namespace_dir(&target_module, self.source)
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
        if let Some(module_summary) = self.summary(&target_module) {
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
        if self.summary(&candidate_submodule).is_some()
            || self
                .layout
                .is_namespace_dir(&candidate_submodule, self.source)
        {
            return ResolvedPythonTarget::Module(candidate_submodule);
        }
        ResolvedPythonTarget::Unknown
    }

    fn seed_summary_slots(
        &self,
        full_path: &Path,
        module_path: &str,
        build_summary: impl Fn() -> Arc<PythonFileSummary>,
    ) {
        let mut cached = None;
        let mut get_summary = || Arc::clone(cached.get_or_insert_with(&build_summary));
        if let Some(module) = self.layout.module_of_path(full_path) {
            let resolved = self.layout.resolve_query_path(full_path);
            if self.layout.module_file(&module, self.source).as_deref() == Some(resolved.as_path())
            {
                let slot = self.slot(&module);
                let _ = slot.get_or_init(|| Some(get_summary()));
            }
        }
        let slot = self.slot(module_path);
        if slot.get().is_none() && self.layout.module_file(module_path, self.source).is_none() {
            let _ = slot.get_or_init(|| Some(get_summary()));
        }
    }

    fn resolve_symbol_or_module(
        &self,
        base_module: &str,
        path: &str,
        known_modules: &HashSet<&str>,
        module_symbols: &HashMap<&str, HashSet<String>>,
    ) -> Option<ResolvedModuleTarget> {
        if !path.contains('.')
            && module_symbols
                .get(base_module)
                .is_some_and(|set| set.contains(path))
        {
            return Some(ResolvedModuleTarget {
                module: base_module.to_owned(),
                symbol: Some(path.to_owned()),
            });
        }
        match self.resolve_written_name(base_module, path) {
            ResolvedPythonTarget::Symbol { module, name }
                if known_modules.contains(module.as_str()) =>
            {
                Some(ResolvedModuleTarget {
                    module,
                    symbol: Some(name),
                })
            }
            ResolvedPythonTarget::Module(module) if known_modules.contains(module.as_str()) => {
                Some(ResolvedModuleTarget {
                    module,
                    symbol: None,
                })
            }
            ResolvedPythonTarget::Symbol { .. }
            | ResolvedPythonTarget::Module(_)
            | ResolvedPythonTarget::Unknown => None,
        }
    }

    fn resolve_written_name(&self, from_module: &str, written: &str) -> ResolvedPythonTarget {
        let mut visited = HashSet::new();
        self.resolve_name_bounded(from_module, written, 0, &mut visited)
    }

    fn resolve_name_bounded(
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
        let Some(summary) = self.summary(from_module) else {
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

        let head_target = if let Some(binding) = summary.imports.get(head) {
            self.resolve_import_binding(from_module, binding, depth + 1, visited)
        } else {
            self.resolve_via_star_imports(from_module, &summary, head, depth + 1, visited)
        };
        let ResolvedPythonTarget::Module(base_module) = head_target else {
            return ResolvedPythonTarget::Unknown;
        };
        self.resolve_dotted_from_module(&summary, &base_module, &rest, depth + 1, visited)
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
            let cursor_summary = self.summary(&cursor);
            let submodule_imported = from_summary.imported_submodules.contains(&next_module)
                || cursor_summary
                    .as_ref()
                    .is_some_and(|summary| summary.imported_submodules.contains(&next_module));
            let next_is_module = self.summary(&next_module).is_some()
                || self.layout.is_namespace_dir(&next_module, self.source);
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
            .module_file(from_module, self.source)
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
            let Some(star_summary) = self.summary(&star_module) else {
                continue;
            };
            if let ExportedAll::Explicit(allowed) = &star_summary.exported_all
                && !allowed.iter().any(|item| item == symbol)
            {
                continue;
            }
            let resolved = self.resolve_name_bounded(&star_module, symbol, depth + 1, visited);
            if resolved != ResolvedPythonTarget::Unknown {
                return resolved;
            }
        }
        ResolvedPythonTarget::Unknown
    }

    fn slot(&self, module: &str) -> PythonSlot {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::code_lint::project::foundation::{MemoryFileSource, WorkspaceAnchor};

    #[test]
    fn dotted_submodule_resolution_requires_explicit_submodule_import() {
        let root = Path::new("/repo");
        let source = MemoryFileSource::from_relative_files(
            root,
            &[
                (".omnicoach.toml", ""),
                ("pkg/__init__.py", ""),
                ("pkg/sub.py", "class Store:\n    pass\n"),
                (
                    "pkg/with_sub_import.py",
                    indoc::indoc! {"
                        import pkg.sub
                        class Impl(pkg.sub.Store):
                            pass
                    "},
                ),
                (
                    "pkg/bare_pkg_import.py",
                    indoc::indoc! {"
                        import pkg
                        class Impl(pkg.sub.Store):
                            pass
                    "},
                ),
            ],
        );
        let layout = PythonLayout::discover(
            WorkspaceAnchor::new(root.to_path_buf(), None),
            root,
            None,
            &source,
            &[],
        );
        let index = PythonIndex::new(layout, &source);
        let resolved_with_sub =
            index.resolve_class("pkg.with_sub_import", "pkg.sub.Store", Some(0));
        let resolved_bare_pkg =
            index.resolve_class("pkg.bare_pkg_import", "pkg.sub.Store", Some(0));

        assert_eq!(
            resolved_with_sub,
            Some(PythonClassId {
                module: "pkg.sub".to_owned(),
                class_index: 0,
            })
        );
        assert_eq!(resolved_bare_pkg, None);
    }

    #[test]
    fn resolve_module_target_resolves_module_alias_and_reexported_class_methods() {
        let root = Path::new("/repo");
        let source = MemoryFileSource::from_relative_files(
            root,
            &[
                (
                    "pyproject.toml",
                    indoc::indoc! {r#"
                        [project]
                        name = "demo"
                    "#},
                ),
                (
                    "pkg/__init__.py",
                    indoc::indoc! {r"
                        from . import _core as core
                        from ._core import Parser
                    "},
                ),
                (
                    "pkg/_core.py",
                    indoc::indoc! {r"
                        class Parser:
                            def parse(self) -> None: ...
                    "},
                ),
            ],
        );
        let layout = PythonLayout::discover(
            WorkspaceAnchor::new(root.to_path_buf(), None),
            root,
            None,
            &source,
            &[],
        );
        let index = PythonIndex::new(layout, &source);
        let known_modules = HashSet::from(["pkg", "pkg._core"]);
        let module_symbols = HashMap::from([(
            "pkg._core",
            HashSet::from(["Parser".to_owned(), "Parser::parse".to_owned()]),
        )]);

        let via_module_alias =
            index.resolve_module_target("pkg.core.Parser.parse", &known_modules, &module_symbols);
        let via_reexported_class_without_symbols =
            index.resolve_module_target("pkg.Parser.parse", &known_modules, &HashMap::new());

        assert_eq!(
            via_module_alias,
            Some(ResolvedModuleTarget {
                module: "pkg._core".to_owned(),
                symbol: Some("Parser::parse".to_owned()),
            })
        );
        assert_eq!(
            via_reexported_class_without_symbols,
            Some(ResolvedModuleTarget {
                module: "pkg._core".to_owned(),
                symbol: Some("Parser".to_owned()),
            })
        );
    }
}
