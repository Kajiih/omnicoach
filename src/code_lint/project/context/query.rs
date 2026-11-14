//! Read-only cross-file query handle (`ProjectContext`) passed to project-aware rules
//! (`CodeLintProject`).

use super::index::ProjectIndex;
use crate::code_lint::ast::ParsedFile;
use crate::code_lint::project::foundation::FileArchitectureFindings;
use crate::code_lint::project::python::PythonModuleContext;
use std::path::Path;
use std::sync::Arc;

/// Read-only cross-file query handle passed to project-aware rules.
///
/// Wraps [`ProjectIndex`] behind an immutable query API exposing file-bound Python queries
/// ([`Self::python`]) and cached architectural conformance evaluation without mutating index slots
/// or exposing raw filesystem handles.
pub struct ProjectContext<'a> {
    index: &'a ProjectIndex<'a>,
}

impl<'a> ProjectContext<'a> {
    /// Wraps `index` in a read-only [`ProjectContext`].
    #[must_use]
    pub const fn new(index: &'a ProjectIndex<'a>) -> Self {
        Self { index }
    }

    /// Returns a read-only Python query context bound to `path`'s Python module.
    #[must_use]
    pub(crate) fn python(&self, path: &Path) -> PythonModuleContext<'a> {
        PythonModuleContext::new(&self.index.python, path)
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

    #[test]
    fn python_query_handle_exposes_nameable_override_and_ancestor_types() {
        use crate::code_lint::ast::python::extract_classes;
        use crate::code_lint::project::{DocumentedOverrideMethod, ResolvedClassAncestors};

        let root = Path::new("/repo");
        let base_code = indoc::indoc! {r#"
            class Base:
                def run(self) -> None:
                    """Run base."""
        "#};
        let child_code = indoc::indoc! {r#"
            from typing import override
            from pkg.base import Base

            class Child(Base):
                @override
                def run(self) -> None:
                    """Run child."""
        "#};
        let source = MemoryFileSource::from_relative_files(
            root,
            &[
                ("pyproject.toml", "[project]\nname = \"demo\"\n"),
                ("pkg/__init__.py", ""),
                ("pkg/base.py", base_code),
                ("pkg/child.py", child_code),
            ],
        );
        let layout = ProjectLayout::discover(root, &source, &[]);
        let index = ProjectIndex::new(layout, &source);
        let context = ProjectContext::new(&index);
        let python = context.python(&root.join("pkg/child.py"));
        let parsed = ParsedFile::new(child_code, Language::Python);
        let classes = extract_classes(&parsed);
        let ancestors: ResolvedClassAncestors = python.resolve_class_ancestors(&classes, 0);
        let overrides: Vec<DocumentedOverrideMethod<'_>> =
            python.documented_override_methods(&parsed);

        assert_eq!(ancestors.cross_module.len(), 1);
        assert_eq!(overrides.len(), 1);
        assert_eq!(overrides[0].ancestor_name, "Base");
    }
}
