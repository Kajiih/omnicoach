//! Flags documented Python functions returning or yielding a positional tuple without a
//! `Returns:` / `Yields:` section or a `Returns …` / `Yields …` summary.
//!
//! Design: `docs/dev/complex_function_docstring/03_design.md` and `03c_section_requirements.md`
//! (proposal P5).

use crate::code_lint::ast::python::PythonDocstring;
use crate::code_lint::ast::{self, ParsedFile};
use crate::code_lint::contract::{CodeRule, RuleTarget};
use crate::diagnostic::{Diagnostic, Language, RuleName, ViolationTemplate, violation_template};
use crate::rule_declaration::{
    Classification, Consensus, Declaration, Example, ImpactedQuality, Precision, Reference,
    RuleDoc, RuleOptions, Topic,
};
use std::path::Path;

const TEMPLATE: ViolationTemplate = violation_template! {
    summary: "Docstring of `{function}` does not describe the elements of its tuple result.",
    rationale: "A tuple return type states only the element types by position, not what each position represents.",
    suggestion: "Add a `Returns:` or `Yields:` section to the docstring of `{function}` that describes each tuple element.",
};

/// The rule's declaration.
pub const RULE: CodeRule = CodeRule {
    declaration: Declaration {
        name: RuleName("undocumented-tuple-return"),
        template: &TEMPLATE,
        languages: &[Language::Python],
        options: RuleOptions::code_rule(()),
        classification: Classification {
            topics: &[Topic::DOCSTRINGS],
            precision: Precision::Heuristic,
            consensus: Consensus::Opinionated,
            impacted_quality: ImpactedQuality::Maintainability,
        },
        doc: RuleDoc {
            summary: "Requires a `Returns:` or `Yields:` description when a documented function returns or yields a positional tuple.",
            what_it_does: indoc::indoc! {r"
                Flags a documented function or method that returns or yields a positional tuple of
                at least two elements when its docstring has neither a `Returns:` / `Yields:`
                section nor a summary sentence opening with `Return`, `Returns`, `Yield` or
                `Yields`.

                A function returns or yields a positional tuple when:

                - its return annotation (or its yielded type in `Iterator[...]`, `Iterable[...]`,
                  `Generator[...]` or their `Async` forms, and after unwrapping `Awaitable`,
                  `Coroutine`, `Annotated` and unions) has a `tuple[A, B, ...]` / `Tuple[A, B, ...]`
                  branch with at least two type arguments and no `...`; or
                - it has no return annotation and its own body (excluding nested functions,
                  classes and lambdas) contains a `return` or `yield` of a tuple display with at
                  least two elements.

                Not checked: functions without a docstring (left to
                `undocumented-nontrivial-function`), homogeneous variable-length `tuple[T, ...]`
                annotations, nested functions, `@override` methods, property setters and deleters,
                data-model dunder methods other than `__init__`, `__new__` and `__call__`, and
                test files."},
            why_is_this_bad: indoc::indoc! {r#"
                Unlike a single return value or a `NamedTuple` / `@dataclass`, a positional tuple
                `tuple[str, int]` or `(a, b)` gives its elements no field names. Even when the
                function's action is clear from its summary (`"Splits an address."` or
                `"Executes the command."`), callers cannot tell from the signature alone what
                each position means, its units, or the order of two same-typed elements."#},
            known_problems: Some(indoc::indoc! {r"
                - A `Returns:` section or `Returns …` summary that mentions the tuple as a whole
                  without naming each position still satisfies the rule.
                - A type alias for a tuple (`Pair = tuple[int, int]`) or an unannotated function
                  that returns a variable holding a tuple rather than a tuple display is not
                  recognized."}),
            references: &[
                Reference {
                    title: "Google Python Style Guide §3.8.3: Functions and Methods",
                    url: "https://google.github.io/styleguide/pyguide.html#383-functions-and-methods",
                },
                Reference {
                    title: "Ruff rule `undocumented-return-type` (DOC201)",
                    url: "https://docs.astral.sh/ruff/rules/undocumented-return-type/",
                },
            ],
            examples: &[Example {
                language: Language::Python,
                flagged: indoc::indoc! {r#"
                    def execute_command(command: list[str]) -> tuple[list[str], bool]:
                        """Executes the command in a subprocess."""
                        output, failed = run_process(command)
                        return output, failed
                "#},
                flagged_span: "execute_command",
                fixed: indoc::indoc! {r#"
                    def execute_command(command: list[str]) -> tuple[list[str], bool]:
                        """Executes the command in a subprocess.

                        Returns:
                            A `(output_lines, failed)` pair where `failed` is True on a non-zero exit.
                        """
                        output, failed = run_process(command)
                        return output, failed
                "#},
            }],
        },
    },
    target: RuleTarget::SourceOnly,
    check: check_file,
};

const FUNCTION: &str = "function";

fn check_file(rule: &CodeRule, path: &Path, file: &ParsedFile, (): ()) -> Vec<Diagnostic> {
    ast::python::extract_function_signatures(file)
        .into_iter()
        .filter(|signature| {
            !signature.is_nested()
                && !signature.is_exempt_from_docstring_rules()
                && signature
                    .docstring()
                    .as_ref()
                    .is_some_and(|docstring| !PythonDocstring::describes_result(docstring))
                && signature.returns_positional_tuple()
        })
        .map(|signature| {
            rule.diagnostic_at_node(path, &signature.name_node, &[(FUNCTION, &signature.name)])
        })
        .collect()
}

#[cfg(test)]
crate::test_utils::rule_test!(
    RULE,
    {
        Python => {
            pass: [
                returns_or_yields_section_exempt => r#"
                    def split_host_port(address: str) -> tuple[str, int]:
                        """Splits an address.

                        Returns:
                            A `(host, port)` pair.
                        """
                        host, port = address.split(":")
                        return host, int(port)
                "#,
                result_verb_summary_exempt => r#"
                    def split_host_port(address: str) -> tuple[str, int]:
                        """Returns the `(host, port)` pair parsed from `address`."""
                        host, port = address.split(":")
                        return host, int(port)
                "#,
                variadic_or_single_element_tuple_exempt => r#"
                    def freeze_items(items: list[int]) -> tuple[int, ...]:
                        """Freezes items into a tuple."""
                        return tuple(items)

                    def wrap_single(item: int) -> tuple[int]:
                        """Wraps a single item."""
                        return (item,)
                "#,
                non_tuple_return_exempt => r#"
                    def list_hosts(addresses: list[str]) -> list[str]:
                        """Extracts hosts from addresses."""
                        return [addr.split(":")[0] for addr in addresses]
                "#,
                undocumented_function_exempt => r#"
                    def split_host_port(address: str) -> tuple[str, int]:
                        host, port = address.split(":")
                        return host, int(port)
                "#,
                nested_function_exempt => r#"
                    def outer(address: str) -> str:
                        """Extracts the host."""
                        def inner(raw: str) -> tuple[str, int]:
                            """Splits a raw address."""
                            host, port = raw.split(":")
                            return host, int(port)
                        return inner(address)[0]
                "#,
                override_method_exempt => r#"
                    from typing import override

                    class Parser(BaseParser):
                        @override
                        def parse(self, raw: str) -> tuple[str, int]:
                            """Parses raw input."""
                            host, port = raw.split(":")
                            return host, int(port)
                "#,
                nested_scope_tuple_return_exempt => r#"
                    def make_splitter():
                        """Builds a splitter function."""
                        def splitter(raw):
                            return raw.split(":")[0], 80
                        key = lambda pair: (pair[0], pair[1])
                        return splitter
                "#,
            ],
            fail: [
                annotated_tuple_return_flagged => r#"
                    def split_host_port(address: str) -> tuple[str, int]:
                        """Splits an address."""
                        host, port = address.split(":")
                        return host, int(port)
                "# => "split_host_port",
                union_with_tuple_return_flagged => r#"
                    def parse_header(line: str) -> tuple[str, str] | None:
                        """Parses a header line."""
                        if ":" not in line:
                            return None
                        key, value = line.split(":", 1)
                        return key.strip(), value.strip()
                "# => "parse_header",
                generator_yielding_tuple_flagged => r#"
                    from collections.abc import Iterator

                    def iter_pairs(items: list[str]) -> Iterator[tuple[int, str]]:
                        """Iterates over indexed items."""
                        for index, item in enumerate(items):
                            yield index, item
                "# => "iter_pairs",
                unannotated_tuple_return_flagged => r#"
                    def bounds(values):
                        """Computes the minimum and maximum."""
                        return min(values), max(values)
                "# => "bounds",
                unannotated_tuple_yield_flagged => r#"
                    def iter_indexed(items):
                        """Iterates over indexed items."""
                        for index, item in enumerate(items):
                            yield index, item
                "# => "iter_indexed",
            ]
        }
    }
);
