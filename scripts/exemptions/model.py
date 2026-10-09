"""Entry types for the exemption mutation harness (`scripts/exemption_mutations.py`)."""

from __future__ import annotations

from typing import NamedTuple

AST = "src/code_lint/ast.rs"
CONTRACT = "src/code_lint/contract.rs"
POLICY = "src/code_lint/policy.rs"
RUST = "src/code_lint/ast/rust.rs"
PYTHON = "src/code_lint/ast/python.rs"
ANNOTATIONS = "src/code_lint/ast/python/annotations.rs"
CLASSES = "src/code_lint/ast/python/classes.rs"
FORMAT_STRINGS = "src/code_lint/ast/python/format_strings.rs"
FUNCTIONS = "src/code_lint/ast/python/functions.rs"
PY_LITERALS = "src/code_lint/ast/python/literals.rs"
LOGGING = "src/code_lint/ast/python/logging.rs"
PARAMETER_USAGE = "src/code_lint/ast/python/parameter_usage.rs"
SCOPES = "src/code_lint/ast/python/scopes.rs"
STRINGS = "src/code_lint/ast/python/strings.rs"
BINDINGS = "src/code_lint/semantic/bindings.rs"
COMMENTS = "src/code_lint/semantic/comments.rs"
ABBREVIATED_NAME = "src/code_lint/rules/abbreviated_name.rs"
CONCRETE_COLLECTION_ATTRIBUTE = "src/code_lint/rules/concrete_collection_attribute.rs"
CONCRETE_COLLECTION_PARAMETER = "src/code_lint/rules/concrete_collection_parameter.rs"
INLINE_PUBLIC_ATTRIBUTE_ANNOTATION = "src/code_lint/rules/inline_public_attribute_annotation.rs"
MUTABLE_COLLECTION_ATTRIBUTE = "src/code_lint/rules/mutable_collection_attribute.rs"
MUTABLE_COLLECTION_PARAMETER = "src/code_lint/rules/mutable_collection_parameter.rs"
MUTABLE_COLLECTION_RETURN = "src/code_lint/rules/mutable_collection_return.rs"
MUTABLE_MODULE_CONSTANT = "src/code_lint/rules/mutable_module_constant.rs"
NULLABLE_COLLECTION_RETURN = "src/code_lint/rules/nullable_collection_return.rs"
QUOTE_WRAPPED = "src/code_lint/rules/quote_wrapped_placeholder.rs"
REPEATED_LITERAL = "src/code_lint/rules/repeated_literal.rs"
SINGLE_LETTER_NAME = "src/code_lint/rules/single_letter_name.rs"
SPECIFIC_COLLECTION_PARAMETER = "src/code_lint/rules/specific_collection_parameter.rs"
TYPE_SUFFIXED_NAME = "src/code_lint/rules/type_suffixed_name.rs"
UNMATCHED_LOGGER = "src/code_lint/rules/unmatched_logger_placeholder.rs"

INLINE_TEST_ALWAYS_FALSE = (
    (
        "pub fn is_in_rust_inline_test(&self, offset: usize) -> bool {",
        "pub fn is_in_rust_inline_test(&self, offset: usize) -> bool {\n"
        "        let _ = offset;\n        return false;",
    ),
)


class Mutation(NamedTuple):
    """Disables one exemption by applying `edits` (each `old` must occur exactly once).

    `cases` are `<rule>::<pass|fail>::<case>`, optionally suffixed with `@python` or `@rust` when
    the same case name exists in both languages and the mutation only affects one of them.
    """

    label: str
    path: str
    edits: tuple[tuple[str, str], ...]
    cases: tuple[str, ...]


class Unmutated(NamedTuple):
    """Cases with no exemption to disable (positive layouts) or whose exemption no small source
    edit can disable (a construct the extractor never visits), with the reason."""

    label: str
    reason: str
    cases: tuple[str, ...]


class Cluster(NamedTuple):
    """The rules of one audit batch; every `pass` case of `rules` must be listed in an entry."""

    rules: tuple[str, ...]
    mutations: tuple[Mutation, ...]
    unmutated: tuple[Unmutated, ...]
