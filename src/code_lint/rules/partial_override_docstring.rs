//! Flags `@override` method docstrings that drop a contract section of the docstring they replace.
//!
//! Design: `docs/dev/trivial_override_docstring/03_design.md`. Section kinds are compared, never
//! header text or entries; only ancestors defined earlier in the same file are seen.

use crate::code_lint::ast::python::{DocstringSection, DocstringSectionKind, PythonDocstring};
use crate::code_lint::ast::{self, ParsedFile};
use crate::code_lint::contract::{CodeRule, RuleTarget};
use crate::diagnostic::{Diagnostic, Language, RuleName, ViolationTemplate, violation_template};
use crate::rule_declaration::{
    Classification, Consensus, Declaration, Example, ImpactedQuality, Precision, Reference,
    RuleDoc, RuleOptions, Topic,
};
use std::path::Path;

const TEMPLATE: ViolationTemplate = violation_template! {
    summary: "Docstring of `{function}` has no {sections} section, unlike the `{class}` docstring it replaces.",
    rationale: "A method's own docstring replaces the inherited one in `help()`, generated documentation and IDE hovers, so readers of the override no longer see that part of the contract.",
    suggestion: "Add the missing sections if the docstring states how the override differs from the base; otherwise remove the docstring so that the complete one is inherited.",
};

/// The rule's declaration.
pub const RULE: CodeRule = CodeRule {
    declaration: Declaration {
        name: RuleName("partial-override-docstring"),
        template: &TEMPLATE,
        languages: &[Language::Python],
        options: RuleOptions::code_rule(()),
        classification: Classification {
            topics: &[Topic::DOCSTRINGS],
            precision: Precision::Exact,
            consensus: Consensus::Opinionated,
            impacted_quality: ImpactedQuality::Maintainability,
        },
        doc: RuleDoc {
            summary: "Flags `@override` method docstrings that omit an `Args:`, `Returns:`, `Yields:` or `Raises:` section of the inherited docstring.",
            what_it_does: indoc::indoc! {r"
                Flags the docstring of a method decorated with `@override` (`typing.override` or
                `typing_extensions.override`) when the docstring it replaces has an `Args:`,
                `Returns:`, `Yields:` or `Raises:` section of a kind the override's docstring lacks.
                The inherited docstring is the one `inspect.getdoc` would show without the
                override's: the first ancestor class defined earlier in the file whose method of
                the same name has a docstring. Section kinds are compared, so `Arguments:`
                satisfies `Args:`; `Returns:` and `Yields:` satisfy each other, as does a summary
                starting with `Returns` or `Yields`. A base `Raises:` section listing only
                `NotImplementedError` is not required, and other sections (`Example:`, `Note:`)
                never are. An override has no docstring or a complete one, which is stricter than
                the Google style guide's docstring with at least the differences. A docstring equal
                to the inherited summary is left to `copied-base-docstring`."},
            why_is_this_bad: indoc::indoc! {r"
                Documentation tools never merge an override's docstring with the inherited one:
                `help()`, Sphinx, Pyright hovers and `inspect.getdoc` show only the override's. A
                one-line override docstring over a base that documents its parameters, result and
                errors hides that contract from every reader of the override, even when the
                override changes none of it."},
            known_problems: Some(indoc::indoc! {r"
                Only ancestors defined earlier in the same file are seen. Entries are not compared:
                an override `Args:` section that omits one of the base's parameters is not flagged.
                Methods without `@override` are not inspected: enable mypy's `explicit-override`
                error code or Pyright's `reportImplicitOverride` to make every override explicit.
                A deferral phrase such as `See base class.` over a sectioned base is also reported
                by `deferral-docstring`."}),
            references: &[
                Reference {
                    title: "Google Python Style Guide §3.8.3.1: Overridden Methods",
                    url: "https://google.github.io/styleguide/pyguide.html#3831-overridden-methods",
                },
                Reference {
                    title: "inspect.getdoc: docstring inheritance",
                    url: "https://docs.python.org/3/library/inspect.html#inspect.getdoc",
                },
            ],
            examples: &[Example {
                language: Language::Python,
                flagged: indoc::indoc! {r#"
                    from typing import override

                    class SessionService:
                        def create(self, name: str) -> Session:
                            """Creates a session.

                            Args:
                                name: The session name, unique per user.

                            Returns:
                                The new session.
                            """
                            raise NotImplementedError

                    class InMemorySessionService(SessionService):
                        @override
                        def create(self, name: str) -> Session:
                            """Creates a session in memory."""
                            return Session(name)
                "#},
                flagged_span: r#""""Creates a session in memory.""""#,
                fixed: indoc::indoc! {r#"
                    from typing import override

                    class SessionService:
                        def create(self, name: str) -> Session:
                            """Creates a session.

                            Args:
                                name: The session name, unique per user.

                            Returns:
                                The new session.
                            """
                            raise NotImplementedError

                    class InMemorySessionService(SessionService):
                        @override
                        def create(self, name: str) -> Session:
                            """Creates a session in memory.

                            Args:
                                name: The session name, unique per user.

                            Returns:
                                The new session, lost when the process exits.
                            """
                            return Session(name)
                "#},
            }],
        },
    },
    target: RuleTarget::All,
    check: check_file,
};

/// The contract section kinds, in rendering order, with their canonical header.
const CONTRACT_SECTIONS: &[(DocstringSectionKind, &str)] = &[
    (DocstringSectionKind::Args, "Args"),
    (DocstringSectionKind::Returns, "Returns"),
    (DocstringSectionKind::Yields, "Yields"),
    (DocstringSectionKind::Raises, "Raises"),
];
const NOT_IMPLEMENTED_ERROR: &str = "NotImplementedError";

const FUNCTION: &str = "function";
const SECTIONS: &str = "sections";
const CLASS: &str = "class";

fn check_file(rule: &CodeRule, path: &Path, file: &ParsedFile, (): ()) -> Vec<Diagnostic> {
    let classes = ast::python::extract_classes(file);
    let mut diagnostics = Vec::new();
    for (index, class) in classes.iter().enumerate() {
        for method in class.methods() {
            if !method.has_override_decorator() {
                continue;
            }
            let Some(docstring) = method.docstring() else {
                continue;
            };
            let Some((ancestor, inherited)) =
                ast::python::inherited_method_docstring(&classes, index, &method.name)
            else {
                continue;
            };
            if docstring.copies_summary_of(&inherited) {
                continue;
            }
            let required = required_kinds(&inherited);
            let documented = documented_kinds(&docstring);
            let missing: Vec<String> = CONTRACT_SECTIONS
                .iter()
                .filter(|(kind, _)| required.contains(kind) && !documented.contains(kind))
                .map(|(_, header)| format!("`{header}:`"))
                .collect();
            if !missing.is_empty() {
                diagnostics.push(rule.diagnostic_at_node(
                    path,
                    &docstring.node(),
                    &[
                        (FUNCTION, &method.name),
                        (SECTIONS, &missing.join(" or ")),
                        (CLASS, &ancestor.name),
                    ],
                ));
            }
        }
    }
    diagnostics
}

/// The section kinds of the inherited docstring, without a `Raises:` section that only
/// documents an abstract method.
fn required_kinds(inherited: &PythonDocstring<'_>) -> Vec<DocstringSectionKind> {
    inherited
        .sections()
        .iter()
        .filter(|section| !is_abstract_raises(section))
        .map(|section| section.kind)
        .collect()
}

/// Returns true if `section` is a `Raises:` section whose entries are all `NotImplementedError`.
fn is_abstract_raises(section: &DocstringSection) -> bool {
    section.kind == DocstringSectionKind::Raises
        && !section.entries.is_empty()
        && section
            .entries
            .iter()
            .all(|entry| entry.bare_name() == NOT_IMPLEMENTED_ERROR)
}

/// The section kinds the override's docstring documents; a `Returns:` or `Yields:` section, or
/// a summary starting with a result verb, documents both result kinds.
fn documented_kinds(docstring: &PythonDocstring<'_>) -> Vec<DocstringSectionKind> {
    let mut kinds: Vec<DocstringSectionKind> = docstring
        .sections()
        .iter()
        .map(|section| section.kind)
        .collect();
    if docstring.describes_result() {
        kinds.extend([DocstringSectionKind::Returns, DocstringSectionKind::Yields]);
    }
    kinds
}

#[cfg(test)]
crate::test_utils::rule_test!(
    RULE,
    {
        Python => {
            pass: [
                no_docstring => r#"
                    from typing import override

                    class Store:
                        def read(self, key: str) -> bytes:
                            """Reads a value.

                            Args:
                                key: The key to read.
                            """

                    class FileStore(Store):
                        @override
                        def read(self, key: str) -> bytes:
                            return b''
                "#,
                complete_override_docstring => r#"
                    from typing import override

                    class Store:
                        def read(self, key: str) -> bytes:
                            """Reads a value.

                            Args:
                                key: The key to read.

                            Returns:
                                The stored bytes.

                            Raises:
                                KeyError: The key is absent.
                            """

                    class FileStore(Store):
                        @override
                        def read(self, key: str) -> bytes:
                            """Reads a value from disk.

                            Args:
                                key: The file name.

                            Returns:
                                The file content.

                            Raises:
                                KeyError: The file is absent.
                            """
                "#,
                difference_over_unsectioned_base => r#"
                    from typing import override

                    class Store:
                        def read(self, key: str) -> bytes:
                            """Reads the value stored under a key."""

                    class CachedStore(Store):
                        @override
                        def read(self, key: str) -> bytes:
                            """Reads from the in-memory cache before the disk."""
                "#,
                undocumented_ancestor_exempt => r#"
                    from typing import override

                    class LocalStore(storage.Store):
                        def read(self, key: str) -> bytes:
                            return b''

                    class FileStore(LocalStore):
                        @override
                        def read(self, key: str) -> bytes:
                            """Reads a value from disk."""
                "#,
                base_defined_after_subclass_exempt => r#"
                    from typing import override

                    class FileStore(Store):
                        @override
                        def read(self, key: str) -> bytes:
                            """Reads a value from disk."""

                    class Store:
                        def read(self, key: str) -> bytes:
                            """Reads a value.

                            Args:
                                key: The key to read.
                            """
                "#,
                module_level_function_exempt => r#"
                    from typing import override

                    class Store:
                        def read(self, key: str) -> bytes:
                            """Reads a value.

                            Args:
                                key: The key to read.
                            """

                    class FileStore(Store):
                        pass

                    @override
                    def read(key: str) -> bytes:
                        """Reads a value from disk."""
                "#,
                undecorated_override_exempt => r#"
                    class Store:
                        def read(self, key: str) -> bytes:
                            """Reads a value.

                            Args:
                                key: The key to read.
                            """

                    class FileStore(Store):
                        def read(self, key: str) -> bytes:
                            """Reads a value from disk."""
                "#,
                non_contract_sections_exempt => r#"
                    from typing import override

                    class Store:
                        def read(self, key: str) -> bytes:
                            """Reads a value.

                            Example:
                                store.read('alice')

                            Note:
                                Reads are not cached.
                            """

                    class FileStore(Store):
                        @override
                        def read(self, key: str) -> bytes:
                            """Reads a value from disk."""
                "#,
                other_header_of_same_kind_exempt => r#"
                    from typing import override

                    class Store:
                        def read(self, key: str) -> bytes:
                            """Reads a value.

                            Args:
                                key: The key to read.

                            Returns:
                                The stored bytes.
                            """

                    class FileStore(Store):
                        @override
                        def read(self, key: str) -> bytes:
                            """Reads a value from disk.

                            Arguments:
                                key: The file name.

                            Return:
                                The file content.
                            """
                "#,
                yields_for_returns_exempt => r#"
                    from typing import override

                    class Store:
                        def rows(self) -> Iterator[bytes]:
                            """Lists the stored values.

                            Returns:
                                An iterator over the values.
                            """

                    class FileStore(Store):
                        @override
                        def rows(self) -> Iterator[bytes]:
                            """Lists the stored files.

                            Yields:
                                The content of each file.
                            """
                "#,
                result_verb_summary_exempt => r#"
                    from typing import override

                    class Store:
                        def size(self) -> int:
                            """Counts the stored values.

                            Returns:
                                The number of values.
                            """

                    class FileStore(Store):
                        @override
                        def size(self) -> int:
                            """Returns the number of files in the directory."""
                "#,
                summary_copy_exempt => r#"
                    from typing import override

                    class Store:
                        def read(self, key: str) -> bytes:
                            """Reads a value.

                            Args:
                                key: The key to read.
                            """

                    class FileStore(Store):
                        @override
                        def read(self, key: str) -> bytes:
                            """Reads a value."""
                "#,
                abstract_raises_exempt => r#"
                    from typing import override

                    class Store:
                        def clear(self) -> None:
                            """Removes every value.

                            Raises:
                                NotImplementedError: Subclasses implement it.
                            """
                            raise NotImplementedError

                    class FileStore(Store):
                        @override
                        def clear(self) -> None:
                            """Deletes every file in the directory."""
                "#,
            ],
            fail: [
                summary_over_sectioned_base => r#"
                    from typing import override

                    class SessionService:
                        def create(self, name: str, owner: str) -> Session:
                            """Creates a session.

                            Args:
                                name: The session name.
                                owner: The user who owns the session.

                            Returns:
                                The new session.

                            Raises:
                                ValueError: The name is taken.
                            """

                    class InMemorySessionService(SessionService):
                        @override
                        def create(self, name: str, owner: str) -> Session:
                            """Creates a session in memory."""
                "# => r#""""Creates a session in memory.""""#,
                unchanged_signature_missing_args => r#"
                    from typing import override

                    class Store:
                        def delete(self, key: str) -> None:
                            """Deletes a value.

                            Args:
                                key: The key to delete.
                            """

                    class FileStore(Store):
                        @override
                        def delete(self, key: str) -> None:
                            """Deletes the file and its parent directory when empty."""
                "# => r#""""Deletes the file and its parent directory when empty.""""#,
                sectioned_override_missing_raises => r#"
                    from typing import override

                    class Store:
                        def read(self, key: str) -> bytes:
                            """Reads a value.

                            Args:
                                key: The key to read.

                            Raises:
                                KeyError: The key is absent.
                            """

                    class FileStore(Store):
                        @override
                        def read(self, key: str) -> bytes:
                            """Reads a value from disk.

                            Args:
                                key: The file name.
                            """
                "# => r#""""Reads a value from disk.

                    Args:
                        key: The file name.
                    """"#,
                raises_with_other_entries_required => r#"
                    from typing import override

                    class Store:
                        def clear(self) -> None:
                            """Removes every value.

                            Raises:
                                NotImplementedError: Subclasses implement it.
                                PermissionError: The store is read-only.
                            """

                    class FileStore(Store):
                        @override
                        def clear(self) -> None:
                            """Deletes every file in the directory."""
                "# => r#""""Deletes every file in the directory.""""#,
                undocumented_nearer_ancestor_skipped => r#"
                    from typing import override

                    class Store:
                        def read(self, key: str) -> bytes:
                            """Reads a value.

                            Args:
                                key: The key to read.
                            """

                    class LocalStore(Store):
                        def read(self, key: str) -> bytes:
                            return b''

                    class FileStore(LocalStore):
                        @override
                        def read(self, key: str) -> bytes:
                            """Reads a value from disk."""
                "# => r#""""Reads a value from disk.""""#,
            ],
        },
    }
);
