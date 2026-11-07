//! Flags Python classes that implement every method of an in-scope `Protocol` without inheriting
//! from it.
//!
//! Uses `RuleTarget::All` because in-memory implementations and test doubles in test files lose
//! the same checks; bare `Fake*` classes are left to `fake-without-protocol`. Python-only because
//! Rust traits are nominal: a type satisfies a trait bound only through an `impl` block.

use crate::code_lint::ast::ParsedFile;
use crate::code_lint::ast::python::{PythonBaseClass, PythonClassInfo, extract_classes};
use crate::code_lint::contract::{ProjectCodeRule, RuleTarget};
use crate::code_lint::policy::is_fake_class_name;
use crate::code_lint::project::{ImportedProtocolSummary, ProjectContext, PythonClassId};
use crate::diagnostic::{Diagnostic, Language, RuleName, ViolationTemplate, violation_template};
use crate::rule_declaration::{
    Classification, Consensus, CountOption, Declaration, ImpactedQuality, LanguageDefaults,
    Precision, ProjectExample, Reference, RuleDoc, RuleOptions, Topic,
};
use std::collections::HashSet;
use std::path::Path;

const MIN_METHODS: CountOption = CountOption {
    key: "min-methods",
    doc: "Minimum public methods on a `Protocol` for explicit inheritance to be required.",
    default: LanguageDefaults::new(2, &[]),
};

const TEMPLATE: ViolationTemplate = violation_template! {
    summary: "Class `{class}` implements every method of `Protocol` `{name}` without inheriting from it.",
    rationale: "Without `{name}` in the base class list, the type checker only verifies `{class}` against `{name}` at typed call sites, misses omitted methods on instantiation, and rejects `@override` on its methods.",
    suggestion: "Add `{name}` to the base classes of `{class}`.",
};

/// The rule's declaration.
pub const RULE: ProjectCodeRule<CountOption> = ProjectCodeRule {
    declaration: Declaration {
        name: RuleName("uninherited-protocol"),
        template: &TEMPLATE,
        languages: &[Language::Python],
        options: RuleOptions::code_rule(MIN_METHODS),
        classification: Classification {
            topics: &[Topic::STATIC_TYPING],
            precision: Precision::Heuristic,
            consensus: Consensus::Opinionated,
            impacted_quality: ImpactedQuality::Reliability,
        },
        doc: RuleDoc {
            summary: "Flags Python classes that implement every method of an in-scope Protocol without inheriting from it.",
            what_it_does: indoc::indoc! {r"
                Flags a class that defines every public method of a `typing.Protocol` declared in
                the same file or imported from another project module, when the `Protocol` has at
                least `min-methods` public methods (including those of the `Protocol`s it extends)
                and the class does not inherit from it, directly or through a project base class.
                When the class implements both a `Protocol` and a `Protocol` extending it, only
                the most derived one is reported.

                Private and dunder methods and `@property` getters are not counted as methods,
                on the `Protocol` or on the class. Not checked: `Protocol` and `ABC` classes,
                `Enum` and `NamedTuple` classes (whose metaclass conflicts with `Protocol`), and
                `Fake*` classes without a base class (see `fake-without-protocol`)."},
            why_is_this_bad: indoc::indoc! {r"
                A `Protocol` lets external types you do not control satisfy an interface
                structurally. A project class written to implement a known `Protocol` gives up the
                checks that explicit inheritance provides at its definition:

                - `mypy` and `pyright` check its method signatures against the `Protocol` only
                  where an instance reaches a `Protocol`-typed parameter or variable;
                - a method later added to the `Protocol` does not make instantiating the class
                  an error;
                - `@typing.override` on its methods is rejected, since no base method exists;
                - `inspect.getdoc()` and docstring inheritance cannot reach the `Protocol`'s
                  method docstrings."},
            known_problems: Some(indoc::indoc! {r"
                - Matching uses method names only: a class that shares `min-methods` method
                  names with an unrelated in-scope `Protocol` is flagged.
                - `Protocol`s in third-party libraries outside project source roots, or in project
                  modules not imported into the file, are not checked.
                - A `Protocol` made mostly of properties and attributes, with fewer than
                  `min-methods` methods, is not checked."}),
            references: &[
                Reference {
                    title: "PEP 544: Explicitly Declaring Implementation",
                    url: "https://peps.python.org/pep-0544/#explicitly-declaring-implementation",
                },
                Reference {
                    title: "Python Typing Specification: Protocols",
                    url: "https://typing.python.org/en/latest/spec/protocol.html",
                },
                Reference::NAME_RESOLUTION,
            ],
            examples: &[ProjectExample {
                language: Language::Python,
                context: &[(
                    "pkg/interfaces.py",
                    indoc::indoc! {r"
                        from typing import Protocol

                        class KeyValueStore(Protocol):
                            def get(self, key: str) -> bytes | None: ...
                            def put(self, key: str, value: bytes) -> None: ...
                    "},
                )],
                path: "pkg/service.py",
                flagged: indoc::indoc! {r"
                    from pkg.interfaces import KeyValueStore

                    class MemoryStore:
                        def get(self, key: str) -> bytes | None:
                            return None

                        def put(self, key: str, value: bytes) -> None:
                            pass
                "},
                flagged_span: "MemoryStore",
                fixed: indoc::indoc! {r"
                    from pkg.interfaces import KeyValueStore

                    class MemoryStore(KeyValueStore):
                        def get(self, key: str) -> bytes | None:
                            return None

                        def put(self, key: str, value: bytes) -> None:
                            pass
                "},
            }],
        },
    },
    target: RuleTarget::All,
    check: check_file,
};

fn check_file(
    rule: &ProjectCodeRule<CountOption>,
    path: &Path,
    file: &ParsedFile,
    project: &ProjectContext<'_>,
    min_methods: usize,
) -> Vec<Diagnostic> {
    let classes = extract_classes(file);
    let class_methods: Vec<HashSet<String>> = classes
        .iter()
        .map(PythonClassInfo::public_non_property_method_names)
        .collect();
    let has_candidate_class = classes.iter().enumerate().any(|(idx, class_info)| {
        is_checked_class(class_info) && class_methods[idx].len() >= min_methods
    });
    if !has_candidate_class {
        return Vec::new();
    }
    let current_module = project.python_module_of_path(path);
    let imported_protocols: Vec<ImportedProtocolSummary> =
        current_module.as_deref().map_or_else(Vec::new, |module| {
            project
                .imported_protocols_in_module(module)
                .into_iter()
                .filter(|imported| imported.counted_methods.len() >= min_methods)
                .collect()
        });
    let local_protocols: Vec<LocalProtocol> = (0..classes.len())
        .filter(|&protocol_index| classes[protocol_index].is_protocol())
        .map(|protocol_index| {
            build_local_protocol(
                &classes,
                &class_methods,
                protocol_index,
                current_module.as_deref(),
                project,
            )
        })
        .filter(|protocol| protocol.methods.len() >= min_methods)
        .collect();
    if local_protocols.is_empty() && imported_protocols.is_empty() {
        return Vec::new();
    }

    let mut diagnostics = Vec::new();
    for (index, class_info) in classes.iter().enumerate() {
        if !is_checked_class(class_info) || class_methods[index].len() < min_methods {
            continue;
        }
        for protocol_name in uninherited_protocol_names(
            &classes,
            &class_methods[index],
            &local_protocols,
            &imported_protocols,
            index,
            current_module.as_deref(),
            project,
        ) {
            diagnostics.push(rule.diagnostic_at_node(
                path,
                &class_info.name_node,
                &[("class", &class_info.name), ("name", protocol_name)],
            ));
        }
    }
    diagnostics
}

/// Returns true if `class_info` is a concrete class that can inherit from a `Protocol`, and is not
/// a bare `Fake*` class left to `fake-without-protocol`.
fn is_checked_class(class_info: &PythonClassInfo<'_>) -> bool {
    let is_interface = class_info.is_protocol() || class_info.is_abc();
    let has_metaclass_conflict = class_info.is_enum() || class_info.is_named_tuple();
    let is_bare_fake = is_fake_class_name(&class_info.name)
        && class_info
            .bases
            .iter()
            .all(PythonBaseClass::is_structural_marker);
    !(is_interface || has_metaclass_conflict || is_bare_fake)
}

/// A `Protocol` defined in the target file, with the methods an implementation must define.
struct LocalProtocol {
    /// Index of the `Protocol` in [`extract_classes`] order.
    index: usize,
    /// In-file ancestor class indexes in C3 MRO order.
    in_file_ancestors: Vec<usize>,
    /// Its counted methods and those of the in-file or imported classes it extends.
    methods: HashSet<String>,
    /// Imported `PythonClassId`s extended by this in-file `Protocol`.
    extended_imported_ids: HashSet<PythonClassId>,
}

fn build_local_protocol(
    classes: &[PythonClassInfo<'_>],
    class_methods: &[HashSet<String>],
    protocol_index: usize,
    current_module: Option<&str>,
    project: &ProjectContext<'_>,
) -> LocalProtocol {
    let ancestors = project.resolve_class_ancestors(current_module, classes, protocol_index);
    let mut methods: HashSet<String> = std::iter::once(protocol_index)
        .chain(ancestors.in_file.iter().copied())
        .flat_map(|extended| class_methods[extended].iter().cloned())
        .collect();
    methods.extend(ancestors.cross_module_public_methods);
    LocalProtocol {
        index: protocol_index,
        in_file_ancestors: ancestors.in_file,
        methods,
        extended_imported_ids: ancestors.cross_module,
    }
}

/// Names of the in-file and imported `Protocol`s that `classes[index]` (`defined` methods)
/// implements without inheriting from, keeping only the most derived `Protocol`s.
fn uninherited_protocol_names<'p>(
    classes: &'p [PythonClassInfo<'_>],
    defined: &HashSet<String>,
    local_protocols: &'p [LocalProtocol],
    imported_protocols: &'p [ImportedProtocolSummary],
    index: usize,
    current_module: Option<&str>,
    project: &ProjectContext<'_>,
) -> Vec<&'p str> {
    let ancestors = project.resolve_class_ancestors(current_module, classes, index);

    let mut matched_local: Vec<&LocalProtocol> = local_protocols
        .iter()
        .filter(|protocol| is_bound_protocol(classes, local_protocols, protocol, index))
        .filter(|protocol| protocol.methods.is_subset(defined))
        .filter(|protocol| !ancestors.in_file.contains(&protocol.index))
        .collect();

    let is_shadowed_locally = |name: &str| {
        classes[..index]
            .iter()
            .any(|earlier_class| earlier_class.name == name)
            || matched_local
                .iter()
                .any(|protocol| classes[protocol.index].name == name)
    };

    let mut seen_imported_ids = HashSet::new();
    let mut matched_imported: Vec<&ImportedProtocolSummary> = imported_protocols
        .iter()
        .filter(|imported| !is_shadowed_locally(&imported.local_name))
        .filter(|imported| imported.counted_methods.is_subset(defined))
        .filter(|imported| !ancestors.cross_module.contains(&imported.class_id))
        .filter(|imported| seen_imported_ids.insert(imported.class_id.clone()))
        .collect();

    let extended_local: HashSet<usize> = matched_local
        .iter()
        .flat_map(|protocol| protocol.in_file_ancestors.iter().copied())
        .collect();
    let mut extended_imported: HashSet<PythonClassId> = matched_local
        .iter()
        .flat_map(|protocol| protocol.extended_imported_ids.iter().cloned())
        .collect();
    extended_imported.extend(
        matched_imported
            .iter()
            .flat_map(|imported| imported.ancestor_ids.iter().cloned()),
    );

    matched_local.retain(|protocol| !extended_local.contains(&protocol.index));
    matched_imported.retain(|imported| !extended_imported.contains(&imported.class_id));

    matched_local
        .into_iter()
        .map(|protocol| classes[protocol.index].name.as_str())
        .chain(
            matched_imported
                .into_iter()
                .map(|imported| imported.local_name.as_str()),
        )
        .collect()
}

/// Returns true if `protocol` is the `Protocol` its name refers to from `classes[index]`: the
/// last of that name defined before it, or else the first after it.
fn is_bound_protocol(
    classes: &[PythonClassInfo<'_>],
    protocols: &[LocalProtocol],
    protocol: &LocalProtocol,
    index: usize,
) -> bool {
    let name = &classes[protocol.index].name;
    let same_name = || {
        protocols
            .iter()
            .filter(|other| classes[other.index].name == *name)
    };
    same_name()
        .rfind(|other| other.index < index)
        .or_else(|| same_name().find(|other| other.index > index))
        .is_some_and(|bound| bound.index == protocol.index)
}

#[cfg(test)]
crate::test_utils::rule_test!(
    RULE,
    {
        Python => {
            pass: [
                inherits_protocol => r#"
                    class Store(Protocol):
                        def read(self, key: str) -> bytes: ...
                        def write(self, key: str, data: bytes) -> None: ...

                    class MemoryStore(Store):
                        def read(self, key: str) -> bytes:
                            return b""

                        def write(self, key: str, data: bytes) -> None:
                            pass
                "#,
                inherits_generic_protocol => r#"
                    class Store(Protocol[T]):
                        def read(self, key: str) -> T: ...
                        def write(self, key: str, data: T) -> None: ...

                    class IntStore(Store[int]):
                        def read(self, key: str) -> int:
                            return 0

                        def write(self, key: str, data: int) -> None:
                            pass
                "#,
                inherits_protocol_through_local_base => r#"
                    class Store(Protocol):
                        def read(self, key: str) -> bytes: ...
                        def write(self, key: str, data: bytes) -> None: ...

                    class BaseStore(Store):
                        pass

                    class CachingStore(BaseStore):
                        def read(self, key: str) -> bytes:
                            return b""

                        def write(self, key: str, data: bytes) -> None:
                            pass
                "#,
                inherits_imported_protocol => {
                    "pkg/interfaces.py" => r#"
                        from typing import Protocol

                        class Store(Protocol):
                            def load(self, key: str) -> str: ...
                            def save(self, key: str, value: str) -> None: ...
                    "#,
                    "pkg/service.py" => r#"
                        from .interfaces import Store

                        class MemoryStore(Store):
                            def load(self, key: str) -> str:
                                return key

                            def save(self, key: str, value: str) -> None:
                                pass
                    "#,
                },
                inherits_imported_protocol_through_imported_base => {
                    "pkg/interfaces.py" => r#"
                        from typing import Protocol

                        class Store(Protocol):
                            def load(self, key: str) -> str: ...
                            def save(self, key: str, value: str) -> None: ...
                    "#,
                    "pkg/base.py" => r#"
                        from .interfaces import Store

                        class BaseStore(Store):
                            def load(self, key: str) -> str:
                                return key
                    "#,
                    "pkg/service.py" => r#"
                        from .base import BaseStore
                        from .interfaces import Store

                        class CachingStore(BaseStore):
                            def load(self, key: str) -> str:
                                return key

                            def save(self, key: str, value: str) -> None:
                                pass
                    "#,
                },
                ignores_unimported_protocol_in_other_module => {
                    "pkg/interfaces.py" => r#"
                        from typing import Protocol

                        def helper() -> int:
                            return 1

                        class Store(Protocol):
                            def load(self, key: str) -> str: ...
                            def save(self, key: str, value: str) -> None: ...
                    "#,
                    "pkg/service.py" => r#"
                        from .interfaces import helper

                        class UnrelatedCache:
                            def load(self, key: str) -> str:
                                return str(helper())

                            def save(self, key: str, value: str) -> None:
                                pass
                    "#,
                },
                protocol_redeclaring_methods => r#"
                    class Store(Protocol):
                        def read(self, key: str) -> bytes: ...
                        def write(self, key: str, data: bytes) -> None: ...

                    class AsyncStore(Protocol):
                        async def read(self, key: str) -> bytes: ...
                        async def write(self, key: str, data: bytes) -> None: ...
                "#,
                abc_declaring_methods => r#"
                    class Store(Protocol):
                        def read(self, key: str) -> bytes: ...
                        def write(self, key: str, data: bytes) -> None: ...

                    class AbstractStore(ABC):
                        @abstractmethod
                        def read(self, key: str) -> bytes: ...

                        @abstractmethod
                        def write(self, key: str, data: bytes) -> None: ...
                "#,
                enum_defining_methods => r#"
                    class Store(Protocol):
                        def read(self, key: str) -> bytes: ...
                        def write(self, key: str, data: bytes) -> None: ...

                    class NullStore(Enum):
                        INSTANCE = 0

                        def read(self, key: str) -> bytes:
                            return b""

                        def write(self, key: str, data: bytes) -> None:
                            pass
                "#,
                named_tuple_defining_methods => r#"
                    class Store(Protocol):
                        def read(self, key: str) -> bytes: ...
                        def write(self, key: str, data: bytes) -> None: ...

                    class StoreEntry(NamedTuple):
                        data: bytes

                        def read(self, key: str) -> bytes:
                            return self.data

                        def write(self, key: str, data: bytes) -> None:
                            pass
                "#,
                protocol_below_min_methods => r#"
                    class Closable(Protocol):
                        def close(self) -> None: ...

                    class Resource:
                        def close(self) -> None:
                            pass
                "#,
                underscore_prefixed_methods_not_counted => r#"
                    class Managed(Protocol):
                        def open(self) -> None: ...
                        def __enter__(self) -> Self: ...
                        def _reset(self) -> None: ...

                    class Session:
                        def open(self) -> None:
                            pass

                        def __enter__(self) -> Self:
                            return self

                        def _reset(self) -> None:
                            pass
                "#,
                protocol_property_not_counted => r#"
                    class NamedReader(Protocol):
                        def read(self) -> bytes: ...

                        @property
                        def name(self) -> str: ...

                    class FileReader:
                        def read(self) -> bytes:
                            return b""

                        @property
                        def name(self) -> str:
                            return "file"
                "#,
                property_not_counted_as_implementation => r#"
                    class Reader(Protocol):
                        def read(self) -> bytes: ...
                        def close(self) -> None: ...

                    class CachedReader:
                        @property
                        def read(self) -> bytes:
                            return b""

                        def close(self) -> None:
                            pass
                "#,
                non_typing_protocol_base => r#"
                    from asyncio import Protocol

                    class FramingProtocol(Protocol):
                        def data_received(self, data: bytes) -> None: ...
                        def connection_lost(self, exc: Exception | None) -> None: ...

                    class Framer:
                        def data_received(self, data: bytes) -> None:
                            pass

                        def connection_lost(self, exc: Exception | None) -> None:
                            pass
                "#,
                bare_fake_left_to_fake_without_protocol => r#"
                    class Store(Protocol):
                        def read(self, key: str) -> bytes: ...
                        def write(self, key: str, data: bytes) -> None: ...

                    class FakeStore:
                        def read(self, key: str) -> bytes:
                            return b""

                        def write(self, key: str, data: bytes) -> None:
                            pass
                "#,
                missing_protocol_method => r#"
                    class Store(Protocol):
                        def read(self, key: str) -> bytes: ...
                        def write(self, key: str, data: bytes) -> None: ...

                    class ReadOnlyStore:
                        def read(self, key: str) -> bytes:
                            return b""
                "#,
            ],
            fail: [
                uninherited_implementation => r#"
                    class Store(Protocol):
                        def read(self, key: str) -> bytes: ...
                        def write(self, key: str, data: bytes) -> None: ...

                    class MemoryStore:
                        def read(self, key: str) -> bytes:
                            return b""

                        def write(self, key: str, data: bytes) -> None:
                            pass
                "# => "MemoryStore",
                uninherited_imported_protocol => {
                    "pkg/interfaces.py" => r#"
                        from typing import Protocol

                        class Store(Protocol):
                            def load(self, key: str) -> str: ...
                            def save(self, key: str, value: str) -> None: ...
                    "#,
                    "pkg/service.py" => r#"
                        from .interfaces import Store

                        class MemoryStore:
                            def load(self, key: str) -> str:
                                return key

                            def save(self, key: str, value: str) -> None:
                                pass
                    "#,
                } => ("pkg/service.py", "MemoryStore"),
                uninherited_reexported_protocol_via_init => {
                    "pkg/__init__.py" => r#"
                        from .interfaces import Store
                    "#,
                    "pkg/interfaces.py" => r#"
                        from typing import Protocol

                        class Store(Protocol):
                            def load(self, key: str) -> str: ...
                            def save(self, key: str, value: str) -> None: ...
                    "#,
                    "pkg/service.py" => r#"
                        from pkg import Store as KVStore

                        class MemoryStore:
                            def load(self, key: str) -> str:
                                return key

                            def save(self, key: str, value: str) -> None:
                                pass
                    "#,
                } => ("pkg/service.py", "MemoryStore"),
                most_derived_imported_protocol_reported => {
                    "pkg/interfaces.py" => r#"
                        from typing import Protocol

                        class Reader(Protocol):
                            def read(self, key: str) -> str: ...
                            def exists(self, key: str) -> bool: ...

                        class ReadWriter(Reader, Protocol):
                            def write(self, key: str, value: str) -> None: ...
                    "#,
                    "pkg/service.py" => r#"
                        from .interfaces import Reader, ReadWriter

                        class FileBuffer:
                            def read(self, key: str) -> str:
                                return key

                            def exists(self, key: str) -> bool:
                                return True

                            def write(self, key: str, value: str) -> None:
                                pass
                    "#,
                } => ("pkg/service.py", "FileBuffer"),
                same_named_classes_in_different_functions => {
                    "pkg/interfaces.py" => r#"
                        from typing import Protocol

                        class Store(Protocol):
                            def load(self, key: str) -> str: ...
                            def save(self, key: str, value: str) -> None: ...
                    "#,
                    "pkg/test_service.py" => r#"
                        from .interfaces import Store

                        def test_valid() -> None:
                            class LocalStore(Store):
                                def load(self, key: str) -> str:
                                    return key

                                def save(self, key: str, value: str) -> None:
                                    pass

                        def test_uninherited() -> None:
                            class LocalStore:
                                def load(self, key: str) -> str:
                                    return key

                                def save(self, key: str, value: str) -> None:
                                    pass
                    "#,
                } => ("pkg/test_service.py", "LocalStore"),
                implementation_defined_before_protocol => r#"
                    class MemoryStore:
                        def read(self, key: str) -> bytes:
                            return b""

                        def write(self, key: str, data: bytes) -> None:
                            pass

                    class Store(Protocol):
                        def read(self, key: str) -> bytes: ...
                        def write(self, key: str, data: bytes) -> None: ...
                "# => "MemoryStore",
                async_methods => r#"
                    class Store(Protocol):
                        async def read(self, key: str) -> bytes: ...
                        async def write(self, key: str, data: bytes) -> None: ...

                    class MemoryStore:
                        async def read(self, key: str) -> bytes:
                            return b""

                        async def write(self, key: str, data: bytes) -> None:
                            pass
                "# => "MemoryStore",
                extra_methods_on_implementation => r#"
                    class Store(Protocol):
                        def read(self, key: str) -> bytes: ...
                        def write(self, key: str, data: bytes) -> None: ...

                    class MemoryStore:
                        def read(self, key: str) -> bytes:
                            return b""

                        def write(self, key: str, data: bytes) -> None:
                            pass

                        def clear(self) -> None:
                            pass
                "# => "MemoryStore",
                generic_typing_extensions_protocol => r#"
                    from typing_extensions import Protocol

                    class Store(Protocol[T]):
                        def read(self, key: str) -> T: ...
                        def write(self, key: str, data: T) -> None: ...

                    class IntStore:
                        def read(self, key: str) -> int:
                            return 0

                        def write(self, key: str, data: int) -> None:
                            pass
                "# => "IntStore",
                qualified_typing_protocol => r#"
                    import typing

                    class Store(typing.Protocol):
                        def read(self, key: str) -> bytes: ...
                        def write(self, key: str, data: bytes) -> None: ...

                    class MemoryStore:
                        def read(self, key: str) -> bytes:
                            return b""

                        def write(self, key: str, data: bytes) -> None:
                            pass
                "# => "MemoryStore",
                implementation_with_unrelated_base => r#"
                    class Store(Protocol):
                        def read(self, key: str) -> bytes: ...
                        def write(self, key: str, data: bytes) -> None: ...

                    class CachedStore(LoggingMixin):
                        def read(self, key: str) -> bytes:
                            return b""

                        def write(self, key: str, data: bytes) -> None:
                            pass
                "# => "CachedStore",
                fake_with_helper_base => r#"
                    class Store(Protocol):
                        def read(self, key: str) -> bytes: ...
                        def write(self, key: str, data: bytes) -> None: ...

                    class FakeStore(BaseFake):
                        def read(self, key: str) -> bytes:
                            return b""

                        def write(self, key: str, data: bytes) -> None:
                            pass
                "# => "FakeStore",
                methods_of_extended_protocol_counted => r#"
                    class Reader(Protocol):
                        def read(self) -> bytes: ...

                    class ReadWriter(Reader, Protocol):
                        def write(self, data: bytes) -> None: ...

                    class Buffer:
                        def read(self) -> bytes:
                            return b""

                        def write(self, data: bytes) -> None:
                            pass
                "# => "Buffer",
                most_derived_protocol_reported => r#"
                    class Reader(Protocol):
                        def read(self) -> bytes: ...
                        def seek(self, offset: int) -> None: ...

                    class ReadWriter(Reader, Protocol):
                        def write(self, data: bytes) -> None: ...

                    class Buffer:
                        def read(self) -> bytes:
                            return b""

                        def seek(self, offset: int) -> None:
                            pass

                        def write(self, data: bytes) -> None:
                            pass
                "# => "Buffer",
            ],
        },
    }
);
