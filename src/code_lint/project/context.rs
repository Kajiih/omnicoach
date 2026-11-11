//! Read-only cross-file query handle (`ProjectContext`) passed to project-aware rules.

use super::architecture::{ArchitectureSpec, FileArchitectureFindings};
use super::index::{
    ImportedProtocolSummary, InheritedDocstringMatch, ProjectIndex, PythonClassId,
    ResolvedClassAncestors,
};
use super::layout::ProjectLayout;
use crate::code_lint::ast::ParsedFile;
use crate::code_lint::ast::python::{
    OwnedDocstring, PythonClassInfo, PythonDocstring, PythonFileSummary, extract_classes,
};
use std::path::Path;
use std::sync::Arc;

/// A documented `@override` method paired with the docstring it inherits in Python C3 MRO order.
#[derive(Clone)]
pub struct DocumentedOverrideMethod<'a> {
    /// Name of the `@override` method.
    pub method_name: String,
    /// The `@override` method's own docstring.
    pub docstring: PythonDocstring<'a>,
    /// Name of the first ancestor class in C3 MRO order whose method of the same name has a docstring.
    pub ancestor_name: String,
    /// Inherited docstring from `ancestor_name`.
    pub inherited_docstring: OwnedDocstring,
}

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

    /// Project layout of the analyzed project.
    #[must_use]
    pub const fn layout(&self) -> &ProjectLayout {
        self.index.layout()
    }

    /// Maps `path` to its canonical dotted Python module path (`"pkg.service"`), if inside a
    /// Python source root.
    #[must_use]
    pub fn python_module_of_path(&self, path: &Path) -> Option<String> {
        self.index.layout().python_module_of_path(path)
    }

    /// Maps `path` to its crate-relative Rust module path (`"code_lint::ast"`), if inside the
    /// Rust source root.
    #[must_use]
    pub fn rust_module_of_path(&self, path: &Path) -> Option<String> {
        self.index.layout().rust_module_of_path(path)
    }

    /// Returns the memoized [`PythonFileSummary`] of `module`.
    #[must_use]
    pub fn python_summary(&self, module: &str) -> Option<Arc<PythonFileSummary>> {
        self.index.python_summary(module)
    }

    /// Resolves a base-class expression in `from_module` to a concrete [`PythonClassId`].
    #[must_use]
    pub fn resolve_python_class(
        &self,
        from_module: &str,
        written_base: &str,
        before_class_index: Option<usize>,
    ) -> Option<PythonClassId> {
        self.index
            .resolve_python_class(from_module, written_base, before_class_index)
    }

    /// Computes the Python C3 MRO (`[self, ...ancestors]`) of `class_id` across modules.
    #[must_use]
    pub fn python_class_mro(&self, class_id: &PythonClassId) -> Vec<PythonClassId> {
        self.index.python_class_mro(class_id)
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
        self.index
            .resolve_inherited_method_docstring(current_module, classes, index, method_name)
    }

    /// Extracts every `@override` method in `file` (`path`) that has both its own docstring and an
    /// inherited docstring in Python C3 MRO order across in-file and imported project classes.
    #[must_use]
    pub fn documented_override_methods<'f>(
        &self,
        file: &'f ParsedFile,
        path: &Path,
    ) -> Vec<DocumentedOverrideMethod<'f>> {
        if !file.source_text().contains("override") {
            return Vec::new();
        }
        let classes = extract_classes(file);
        let current_module = self.python_module_of_path(path);
        let mut overrides = Vec::new();
        for (index, class) in classes.iter().enumerate() {
            for method in class.methods() {
                if !method.has_override_decorator() {
                    continue;
                }
                let Some(docstring) = method.docstring() else {
                    continue;
                };
                let Some(InheritedDocstringMatch {
                    ancestor_name,
                    docstring: inherited_docstring,
                }) = self.resolve_inherited_method_docstring(
                    current_module.as_deref(),
                    &classes,
                    index,
                    &method.name,
                )
                else {
                    continue;
                };
                overrides.push(DocumentedOverrideMethod {
                    method_name: method.name,
                    docstring,
                    ancestor_name,
                    inherited_docstring,
                });
            }
        }
        overrides
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
        self.index
            .resolve_ancestor_class_names(current_module, classes, index)
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
        self.index
            .resolve_class_ancestors(current_module, classes, index)
    }

    /// Returns all `Protocol`s imported into `module` in deterministic local-name order.
    #[must_use]
    pub fn imported_protocols_in_module(&self, module: &str) -> Vec<ImportedProtocolSummary> {
        self.index.imported_protocols_in_module(module)
    }

    /// Returns the resolved [`ArchitectureSpec`] for this project, if declared.
    #[must_use]
    pub fn architecture_spec(&self) -> Option<&ArchitectureSpec> {
        self.index.architecture_spec()
    }

    /// Evaluates (or returns cached) architectural conformance findings for `file` at `path`.
    #[must_use]
    pub fn file_architecture_findings(
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
    use crate::code_lint::ast::python::extract_classes;
    use crate::code_lint::project::source::MemoryFileSource;
    use crate::diagnostic::Language;

    #[test]
    fn resolves_c3_mro_and_docstrings_across_python_modules() {
        let root = Path::new("/repo");
        let child_code = indoc::indoc! {"
            from pkg.base import Left, Right

            class Leaf(Left, Right):
                def execute(self) -> None:
                    pass
        "};
        let source = MemoryFileSource::from_relative_files(
            root,
            &[
                (".omnicoach.toml", ""),
                (
                    "pkg/base.py",
                    indoc::indoc! {r#"
                        class Root:
                            def execute(self) -> None:
                                """Execute from Root."""

                        class Left(Root):
                            pass

                        class Right(Root):
                            def execute(self) -> None:
                                """Execute from Right."""
                    "#},
                ),
                ("pkg/child.py", child_code),
            ],
        );
        let layout = ProjectLayout::discover(root, &source, &[]);
        let index = ProjectIndex::new(layout, &source);
        let context = ProjectContext::new(&index);
        let parsed = ParsedFile::new(child_code, Language::Python);
        let classes = extract_classes(&parsed);
        let inherited = context
            .resolve_inherited_method_docstring(Some("pkg.child"), &classes, 0, "execute")
            .expect("should resolve diamond MRO docstring from Right before Root");
        assert_eq!(inherited.ancestor_name, "Right");
        assert_eq!(inherited.docstring.text(), "Execute from Right.");
    }

    #[test]
    fn evaluates_toml_architecture_findings_across_python_modules() {
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
        let project_config = super::super::config::ProjectConfig::from_toml_str(toml_content)
            .expect("valid project TOML");
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
        let layout = ProjectLayout::discover(root, &source, &[]);
        let index = ProjectIndex::new(layout, &source);
        let context = ProjectContext::new(&index);
        let resolved_with_sub =
            context.resolve_python_class("pkg.with_sub_import", "pkg.sub.Store", Some(0));
        let resolved_bare_pkg =
            context.resolve_python_class("pkg.bare_pkg_import", "pkg.sub.Store", Some(0));

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
    fn terminates_and_resolves_docstrings_across_cyclic_python_bases() {
        let root = Path::new("/repo");
        let second_code = indoc::indoc! {r"
            from pkg.first import First

            class Second(First):
                def run(self) -> None:
                    pass
        "};
        let source = MemoryFileSource::from_relative_files(
            root,
            &[
                (".omnicoach.toml", ""),
                (
                    "pkg/first.py",
                    indoc::indoc! {r#"
                        from pkg.second import Second

                        class First(Second):
                            def run(self) -> None:
                                """Docstring on cyclic First."""
                    "#},
                ),
                ("pkg/second.py", second_code),
            ],
        );
        let layout = ProjectLayout::discover(root, &source, &[]);
        let index = ProjectIndex::new(layout, &source);
        let context = ProjectContext::new(&index);
        let parsed = ParsedFile::new(second_code, Language::Python);
        let classes = extract_classes(&parsed);
        let inherited = context
            .resolve_inherited_method_docstring(Some("pkg.second"), &classes, 0, "run")
            .expect("cyclic cross-module bases must terminate and still resolve First.run");
        assert_eq!(inherited.ancestor_name, "First");
        assert_eq!(inherited.docstring.text(), "Docstring on cyclic First.");
    }
}
