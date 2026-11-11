//! Flags `@override` method docstrings that repeat the docstring they inherit.
//!
//! Design: `docs/dev/trivial_override_docstring/03_design.md`. Ancestors are resolved in Python C3
//! MRO order across in-file and imported project classes; the match is equality with the whole
//! inherited docstring or with its summary paragraph, after normalization.

use crate::code_lint::ast::ParsedFile;
use crate::code_lint::contract::{ProjectCodeRule, RuleTarget};
use crate::code_lint::project::ProjectContext;
use crate::diagnostic::{Diagnostic, Language, RuleName, ViolationTemplate, violation_template};
use crate::rule_declaration::{
    Classification, Consensus, Declaration, ImpactedQuality, Precision, ProjectExample, Reference,
    RuleDoc, RuleOptions, Topic,
};
use std::path::Path;

const TEMPLATE: ViolationTemplate = violation_template! {
    summary: "Docstring of `{function}` repeats text of the docstring it inherits from `{class}`.",
    rationale: "Without it, `help()`, generated documentation and IDE hovers show the complete inherited docstring; the copy shows the same text or less, and drifts when either contract changes.",
    suggestion: "Remove the docstring so that the base docstring is inherited.",
};

/// The rule's declaration.
pub const RULE: ProjectCodeRule = ProjectCodeRule {
    declaration: Declaration {
        name: RuleName("copied-base-docstring"),
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
            summary: "Flags `@override` method docstrings that repeat the inherited docstring or its summary.",
            what_it_does: indoc::indoc! {r"
                Flags the docstring of a method decorated with `@override` (`typing.override` or
                `typing_extensions.override`) when it equals the docstring it inherits, or the first
                paragraph of that docstring, ignoring case, whitespace and a trailing period. The
                inherited docstring is the one `inspect.getdoc` would show without the override's:
                the first ancestor class in Python C3 MRO order (defined earlier in the file or
                imported from another project module) whose method of the same name has a docstring.
                A docstring that extends, shortens or adapts the inherited text is not flagged. An
                override has no docstring or a complete one, which is stricter than the Google style
                guide's docstring with at least the differences."},
            why_is_this_bad: indoc::indoc! {r"
                `help()`, Sphinx, Pyright hovers and `inspect.getdoc` already show the inherited
                docstring on a method that has none. A copy adds nothing, and a copy of the summary
                alone hides the inherited sections. When the base contract changes, the copy keeps
                the old text."},
            known_problems: Some(indoc::indoc! {r"
                Base classes in third-party libraries outside project source roots are not seen.
                Methods without `@override` are not inspected: enable mypy's `explicit-override`
                error code or Pyright's `reportImplicitOverride` to make every override explicit.
                When the inherited docstring is itself a deferral phrase, `deferral-docstring` also
                reports the copy."}),
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
            examples: &[ProjectExample {
                language: Language::Python,
                context: &[(
                    "pkg/base.py",
                    indoc::indoc! {r#"
                        class Store:
                            def read(self, key: str) -> bytes:
                                """Reads the value stored under a key."""
                                raise NotImplementedError
                    "#},
                )],
                path: "pkg/service.py",
                flagged: indoc::indoc! {r#"
                    from typing import override
                    from pkg.base import Store

                    class FileStore(Store):
                        @override
                        def read(self, key: str) -> bytes:
                            """Reads the value stored under a key."""
                            return self._files[key].read_bytes()
                "#},
                flagged_span: r#""""Reads the value stored under a key.""""#,
                fixed: indoc::indoc! {r"
                    from typing import override
                    from pkg.base import Store

                    class FileStore(Store):
                        @override
                        def read(self, key: str) -> bytes:
                            return self._files[key].read_bytes()
                "},
            }],
        },
    },
    target: RuleTarget::All,
    check: check_file,
};

fn check_file(
    rule: &ProjectCodeRule,
    path: &Path,
    file: &ParsedFile,
    project: &ProjectContext<'_>,
    (): (),
) -> Vec<Diagnostic> {
    project
        .documented_override_methods(file, path)
        .into_iter()
        .filter(|entry| {
            entry.docstring.normalized() == entry.inherited_docstring.normalized()
                || entry
                    .docstring
                    .copies_owned_summary_of(&entry.inherited_docstring)
        })
        .map(|entry| {
            rule.diagnostic_at_node(
                path,
                &entry.docstring.node(),
                &[
                    ("function", &entry.method_name),
                    ("class", &entry.ancestor_name),
                ],
            )
        })
        .collect()
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
                            """Reads the value stored under a key."""

                    class FileStore(Store):
                        @override
                        def read(self, key: str) -> bytes:
                            return b''
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
                extension_of_inherited_exempt => r#"
                    from typing import override

                    class RawConfig:
                        def set(self, option: str, value: str) -> None:
                            """Sets an option."""

                    class Config(RawConfig):
                        @override
                        def set(self, option: str, value: str) -> None:
                            """Sets an option. Extends RawConfig.set by validating the value."""
                "#,
                same_summary_different_body_exempt => r#"
                    from typing import override

                    class Store:
                        def read(self, key: str) -> bytes:
                            """Reads the value stored under a key.

                            Each call reads the disk.
                            """

                    class CachedStore(Store):
                        @override
                        def read(self, key: str) -> bytes:
                            """Reads the value stored under a key.

                            Each call reads the in-memory cache first.
                            """
                "#,
                adapted_copy_exempt => r#"
                    from typing import override

                    class Date:
                        def replace(self, year: int, month: int, day: int) -> Date:
                            """Returns a new date with the given fields replaced by new values."""

                    class DateTime(Date):
                        @override
                        def replace(self, year: int, month: int, day: int) -> DateTime:
                            """Returns a new datetime with the given fields replaced by new values."""
                "#,
                word_prefix_of_inherited_exempt => r#"
                    from typing import override

                    class Address:
                        def is_global(self) -> bool:
                            """True if the address is global in the IPv4 registry or the IPv6 registry."""

                    class IPv4Address(Address):
                        @override
                        def is_global(self) -> bool:
                            """True if the address is global in the IPv4 registry."""
                "#,
                undocumented_ancestor_exempt => r#"
                    from typing import override

                    class LocalStore(storage.Store):
                        def read(self, key: str) -> bytes:
                            return b''

                    class FileStore(LocalStore):
                        @override
                        def read(self, key: str) -> bytes:
                            """Reads the value stored under a key."""
                "#,
                base_defined_after_subclass_exempt => r#"
                    from typing import override

                    class FileStore(Store):
                        @override
                        def read(self, key: str) -> bytes:
                            """Reads the value stored under a key."""

                    class Store:
                        def read(self, key: str) -> bytes:
                            """Reads the value stored under a key."""
                "#,
                base_bound_to_last_earlier_definition => r#"
                    from typing import override

                    class Store:
                        def read(self, key: str) -> bytes:
                            """Reads the value stored under a key."""

                    class Store:
                        def read(self, key: str) -> bytes:
                            """Reads the bytes of a file."""

                    class FileStore(Store):
                        @override
                        def read(self, key: str) -> bytes:
                            """Reads the value stored under a key."""
                "#,
                module_level_function_exempt => r#"
                    from typing import override

                    class Store:
                        def read(self, key: str) -> bytes:
                            """Reads the value stored under a key."""

                    class FileStore(Store):
                        pass

                    @override
                    def read(key: str) -> bytes:
                        """Reads the value stored under a key."""
                "#,
                undecorated_override_exempt => r#"
                    class Store:
                        def read(self, key: str) -> bytes:
                            """Reads the value stored under a key."""

                    class FileStore(Store):
                        def read(self, key: str) -> bytes:
                            """Reads the value stored under a key."""
                "#,
                imported_base_difference_exempt => {
                    "pkg/base.py" => r#"
                        class Store:
                            def read(self, key: str) -> bytes:
                                """Reads the value stored under a key."""
                    "#,
                    "pkg/service.py" => r#"
                        from typing import override
                        from .base import Store

                        class CachedStore(Store):
                            @override
                            def read(self, key: str) -> bytes:
                                """Reads from the in-memory cache before disk."""
                    "#,
                },
            ],
            fail: [
                exact_copy => r#"
                    from typing import override

                    class Store:
                        def read(self, key: str) -> bytes:
                            """Reads the value stored under a key."""

                    class FileStore(Store):
                        @override
                        def read(self, key: str) -> bytes:
                            """Reads the value stored under a key."""
                            return b''
                "# => r#""""Reads the value stored under a key.""""#,
                copied_imported_base_docstring => {
                    "pkg/base.py" => r#"
                        class Store:
                            def read(self, key: str) -> bytes:
                                """Reads the value stored under a key."""
                    "#,
                    "pkg/service.py" => r#"
                        from typing import override
                        from .base import Store

                        class FileStore(Store):
                            @override
                            def read(self, key: str) -> bytes:
                                """Reads the value stored under a key."""
                                return b''
                    "#,
                } => ("pkg/service.py", r#""""Reads the value stored under a key.""""#),
                self_shadowed_imported_base => {
                    "pkg/base.py" => r#"
                        class Store:
                            def read(self, key: str) -> bytes:
                                """Reads the value stored under a key."""
                    "#,
                    "pkg/service.py" => r#"
                        from typing import override
                        from .base import Store

                        class Store(Store):
                            @override
                            def read(self, key: str) -> bytes:
                                """Reads the value stored under a key."""
                                return b''
                    "#,
                } => ("pkg/service.py", r#""""Reads the value stored under a key.""""#),
                copied_pyi_stub_base_docstring => {
                    "pkg/base.pyi" => r#"
                        class Store:
                            def read(self, key: str) -> bytes:
                                """Reads the value stored under a key."""
                                ...
                    "#,
                    "pkg/base.py" => r#"
                        class Store:
                            def read(self, key: str) -> bytes:
                                return b''
                    "#,
                    "pkg/service.py" => r#"
                        from typing import override
                        from .base import Store

                        class FileStore(Store):
                            @override
                            def read(self, key: str) -> bytes:
                                """Reads the value stored under a key."""
                                return b''
                    "#,
                } => ("pkg/service.py", r#""""Reads the value stored under a key.""""#),
                diamond_mro_inherits_right_branch_before_root => {
                    "pkg/root.py" => r#"
                        class RootStore:
                            def read(self, key: str) -> bytes:
                                """Reads from the root store."""
                    "#,
                    "pkg/branches.py" => r#"
                        from .root import RootStore

                        class LeftStore(RootStore):
                            pass

                        class RightStore(RootStore):
                            def read(self, key: str) -> bytes:
                                """Reads from the right store."""
                    "#,
                    "pkg/service.py" => r#"
                        from typing import override
                        from .branches import LeftStore, RightStore

                        class CombinedStore(LeftStore, RightStore):
                            @override
                            def read(self, key: str) -> bytes:
                                """Reads from the right store."""
                                return b''
                    "#,
                } => ("pkg/service.py", r#""""Reads from the right store.""""#),
                copy_with_other_wrapping_and_case => r#"
                    from typing import override

                    class Store:
                        def read(self, key: str) -> bytes:
                            """Reads the value
                            stored under a key."""

                    class FileStore(Store):
                        @override
                        def read(self, key: str) -> bytes:
                            """reads the value stored under a key"""
                "# => r#""""reads the value stored under a key""""#,
                copy_of_summary_paragraph => r#"
                    from typing import override

                    class Store:
                        def read(self, key: str) -> bytes:
                            """Reads the value stored under a key.

                            Args:
                                key: The key to read.
                            """

                    class FileStore(Store):
                        @override
                        def read(self, key: str) -> bytes:
                            """Reads the value stored under a key."""
                "# => r#""""Reads the value stored under a key.""""#,
                undocumented_nearer_ancestor_skipped => r#"
                    from typing import override

                    class Store:
                        def read(self, key: str) -> bytes:
                            """Reads the value stored under a key."""

                    class LocalStore(Store):
                        def read(self, key: str) -> bytes:
                            return b''

                    class FileStore(LocalStore):
                        @override
                        def read(self, key: str) -> bytes:
                            """Reads the value stored under a key."""
                "# => r#""""Reads the value stored under a key.""""#,
                second_base_searched => r#"
                    from typing import override

                    class Store:
                        def read(self, key: str) -> bytes:
                            """Reads the value stored under a key."""

                    class LoggingMixin:
                        def log(self) -> None:
                            """Logs the access."""

                    class FileStore(LoggingMixin, Store):
                        @override
                        def read(self, key: str) -> bytes:
                            """Reads the value stored under a key."""
                "# => r#""""Reads the value stored under a key.""""#,
                last_documented_definition_in_base => r#"
                    from typing import override

                    class Store:
                        @property
                        def size(self) -> int:
                            """The number of stored values."""

                        @size.setter
                        def size(self, value: int) -> None:
                            pass

                    class FileStore(Store):
                        @property
                        @override
                        def size(self) -> int:
                            """The number of stored values."""
                "# => r#""""The number of stored values.""""#,
            ],
        },
    }
);
