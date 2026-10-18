"""Entry types for the exemption mutation harness (`scripts/exemption_mutations.py`)."""

from __future__ import annotations

from typing import NamedTuple

AST = "src/code_lint/ast.rs"
CALL_CLUSTERS = "src/code_lint/ast/call_clusters.rs"
POLICY = "src/code_lint/policy.rs"
RUST = "src/code_lint/ast/rust.rs"
RS_CALLS = "src/code_lint/ast/rust/calls.rs"
PYTHON = "src/code_lint/ast/python.rs"
ANNOTATIONS = "src/code_lint/ast/python/annotations.rs"
PY_CALLS = "src/code_lint/ast/python/calls.rs"
CLASSES = "src/code_lint/ast/python/classes.rs"
DOCSTRINGS = "src/code_lint/ast/python/docstrings.rs"
FORMAT_STRINGS = "src/code_lint/ast/python/format_strings.rs"
FUNCTIONS = "src/code_lint/ast/python/functions.rs"
PY_LITERALS = "src/code_lint/ast/python/literals.rs"
LOGGING = "src/code_lint/ast/python/logging.rs"
PARAMETER_USAGE = "src/code_lint/ast/python/parameter_usage.rs"
RETURNS = "src/code_lint/ast/python/returns.rs"
POSITIONAL_READS = "src/code_lint/ast/python/positional_reads.rs"
SCOPES = "src/code_lint/ast/python/scopes.rs"
STRINGS = "src/code_lint/ast/python/strings.rs"
BINDINGS = "src/code_lint/semantic/bindings.rs"
CALLS = "src/code_lint/semantic/calls.rs"
ABBREVIATED_NAME = "src/code_lint/rules/abbreviated_name.rs"
BARE_MULTILINE_STRING = "src/code_lint/rules/bare_multiline_string.rs"
CONCRETE_COLLECTION_ATTRIBUTE = "src/code_lint/rules/concrete_collection_attribute.rs"
CONCRETE_COLLECTION_PARAMETER = "src/code_lint/rules/concrete_collection_parameter.rs"
DUPLICATED_TEST_SETUP = "src/code_lint/rules/duplicated_test_setup.rs"
ENVIRONMENT_VARIABLE_IN_FUNCTION = (
    "src/code_lint/rules/environment_variable_in_function.rs"
)
ERROR_LOG_IN_EXCEPT = "src/code_lint/rules/error_log_in_except.rs"
FAKE_WITHOUT_PROTOCOL = "src/code_lint/rules/fake_without_protocol.rs"
IDENTICAL_POSITIONAL_TYPES = "src/code_lint/rules/identical_positional_types.rs"
INLINE_PUBLIC_ATTRIBUTE_ANNOTATION = (
    "src/code_lint/rules/inline_public_attribute_annotation.rs"
)
MUTABLE_COLLECTION_ATTRIBUTE = "src/code_lint/rules/mutable_collection_attribute.rs"
MUTABLE_COLLECTION_PARAMETER = "src/code_lint/rules/mutable_collection_parameter.rs"
MUTABLE_COLLECTION_RETURN = "src/code_lint/rules/mutable_collection_return.rs"
MUTABLE_MODULE_CONSTANT = "src/code_lint/rules/mutable_module_constant.rs"
NESTED_FUNCTION = "src/code_lint/rules/nested_function.rs"
NULLABLE_COLLECTION_RETURN = "src/code_lint/rules/nullable_collection_return.rs"
QUOTE_WRAPPED = "src/code_lint/rules/quote_wrapped_placeholder.rs"
REPEATED_INDEX_ACCESS = "src/code_lint/rules/repeated_index_access.rs"
REPEATED_LITERAL = "src/code_lint/rules/repeated_literal.rs"
SINGLE_LETTER_NAME = "src/code_lint/rules/single_letter_name.rs"
SLEEP_IN_TESTS = "src/code_lint/rules/sleep_in_tests.rs"
SPECIFIC_COLLECTION_PARAMETER = "src/code_lint/rules/specific_collection_parameter.rs"
SUPPRESSED_EXCEPTION = "src/code_lint/rules/suppressed_exception.rs"
TEST_CALLS_TEST = "src/code_lint/rules/test_calls_test.rs"
TOO_MANY_ASSERTIONS = "src/code_lint/rules/too_many_assertions.rs"
TYPE_SUFFIXED_NAME = "src/code_lint/rules/type_suffixed_name.rs"
UNEXPLAINED_OPTIONAL_RETURN = "src/code_lint/rules/unexplained_optional_return.rs"
UNMATCHED_LOGGER = "src/code_lint/rules/unmatched_logger_placeholder.rs"
VERB_LED_PROPERTY_DOCSTRING = "src/code_lint/rules/verb_led_property_docstring.rs"
YIELD_LOOP_IN_TESTS = "src/code_lint/rules/yield_loop_in_tests.rs"

INLINE_TEST_ALWAYS_FALSE = (
    (
        "pub fn is_in_rust_inline_test(&self, offset: usize) -> bool {",
        "pub fn is_in_rust_inline_test(&self, offset: usize) -> bool {\n"
        "        let _ = offset;\n        return false;",
    ),
)


class Mutation(NamedTuple):
    """Disables one exemption by editing the rule source; each listed case must then fail.

    Attributes:
        label: Name of the exemption, printed with each result.
        path: Source file to edit, relative to the repository root.
        edits: `(old, new)` replacements; each `old` must occur exactly once in `path`.
        cases: `<rule>::<pass|fail>::<case>`, optionally suffixed with `@python` or `@rust` when
            the same case name exists in both languages and the mutation only affects one.
    """

    label: str
    path: str
    edits: tuple[tuple[str, str], ...]
    cases: tuple[str, ...]


class Unmutated(NamedTuple):
    """Cases with no exemption to disable (positive layouts) or whose exemption no small source
    edit can disable (a construct the extractor never visits).

    Attributes:
        label: Name of the group of cases.
        reason: Why no mutation covers these cases.
        cases: `<rule>::pass::<case>` names, in the same format as `Mutation.cases`.
    """

    label: str
    reason: str
    cases: tuple[str, ...]


class Cluster(NamedTuple):
    """The rules of one audit batch; every `pass` case of `rules` must be listed in an entry."""

    rules: tuple[str, ...]
    mutations: tuple[Mutation, ...]
    unmutated: tuple[Unmutated, ...]
