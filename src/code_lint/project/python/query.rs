//! File-bound Python cross-module query handle (`PythonModuleContext`), C3 MRO resolution,
//! inherited `@override` docstrings, and imported `Protocol` discovery (`CodeLintProjectPython`).

use super::index::{PythonClassId, PythonIndex, ResolvedPythonTarget};
use crate::code_lint::ast::ParsedFile;
use crate::code_lint::ast::python::{
    OwnedDocstring, PythonClassInfo, PythonDocstring, extract_classes,
};
use std::collections::HashSet;
use std::path::Path;
#[cfg(test)]
use std::sync::Arc;

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

/// Read-only Python cross-module query handle bound to a target file's module path.
pub struct PythonModuleContext<'a> {
    index: &'a PythonIndex<'a>,
    current_module: Option<String>,
}

impl<'a> PythonModuleContext<'a> {
    /// Constructs a [`PythonModuleContext`] bound to `path` within `index`.
    #[must_use]
    pub(in crate::code_lint::project) fn new(index: &'a PythonIndex<'a>, path: &Path) -> Self {
        let current_module = index.layout().context_module_of_path(path);
        Self {
            index,
            current_module,
        }
    }

    /// Constructs a [`PythonModuleContext`] bound to an already-resolved `current_module`.
    #[must_use]
    pub(in crate::code_lint::project) const fn from_module(
        index: &'a PythonIndex<'a>,
        current_module: Option<String>,
    ) -> Self {
        Self {
            index,
            current_module,
        }
    }

    /// Returns the memoized [`crate::code_lint::ast::python::PythonFileSummary`] of `module`.
    #[cfg(test)]
    #[must_use]
    pub fn summary(
        &self,
        module: &str,
    ) -> Option<Arc<crate::code_lint::ast::python::PythonFileSummary>> {
        self.index.summary(module)
    }

    /// Extracts every `@override` method in `file` that has both its own docstring and an
    /// inherited docstring in Python C3 MRO order across in-file and imported project classes.
    #[must_use]
    pub fn documented_override_methods<'f>(
        &self,
        file: &'f ParsedFile,
    ) -> Vec<DocumentedOverrideMethod<'f>> {
        if !file.source_text().contains("override") {
            return Vec::new();
        }
        let classes = extract_classes(file);
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
                }) = self.resolve_inherited_method_docstring(&classes, index, &method.name)
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
    /// classes (`classes[..index]`) and cross-module classes resolved from the bound module.
    #[must_use]
    pub fn resolve_ancestor_class_names(
        &self,
        classes: &[PythonClassInfo<'_>],
        index: usize,
    ) -> Vec<String> {
        self.class_mro_entries(classes, index)
            .into_iter()
            .skip(1)
            .filter_map(|entry| match entry {
                ClassMroEntry::Local(ancestor_index) => Some(classes[ancestor_index].name.clone()),
                ClassMroEntry::Imported(ancestor_id) => {
                    self.index.summary(&ancestor_id.module).and_then(|summary| {
                        summary
                            .classes
                            .get(ancestor_id.class_index)
                            .map(|class_summary| class_summary.name.clone())
                    })
                }
            })
            .collect()
    }

    /// Resolves the strict C3 MRO ancestors of `classes[index]` across in-file classes
    /// (`classes[..index]`) and cross-module classes resolved from the bound module.
    #[must_use]
    pub fn resolve_class_ancestors(
        &self,
        classes: &[PythonClassInfo<'_>],
        index: usize,
    ) -> ResolvedClassAncestors {
        let mut ancestors = ResolvedClassAncestors::default();
        for entry in self.class_mro_entries(classes, index).into_iter().skip(1) {
            match entry {
                ClassMroEntry::Local(ancestor_index) => ancestors.in_file.push(ancestor_index),
                ClassMroEntry::Imported(ancestor_id) => {
                    if let Some(summary) = self.index.summary(&ancestor_id.module)
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

    /// Returns true if `classes[index]` or any of its C3 MRO ancestors (in-file or imported from
    /// another project module) has a direct base class whose import-resolved path is one of
    /// `candidates`.
    #[must_use]
    pub fn class_mro_has_base_in(
        &self,
        classes: &[PythonClassInfo<'_>],
        index: usize,
        candidates: &[&str],
    ) -> bool {
        self.class_mro_entries(classes, index)
            .into_iter()
            .any(|entry| match entry {
                ClassMroEntry::Local(local_index) => {
                    self.local_class_has_external_base_in(classes, local_index, candidates)
                }
                ClassMroEntry::Imported(ancestor_id) => {
                    self.imported_class_has_external_base_in(&ancestor_id, candidates)
                }
            })
    }

    /// Returns all `Protocol`s imported into the bound module in deterministic local-name order.
    #[must_use]
    pub fn imported_protocols(&self) -> Vec<ImportedProtocolSummary> {
        let Some(module) = self.current_module.as_deref() else {
            return Vec::new();
        };
        let Some(summary) = self.index.summary(module) else {
            return Vec::new();
        };
        let mut local_names: Vec<&String> = summary.imports.keys().collect();
        local_names.sort();
        let mut imported = Vec::new();
        for local_name in local_names {
            let Some(binding) = summary.imports.get(local_name) else {
                continue;
            };
            if binding.imported_name.is_none() || binding.type_checking_only {
                continue;
            }
            if let ResolvedPythonTarget::Symbol {
                module: target_module,
                name: target_name,
            } = self
                .index
                .resolve_import_binding(module, binding, 0, &mut HashSet::new())
                && let Some(target_summary) = self.index.summary(&target_module)
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
                    let mro = self.class_mro(&class_id);
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

    fn resolve_inherited_method_docstring(
        &self,
        classes: &[PythonClassInfo<'_>],
        index: usize,
        method_name: &str,
    ) -> Option<InheritedDocstringMatch> {
        for entry in self.class_mro_entries(classes, index).into_iter().skip(1) {
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
                    if let Some(summary) = self.index.summary(&ancestor_id.module)
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

    fn protocol_counted_methods(&self, mro: &[PythonClassId]) -> HashSet<String> {
        let mut methods = HashSet::new();
        for ancestor_id in mro {
            if let Some(summary) = self.index.summary(&ancestor_id.module)
                && let Some(class_summary) = summary.classes.get(ancestor_id.class_index)
            {
                methods.extend(class_summary.public_non_property_methods.iter().cloned());
            }
        }
        methods
    }

    fn class_mro_entries(
        &self,
        classes: &[PythonClassInfo<'_>],
        index: usize,
    ) -> Vec<ClassMroEntry> {
        let mut active = HashSet::new();
        self.compute_local_class_mro(classes, index, &mut active)
    }

    /// Computes the C3 MRO of `classes[index]`. As at runtime, a simple base name refers to the
    /// last lexically visible class of that name bound before the subclass.
    fn compute_local_class_mro(
        &self,
        classes: &[PythonClassInfo<'_>],
        index: usize,
        active: &mut HashSet<usize>,
    ) -> Vec<ClassMroEntry> {
        let self_entry = ClassMroEntry::Local(index);
        if index >= classes.len() || !active.insert(index) {
            return Vec::new();
        }
        let module_level_classes_before = classes[..index]
            .iter()
            .filter(|class| class.is_module_level())
            .count();
        let mut direct_bases = Vec::new();
        for base in &classes[index].bases {
            if base.is_structural_marker() {
                continue;
            }
            if let Some(entry) = self.resolve_local_class_base(
                classes,
                index,
                module_level_classes_before,
                base.unsubscripted_name(),
            ) {
                direct_bases.push(entry);
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
                    ClassMroEntry::Local(base_index) => {
                        self.compute_local_class_mro(classes, *base_index, active)
                    }
                    ClassMroEntry::Imported(base_id) => self
                        .class_mro(base_id)
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

    fn local_class_has_external_base_in(
        &self,
        classes: &[PythonClassInfo<'_>],
        index: usize,
        candidates: &[&str],
    ) -> bool {
        let module_level_classes_before = classes[..index]
            .iter()
            .filter(|class| class.is_module_level())
            .count();
        classes[index].bases.iter().any(|base| {
            self.resolve_local_class_base(
                classes,
                index,
                module_level_classes_before,
                base.unsubscripted_name(),
            )
            .is_none()
                && base
                    .resolved_path
                    .as_deref()
                    .is_some_and(|path| candidates.contains(&path))
        })
    }

    fn imported_class_has_external_base_in(
        &self,
        class_id: &PythonClassId,
        candidates: &[&str],
    ) -> bool {
        let Some(summary) = self.index.summary(&class_id.module) else {
            return false;
        };
        let Some(class_summary) = summary.classes.get(class_id.class_index) else {
            return false;
        };
        class_summary.bases.iter().any(|base| {
            self.index
                .resolve_class(
                    &class_id.module,
                    &base.unsubscripted_name,
                    Some(class_id.class_index),
                )
                .is_none()
                && base
                    .resolved_path
                    .as_deref()
                    .is_some_and(|path| candidates.contains(&path))
        })
    }

    fn resolve_local_class_base(
        &self,
        classes: &[PythonClassInfo<'_>],
        index: usize,
        module_level_classes_before: usize,
        base_name: &str,
    ) -> Option<ClassMroEntry> {
        if !base_name.contains('.')
            && let Some(local_index) = (0..index).rev().find(|&candidate| {
                classes[candidate].name == base_name
                    && classes[candidate].is_visible_at(&classes[index])
            })
        {
            return Some(ClassMroEntry::Local(local_index));
        }
        if let Some(module) = self.current_module.as_deref()
            && let Some(target_id) =
                self.index
                    .resolve_class(module, base_name, Some(module_level_classes_before))
            && (base_name.contains('.') || target_id.module != module)
        {
            return Some(ClassMroEntry::Imported(target_id));
        }
        None
    }

    fn class_mro(&self, class_id: &PythonClassId) -> Vec<PythonClassId> {
        let mut active = HashSet::new();
        self.compute_c3_mro(class_id, &mut active)
    }

    fn compute_c3_mro(
        &self,
        class_id: &PythonClassId,
        active: &mut HashSet<PythonClassId>,
    ) -> Vec<PythonClassId> {
        if !active.insert(class_id.clone()) {
            return Vec::new();
        }
        let Some(summary) = self.index.summary(&class_id.module) else {
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
                self.index.resolve_class(
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
}

struct InheritedDocstringMatch {
    ancestor_name: String,
    docstring: OwnedDocstring,
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
    use super::super::layout::PythonLayout;
    use super::*;
    use crate::code_lint::project::foundation::{
        FileSource as _, MemoryFileSource, WorkspaceAnchor,
    };
    use crate::diagnostic::Language;
    use rstest::rstest;

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
        let layout = PythonLayout::discover(
            WorkspaceAnchor::new(root.to_path_buf(), None),
            root,
            None,
            &source,
            &[],
        );
        let index = PythonIndex::new(layout, &source);
        let file = ParsedFile::new(child_module_source, Language::Python);
        let classes = extract_classes(&file);
        let child = classes
            .iter()
            .rposition(|class| class.name == "Child")
            .expect("fixture defines `Child`");
        let context = PythonModuleContext::new(&index, &root.join("pkg/child.py"));
        context.resolve_class_ancestors(&classes, child)
    }

    #[rstest]
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
        let layout = PythonLayout::discover(
            WorkspaceAnchor::new(root.to_path_buf(), None),
            root,
            None,
            &source,
            &[],
        );
        let index = PythonIndex::new(layout, &source);
        let context = PythonModuleContext::new(&index, &root.join("pkg/child.py"));
        let parsed = ParsedFile::new(child_code, Language::Python);
        let classes = extract_classes(&parsed);
        let inherited = context
            .resolve_inherited_method_docstring(&classes, 0, "execute")
            .expect("should resolve diamond MRO docstring from Right before Root");
        assert_eq!(inherited.ancestor_name, "Right");
        assert_eq!(inherited.docstring.text(), "Execute from Right.");
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
        let layout = PythonLayout::discover(
            WorkspaceAnchor::new(root.to_path_buf(), None),
            root,
            None,
            &source,
            &[],
        );
        let index = PythonIndex::new(layout, &source);
        let context = PythonModuleContext::new(&index, &root.join("pkg/second.py"));
        let parsed = ParsedFile::new(second_code, Language::Python);
        let classes = extract_classes(&parsed);
        let inherited = context
            .resolve_inherited_method_docstring(&classes, 0, "run")
            .expect("cyclic cross-module bases must terminate and still resolve First.run");
        assert_eq!(inherited.ancestor_name, "First");
        assert_eq!(inherited.docstring.text(), "Docstring on cyclic First.");
    }

    #[rstest]
    #[case::diamond_cross_module(
        &[
            ("pkg/__init__.py", ""),
            ("pkg/root.py", "from pydantic import BaseModel\nclass Root(BaseModel): pass\n"),
            ("pkg/left.py", "from pkg.root import Root\nclass Left(Root): pass\n"),
            ("pkg/right.py", "from pkg.root import Root\nclass Right(Root): pass\n"),
            (
                "pkg/leaf.py",
                indoc::indoc! {r"
                    from pkg.left import Left
                    from pkg.right import Right
                    class Leaf(Left, Right): pass
                "},
            ),
        ],
        "pkg/leaf.py",
        true
    )]
    #[case::cyclic_cross_module(
        &[
            ("pkg/__init__.py", ""),
            ("pkg/first.py", "from pkg.second import Second\nclass First(Second): pass\n"),
            ("pkg/second.py", "from pkg.first import First\nclass Second(First): pass\n"),
        ],
        "pkg/first.py",
        false
    )]
    #[case::package_init_reexport(
        &[
            ("pkg/__init__.py", "from .base import FrozenModel\n"),
            ("pkg/base.py", "from pydantic import BaseModel\nclass FrozenModel(BaseModel): pass\n"),
            ("pkg/models.py", "from pkg import FrozenModel\nclass OrderConfig(FrozenModel): pass\n"),
        ],
        "pkg/models.py",
        true
    )]
    #[case::aliased_import(
        &[
            ("pkg/__init__.py", ""),
            ("pkg/base.py", "from pydantic import BaseModel\nclass FrozenModel(BaseModel): pass\n"),
            ("pkg/models.py", "from pkg.base import FrozenModel as Base\nclass OrderConfig(Base): pass\n"),
        ],
        "pkg/models.py",
        true
    )]
    #[case::module_qualified_base(
        &[
            ("pkg/__init__.py", ""),
            ("pkg/base.py", "from pydantic import BaseModel\nclass FrozenModel(BaseModel): pass\n"),
            ("pkg/models.py", "import pkg.base\nclass OrderConfig(pkg.base.FrozenModel): pass\n"),
        ],
        "pkg/models.py",
        true
    )]
    #[case::function_local_base_model_shadowing(
        &[
            ("pkg/__init__.py", ""),
            (
                "pkg/models.py",
                indoc::indoc! {r"
                    def build() -> None:
                        class BaseModel: pass
                        class OrderConfig(BaseModel): pass
                "},
            ),
        ],
        "pkg/models.py",
        false
    )]
    #[case::wildcard_imported_non_pydantic_base_model(
        &[
            ("pkg/__init__.py", ""),
            ("pkg/base.py", "class BaseModel: pass\n"),
            ("pkg/models.py", "from pkg.base import *\nclass OrderConfig(BaseModel): pass\n"),
        ],
        "pkg/models.py",
        false
    )]
    fn class_mro_has_base_in_resolves_cross_module_and_shadowed_bases(
        #[case] files: &[(&str, &str)],
        #[case] target_rel: &str,
        #[case] expected: bool,
    ) {
        let root = Path::new("/repo");
        let source = MemoryFileSource::from_relative_files(root, files);
        let layout = PythonLayout::discover(
            WorkspaceAnchor::new(root.to_path_buf(), None),
            root,
            None,
            &source,
            &[],
        );
        let index = PythonIndex::new(layout, &source);
        let target_path = root.join(target_rel);
        let text = source.read_to_string(&target_path).unwrap();
        let parsed = ParsedFile::new(&text, Language::Python);
        index.seed_from_parsed(&target_path, &parsed);
        let classes = extract_classes(&parsed);
        let context = PythonModuleContext::new(&index, &target_path);
        let target_index = classes.len() - 1;
        assert_eq!(
            context.class_mro_has_base_in(
                &classes,
                target_index,
                &["BaseModel", "pydantic.BaseModel", "pydantic.main.BaseModel"],
            ),
            expected
        );
    }
}
