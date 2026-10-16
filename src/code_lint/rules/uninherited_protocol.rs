//! Flags Python classes that implement every method of an in-file `Protocol` without inheriting
//! from it.
//!
//! Uses `RuleTarget::All` because in-memory implementations and test doubles in test files lose
//! the same checks; bare `Fake*` classes are left to `fake-without-protocol`. Python-only because
//! Rust traits are nominal: a type satisfies a trait bound only through an `impl` block.

use crate::code_lint::ast::ParsedFile;
use crate::code_lint::ast::python::{
    PythonBaseClass, PythonClassInfo, extract_classes, is_fake_class_name, local_ancestors,
};
use crate::code_lint::contract::{CodeRule, RuleTarget};
use crate::diagnostic::{Diagnostic, Language, RuleName, ViolationTemplate, violation_template};
use crate::rule_declaration::{
    Classification, Consensus, CountOption, Declaration, Example, ImpactedQuality,
    LanguageDefaults, Precision, Reference, RuleDoc, RuleOptions, Topic,
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
pub const RULE: CodeRule<CountOption> = CodeRule {
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
            summary: "Flags Python classes that implement every method of an in-file Protocol without inheriting from it.",
            what_it_does: indoc::indoc! {r"
                Flags a class that defines every public method of a `typing.Protocol` declared in
                the same file, when the `Protocol` has at least `min-methods` public methods
                (including those of the in-file `Protocol`s it extends) and the class does not
                inherit from it, directly or through an in-file base class. When the class
                implements both a `Protocol` and a `Protocol` extending it, only the most derived
                one is reported.

                Private and dunder methods and `@property` getters are not counted as methods,
                on the `Protocol` or on the class. Not checked: `Protocol` and `ABC` classes,
                `Enum` and `NamedTuple` classes (whose metaclass conflicts with `Protocol`), and
                `Fake*` classes without a base class (see `fake-without-protocol`)."},
            why_is_this_bad: indoc::indoc! {r"
                A `Protocol` lets external types you do not control satisfy an interface
                structurally. A class written next to the `Protocol` to implement it gives up the
                checks that explicit inheritance provides at its definition:

                - `mypy` and `pyright` check its method signatures against the `Protocol` only
                  where an instance reaches a `Protocol`-typed parameter or variable;
                - a method later added to the `Protocol` does not make instantiating the class
                  an error;
                - `@typing.override` on its methods is rejected, since no base method exists;
                - `inspect.getdoc()` and in-file docstring inheritance cannot reach the
                  `Protocol`'s method docstrings."},
            known_problems: Some(indoc::indoc! {r"
                - Matching uses method names only: a class that shares `min-methods` method
                  names with an unrelated `Protocol` in the same file is flagged.
                - A `Protocol` imported from another module is not seen.
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
            examples: &[Example {
                language: Language::Python,
                flagged: indoc::indoc! {r"
                    class KeyValueStore(Protocol):
                        def get(self, key: str) -> bytes | None: ...
                        def put(self, key: str, value: bytes) -> None: ...

                    class MemoryStore:
                        def get(self, key: str) -> bytes | None:
                            return None

                        def put(self, key: str, value: bytes) -> None:
                            pass
                "},
                flagged_span: "MemoryStore",
                fixed: indoc::indoc! {r"
                    class KeyValueStore(Protocol):
                        def get(self, key: str) -> bytes | None: ...
                        def put(self, key: str, value: bytes) -> None: ...

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
    rule: &CodeRule<CountOption>,
    path: &Path,
    file: &ParsedFile,
    min_methods: usize,
) -> Vec<Diagnostic> {
    let classes = extract_classes(file);
    let protocols: Vec<LocalProtocol> = (0..classes.len())
        .filter(|&protocol_index| classes[protocol_index].is_protocol())
        .map(|protocol_index| LocalProtocol {
            index: protocol_index,
            methods: std::iter::once(protocol_index)
                .chain(local_ancestors(&classes, protocol_index))
                .flat_map(|extended| counted_methods(&classes[extended]))
                .collect(),
        })
        .collect();
    let mut diagnostics = Vec::new();
    for (index, class) in classes.iter().enumerate() {
        if !is_checked_class(class) {
            continue;
        }
        for protocol in uninherited_protocols(&classes, &protocols, index, min_methods) {
            diagnostics.push(rule.diagnostic_at_node(
                path,
                &class.name_node,
                &[
                    ("class", &class.name),
                    ("name", &classes[protocol.index].name),
                ],
            ));
        }
    }
    diagnostics
}

/// Returns true if `class` is a concrete class that can inherit from a `Protocol`, and is not a
/// bare `Fake*` class left to `fake-without-protocol`.
fn is_checked_class(class: &PythonClassInfo<'_>) -> bool {
    let is_interface = class.is_protocol() || class.is_abc();
    let has_metaclass_conflict = class.is_enum() || class.is_named_tuple();
    let is_bare_fake = is_fake_class_name(&class.name)
        && class
            .bases
            .iter()
            .all(PythonBaseClass::is_structural_marker);
    !(is_interface || has_metaclass_conflict || is_bare_fake)
}

/// The `protocols` with at least `min_methods` methods that `classes[index]` defines all of
/// without inheriting from them, keeping only the most derived of a `Protocol` and its in-file
/// extensions.
///
/// Each `Protocol` name refers to the last `Protocol` of that name defined before the class, or
/// the first one after it, so a redefined `Protocol` is matched once.
fn uninherited_protocols<'p>(
    classes: &[PythonClassInfo<'_>],
    protocols: &'p [LocalProtocol],
    index: usize,
    min_methods: usize,
) -> Vec<&'p LocalProtocol> {
    let defined = counted_methods(&classes[index]);
    let ancestor_names: HashSet<&str> = local_ancestors(classes, index)
        .into_iter()
        .map(|ancestor| classes[ancestor].name.as_str())
        .collect();
    let mut matched: Vec<&LocalProtocol> = protocols
        .iter()
        .filter(|protocol| is_bound_protocol(classes, protocols, protocol, index))
        .filter(|protocol| protocol.methods.len() >= min_methods)
        .filter(|protocol| protocol.methods.is_subset(&defined))
        .filter(|protocol| !ancestor_names.contains(classes[protocol.index].name.as_str()))
        .collect();
    let extended: HashSet<usize> = matched
        .iter()
        .flat_map(|protocol| local_ancestors(classes, protocol.index))
        .collect();
    matched.retain(|protocol| !extended.contains(&protocol.index));
    matched
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

/// A `Protocol` defined in the file, with the methods an implementation must define.
struct LocalProtocol {
    /// Index of the `Protocol` in [`extract_classes`] order.
    index: usize,
    /// Its counted methods and those of the in-file classes it extends.
    methods: HashSet<String>,
}

/// Names of the methods defined directly in `class` that count toward a `Protocol` contract:
/// public, and not a property accessor.
fn counted_methods(class: &PythonClassInfo<'_>) -> HashSet<String> {
    let methods = class.methods();
    let properties: HashSet<&str> = methods
        .iter()
        .filter(|method| method.is_property_getter())
        .map(|method| method.name.as_str())
        .collect();
    methods
        .iter()
        .filter(|method| !method.name.starts_with('_'))
        .filter(|method| !properties.contains(method.name.as_str()))
        .map(|method| method.name.clone())
        .collect()
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
