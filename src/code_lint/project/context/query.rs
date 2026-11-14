//! Read-only cross-file query handle (`ProjectContext`) passed to project-aware rules.

use super::index::ProjectIndex;
use crate::code_lint::ast::ParsedFile;
use crate::code_lint::ast::python::PythonClassInfo;
#[cfg(test)]
use crate::code_lint::ast::python::PythonFileSummary;
use crate::code_lint::project::foundation::FileArchitectureFindings;
use crate::code_lint::project::python::{
    DocumentedOverrideMethod, ImportedProtocolSummary, PythonModuleContext, ResolvedClassAncestors,
};
use std::path::Path;
use std::sync::Arc;

/// Read-only cross-file query handle passed to project-aware rules.
///
/// Wraps [`ProjectIndex`] behind an immutable query API so rules can resolve cross-module symbols,
/// `Protocol` contracts, C3 MROs, module paths, and architectural conformance findings without
/// mutating or seeding index slots or accessing raw filesystem handles.
pub struct ProjectContext<'a> {
    index: &'a ProjectIndex<'a>,
}

impl<'a> ProjectContext<'a> {
    /// Wraps `index` in a read-only [`ProjectContext`].
    #[must_use]
    pub const fn new(index: &'a ProjectIndex<'a>) -> Self {
        Self { index }
    }

    /// Maps a Python file path to its canonical dotted module path within this project.
    #[must_use]
    pub fn python_module_of_path(&self, path: &Path) -> Option<String> {
        self.index.python.layout().context_module_of_path(path)
    }

    /// Returns the memoized [`PythonFileSummary`] of `module`.
    #[cfg(test)]
    #[must_use]
    pub fn python_summary(&self, module: &str) -> Option<Arc<PythonFileSummary>> {
        PythonModuleContext::from_module(&self.index.python, Some(module.to_owned()))
            .summary(module)
    }

    /// Extracts every `@override` method in `file` (`path`) that has both its own docstring and an
    /// inherited docstring in Python C3 MRO order across in-file and imported project classes.
    #[must_use]
    pub fn documented_override_methods<'f>(
        &self,
        file: &'f ParsedFile,
        path: &Path,
    ) -> Vec<DocumentedOverrideMethod<'f>> {
        PythonModuleContext::new(&self.index.python, path).documented_override_methods(file)
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
        PythonModuleContext::from_module(&self.index.python, current_module.map(str::to_owned))
            .resolve_ancestor_class_names(classes, index)
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
        PythonModuleContext::from_module(&self.index.python, current_module.map(str::to_owned))
            .resolve_class_ancestors(classes, index)
    }

    /// Returns true if `classes[index]` or any of its C3 MRO ancestors (in-file or imported from
    /// another project module) has a direct base class whose import-resolved path is one of
    /// `candidates`.
    #[must_use]
    pub fn class_mro_has_base_in(
        &self,
        current_module: Option<&str>,
        classes: &[PythonClassInfo<'_>],
        index: usize,
        candidates: &[&str],
    ) -> bool {
        PythonModuleContext::from_module(&self.index.python, current_module.map(str::to_owned))
            .class_mro_has_base_in(classes, index, candidates)
    }

    /// Returns all `Protocol`s imported into `module` in deterministic local-name order.
    #[must_use]
    pub fn imported_protocols_in_module(&self, module: &str) -> Vec<ImportedProtocolSummary> {
        PythonModuleContext::from_module(&self.index.python, Some(module.to_owned()))
            .imported_protocols()
    }

    /// Evaluates (or returns cached) architectural conformance findings for `file` at `path`.
    #[must_use]
    pub(in crate::code_lint) fn file_architecture_findings(
        &self,
        file: &ParsedFile,
        path: &Path,
    ) -> Option<Arc<FileArchitectureFindings>> {
        self.index.file_architecture_findings(file, path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::code_lint::project::context::ProjectLayout;
    use crate::code_lint::project::foundation::{MemoryFileSource, ProjectConfig};
    use crate::diagnostic::Language;

    #[test]
    fn evaluates_toml_architecture_findings_across_modules() {
        let root = Path::new("/repo");
        let bad_api_code = "from pkg.cli import run_cli\n";
        let toml_content = indoc::indoc! {r#"
            [[architecture.components]]
            name = "Core"
            roots = ["pkg.core"]

            [[architecture.components]]
            name = "Cli"
            roots = ["pkg.cli"]
            depends-on = ["Core"]
        "#};
        let source = MemoryFileSource::from_relative_files(
            root,
            &[
                (".omnicoach.toml", toml_content),
                ("pkg/__init__.py", ""),
                ("pkg/core.py", "X = 1\n"),
                ("pkg/cli.py", "def run_cli(): pass\n"),
            ],
        );
        let project_config =
            ProjectConfig::from_toml_str(toml_content).expect("valid project TOML");
        let layout = ProjectLayout::discover(root, &source, &[]);
        let index = ProjectIndex::new(layout, &source).with_project_config(project_config);
        let context = ProjectContext::new(&index);
        let parsed = ParsedFile::new(bad_api_code, Language::Python);
        let findings = context
            .file_architecture_findings(&parsed, &root.join("pkg/core.py"))
            .expect("architecture spec should be active");
        assert_eq!(findings.undeclared_component_dependencies.len(), 1);
        assert_eq!(
            findings.undeclared_component_dependencies[0].component,
            "Core"
        );
        assert_eq!(
            findings.undeclared_component_dependencies[0].dependency,
            "Cli"
        );
    }
}
