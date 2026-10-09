"""Entry types for the exemption mutation harness (`scripts/exemption_mutations.py`)."""

from __future__ import annotations

from typing import NamedTuple

AST = "src/code_lint/ast.rs"
POLICY = "src/code_lint/policy.rs"
RUST = "src/code_lint/ast/rust.rs"
PYTHON = "src/code_lint/ast/python.rs"
CLASSES = "src/code_lint/ast/python/classes.rs"
FORMAT_STRINGS = "src/code_lint/ast/python/format_strings.rs"
PY_LITERALS = "src/code_lint/ast/python/literals.rs"
LOGGING = "src/code_lint/ast/python/logging.rs"
STRINGS = "src/code_lint/ast/python/strings.rs"
QUOTE_WRAPPED = "src/code_lint/rules/quote_wrapped_placeholder.rs"
REPEATED_LITERAL = "src/code_lint/rules/repeated_literal.rs"
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
