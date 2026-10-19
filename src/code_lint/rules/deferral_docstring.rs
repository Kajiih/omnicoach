//! Flags `@override` method docstrings that only point to the inherited documentation.
//!
//! Design: `docs/dev/trivial_override_docstring/03_design.md`. Only the whole docstring is
//! matched against the phrase list; a phrase followed by more text is not a pure pointer.

use crate::code_lint::ast::python::{PythonBaseClass, PythonClassInfo, normalize_docstring_text};
use crate::code_lint::ast::{self, ParsedFile};
use crate::code_lint::contract::{CodeRule, RuleTarget};
use crate::diagnostic::{Diagnostic, Language, RuleName, ViolationTemplate, violation_template};
use crate::rule_declaration::{
    Classification, Consensus, Declaration, Example, FilterListDefaults, ImpactedQuality, ListKind,
    ListOption, Precision, Reference, RuleDoc, RuleOptions, Topic,
};
use std::collections::HashSet;
use std::path::Path;

const BANNED: ListOption = ListOption {
    kind: ListKind::Deny,
    doc: "Phrases that make up a whole deferral docstring; `{base}` stands for a base class and `{method}` for the method's name.",
    default: FilterListDefaults {
        base: &[
            "see base class",
            "see the base class",
            "see parent class",
            "see superclass",
            "see base class docstring",
            "see base class for details",
            "same as base class",
            "same as parent class",
            "inherited",
            "docstring inherited",
            "inherited from base class",
            "overrides base class method",
            "overrides parent class implementation",
            "overrides base definition",
            "see {base}",
            "see {base}.{method}",
            "same as {base}.{method}",
            "overrides {base}.{method}",
            "implements {base}.{method}",
            "inherited from {base}",
            "implements the {method} interface",
        ],
        extend: &[],
        remove: &[],
    },
};

const TEMPLATE: ViolationTemplate = violation_template! {
    summary: "Docstring of `{function}` only refers to the documentation of another definition.",
    rationale: "A method's own docstring replaces the inherited one in `help()`, generated documentation and IDE hovers, so the pointer hides the very docstring it refers to.",
    suggestion: "Remove the docstring so that the base docstring is inherited.",
};

/// The rule's declaration.
pub const RULE: CodeRule<ListOption> = CodeRule {
    declaration: Declaration {
        name: RuleName("deferral-docstring"),
        template: &TEMPLATE,
        languages: &[Language::Python],
        options: RuleOptions::code_rule(BANNED),
        classification: Classification {
            topics: &[Topic::DOCSTRINGS],
            precision: Precision::Exact,
            consensus: Consensus::Opinionated,
            impacted_quality: ImpactedQuality::Maintainability,
        },
        doc: RuleDoc {
            summary: "Flags `@override` method docstrings that only refer to the inherited documentation.",
            what_it_does: indoc::indoc! {r"
                Flags the docstring of a method decorated with `@override` (`typing.override` or
                `typing_extensions.override`) when the whole docstring is one of the `banned`
                phrases, such as `See base class.` or `Same as Base.run().`. Case, whitespace,
                backticks, a trailing period, `()` and the Sphinx roles `:meth:`, `:class:` and
                `:func:` are ignored. In a phrase, `{base}` matches a base of the enclosing class,
                as written or by its last dotted segment, or an ancestor class defined earlier in
                the file; `{method}` matches the method's name. A docstring that adds text after the
                phrase is not flagged. An override has no docstring or a complete one, which is
                stricter than the Google style guide's docstring with at least the differences."},
            why_is_this_bad: indoc::indoc! {r"
                `help()`, Sphinx, Pyright hovers and `inspect.getdoc` show a method's own docstring
                and fall back to the inherited one only when the method has none. A pointer such as
                `See base class.` therefore replaces the documentation it points to: readers see
                the pointer instead of the contract. Without the docstring, the tools show the base
                docstring directly."},
            known_problems: Some(indoc::indoc! {r"
                Wordings outside the list, and phrases in other languages, are not flagged; extend
                the list. A reference to a class that is not a base (`See Pytester.spawn.`) is not
                flagged, since it often delegates to the only documentation. Methods without
                `@override` are not inspected: enable mypy's `explicit-override` error code or
                Pyright's `reportImplicitOverride` to make every override explicit."}),
            references: &[
                Reference {
                    title: "Google Python Style Guide §3.8.3.1: Overridden Methods",
                    url: "https://google.github.io/styleguide/pyguide.html#3831-overridden-methods",
                },
                Reference {
                    title: "PEP 698: Override Decorator for Static Typing",
                    url: "https://peps.python.org/pep-0698/",
                },
            ],
            examples: &[Example {
                language: Language::Python,
                flagged: indoc::indoc! {r#"
                    from typing import override

                    class FileStore(Store):
                        @override
                        def read(self, key: str) -> bytes:
                            """See base class."""
                            return self._files[key].read_bytes()
                "#},
                flagged_span: r#""""See base class.""""#,
                fixed: indoc::indoc! {r"
                    from typing import override

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

/// Phrase token for a base class reference.
const BASE_TOKEN: &str = "{base}";
/// Phrase token for the method's name.
const METHOD_TOKEN: &str = "{method}";
/// Sphinx cross-reference roles removed before matching; `:py:` forms first, since they contain
/// the short forms.
const SPHINX_ROLES: &[&str] = &[
    ":py:meth:",
    ":py:class:",
    ":py:func:",
    ":meth:",
    ":class:",
    ":func:",
];

const FUNCTION: &str = "function";

fn check_file(
    rule: &CodeRule<ListOption>,
    path: &Path,
    file: &ParsedFile,
    banned: &HashSet<String>,
) -> Vec<Diagnostic> {
    let phrases: Vec<String> = banned
        .iter()
        .map(|phrase| normalize_docstring_text(phrase))
        .collect();
    let classes = ast::python::extract_classes(file);
    let mut diagnostics = Vec::new();
    for (index, class) in classes.iter().enumerate() {
        let base_references = base_references(&classes, index);
        for method in class.methods() {
            if !method.has_override_decorator() {
                continue;
            }
            let Some(docstring) = method.docstring() else {
                continue;
            };
            let text = comparable_text(&docstring.normalized());
            let method_name = method.name.to_lowercase();
            let is_deferral = phrases.iter().any(|phrase| {
                matches_phrase(
                    &text,
                    &phrase.replace(METHOD_TOKEN, &method_name),
                    &base_references,
                )
            });
            if is_deferral {
                diagnostics.push(rule.diagnostic_at_node(
                    path,
                    &docstring.node(),
                    &[(FUNCTION, &method.name)],
                ));
            }
        }
    }
    diagnostics
}

/// The lowercase references that `{base}` matches in methods of `classes[index]`: each base as
/// written (without type arguments) and each in-file ancestor, with dotted names also matched by
/// their last segment.
fn base_references(classes: &[PythonClassInfo<'_>], index: usize) -> Vec<String> {
    let ancestors = ast::python::local_ancestors(classes, index);
    let names = classes[index]
        .declaration()
        .bases
        .iter()
        .map(PythonBaseClass::unsubscripted_name)
        .chain(
            ancestors
                .iter()
                .map(|&ancestor| classes[ancestor].declaration().name.as_str()),
        );
    let mut references = Vec::new();
    for name in names {
        let reference = name.to_lowercase();
        if let Some((_, last_segment)) = reference.rsplit_once('.') {
            references.push(last_segment.to_owned());
        }
        references.push(reference);
    }
    references
}

/// The normalized docstring text without Sphinx roles, backticks and `()`, so that
/// ``:meth:`Base.run()` `` reads as `base.run`.
fn comparable_text(normalized: &str) -> String {
    let mut text = normalized.to_owned();
    for role in SPHINX_ROLES {
        text = text.replace(role, "");
    }
    text.replace('`', "").replace("()", "")
}

/// Returns true if `text` is exactly `phrase`, with a `{base}` token standing for one of
/// `base_references`.
fn matches_phrase(text: &str, phrase: &str, base_references: &[String]) -> bool {
    match phrase.split_once(BASE_TOKEN) {
        Some((prefix, suffix)) => text
            .strip_prefix(prefix)
            .and_then(|rest| rest.strip_suffix(suffix))
            .is_some_and(|reference| base_references.iter().any(|base| base == reference)),
        None => text == phrase,
    }
}

#[cfg(test)]
crate::test_utils::rule_test!(
    RULE,
    {
        Python => {
            pass: [
                no_docstring => r"
                    from typing import override

                    class FileStore(Store):
                        @override
                        def read(self, key: str) -> bytes:
                            return b''
                ",
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
                phrase_followed_by_text_exempt => r#"
                    from typing import override

                    class CachedStore(Store):
                        @override
                        def read(self, key: str) -> bytes:
                            """See base class. Also invalidates the cache."""
                "#,
                reference_to_non_base_exempt => r#"
                    from typing import override

                    class Testdir(Base):
                        @override
                        def spawn(self, command: str) -> None:
                            """See :meth:`Pytester.spawn`."""
                "#,
                module_level_function_exempt => r#"
                    from typing import override

                    class Store:
                        pass

                    @override
                    def read(key: str) -> bytes:
                        """See base class."""
                "#,
                undecorated_override_exempt => r#"
                    class Store:
                        def read(self, key: str) -> bytes:
                            """Reads the value stored under a key."""

                    class FileStore(Store):
                        def read(self, key: str) -> bytes:
                            """See base class."""
                "#,
            ],
            fail: [
                see_base_class => r#"
                    from typing import override

                    class FileStore(Store):
                        @override
                        def read(self, key: str) -> bytes:
                            """See base class."""
                "# => r#""""See base class.""""#,
                case_and_whitespace_ignored => r#"
                    from typing import override

                    class FileStore(Store):
                        @override
                        def read(self, key: str) -> bytes:
                            """  see  BASE   class  """
                "# => r#""""  see  BASE   class  """"#,
                typing_extensions_override => r#"
                    import typing_extensions

                    class FileStore(Store):
                        @typing_extensions.override
                        def read(self, key: str) -> bytes:
                            """Inherited."""
                "# => r#""""Inherited.""""#,
                named_base_method_with_role => r#"
                    from typing import override

                    class FileIO(RawIOBase):
                        @override
                        def readinto(self, buffer: bytearray) -> int:
                            """Same as :meth:`RawIOBase.readinto()`."""
                "# => r#""""Same as :meth:`RawIOBase.readinto()`.""""#,
                dotted_base_by_last_segment => r#"
                    from typing import override

                    class FileStore(storage.Store):
                        @override
                        def read(self, key: str) -> bytes:
                            """See `Store`."""
                "# => r#""""See `Store`.""""#,
                in_file_ancestor => r#"
                    from typing import override

                    class Store:
                        pass

                    class LocalStore(Store):
                        pass

                    class FileStore(LocalStore):
                        @override
                        def read(self, key: str) -> bytes:
                            """See Store.read()."""
                "# => r#""""See Store.read().""""#,
                method_interface_phrase => r#"
                    from typing import override

                    class Leaf(Node):
                        @override
                        def _edit_leaf(self) -> None:
                            """Implements the _edit_leaf interface."""
                "# => r#""""Implements the _edit_leaf interface.""""#,
            ],
        },
    }
);
