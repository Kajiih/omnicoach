//! Flags nontrivial Python functions whose docstring has no `Args:` section explaining their
//! non-obvious parameters.
//!
//! Design: `docs/dev/complex_function_docstring/03_design.md` and `03c_section_requirements.md`
//! (proposal P2, without the prose-mention exemption).

use crate::code_lint::ast::python::{DocstringSectionKind, PythonClassInfo};
use crate::code_lint::ast::{self, ParsedFile};
use crate::code_lint::contract::{CodeRule, RuleTarget};
use crate::diagnostic::{Diagnostic, Language, RuleName, ViolationTemplate, violation_template};
use crate::rule_declaration::{
    Classification, Consensus, CountOption, Declaration, Example, ImpactedQuality,
    LanguageDefaults, Precision, Reference, RuleDoc, RuleOptions, Topic,
};
use std::path::Path;

const MIN_STATEMENTS: CountOption = CountOption {
    key: "min-statements",
    doc: "Minimum statements in a function body for an `Args:` section to be required when a \
          non-obvious parameter is present; 0 disables this trigger.",
    default: LanguageDefaults::new(12, &[]),
};

const MIN_COGNITIVE_COMPLEXITY: CountOption = CountOption {
    key: "min-cognitive-complexity",
    doc: "Minimum cognitive complexity of a function for an `Args:` section to be required when a \
          non-obvious parameter is present; 0 disables this trigger.",
    default: LanguageDefaults::new(6, &[]),
};

const TEMPLATE: ViolationTemplate = violation_template! {
    summary: "Docstring of `{function}` has no `Args:` section explaining {parameters}.",
    rationale: "A flag, an optional value, a callback or a variadic parameter changes how `{function}` behaves in ways its name and type annotation do not state.",
    suggestion: "Add an `Args:` section to the docstring of `{function}` that explains {parameters}.",
};

/// The rule's declaration.
pub const RULE: CodeRule<(CountOption, CountOption)> = CodeRule {
    declaration: Declaration {
        name: RuleName("unexplained-parameter"),
        template: &TEMPLATE,
        languages: &[Language::Python],
        options: RuleOptions::code_rule((MIN_STATEMENTS, MIN_COGNITIVE_COMPLEXITY)),
        classification: Classification {
            topics: &[Topic::DOCSTRINGS],
            precision: Precision::Heuristic,
            consensus: Consensus::Opinionated,
            impacted_quality: ImpactedQuality::Maintainability,
        },
        doc: RuleDoc {
            summary: "Requires an `Args:` section when a nontrivial function has an optional, boolean, callable or variadic parameter.",
            what_it_does: indoc::indoc! {r"
                Flags a documented function or method that reaches `min-statements` or
                `min-cognitive-complexity`, has no `Args:` section (`Args:`, `Arguments:`,
                `Parameters:` or `Keyword Args:`), and takes at least one parameter whose
                contract is not stated by its name and type alone:

                - an `Optional[...]` / `... | None` annotation or a `= None` default;
                - a `bool` annotation or a `= True` / `= False` default;
                - a `Callable` annotation;
                - `*args` or `**kwargs`.

                One finding per function lists every such parameter in declaration order.

                For `__init__` and `__new__`, an `Args:` section in either the method docstring or
                the enclosing class docstring satisfies the rule, and for `__init__`, a parameter
                stored unchanged on `self` (`self.<name> = <name>`) is also satisfied by an entry
                under `Attributes:` in the class docstring. Not checked: functions without a
                docstring (left to `undocumented-nontrivial-function`), functions whose docstring
                already has an `Args:` section (incomplete sections are covered by Ruff `D417`),
                `_`-prefixed parameter names, nested functions, `@override` methods, property
                setters and deleters, data-model dunder methods other than `__init__`, `__new__`
                and `__call__`, and test files."},
            why_is_this_bad: indoc::indoc! {r"
                Google style allows omitting `Args:` when a function's name, signature and
                summary suffice, and Ruff `D417` only checks `Args:` sections that already exist.
                When a nontrivial function takes a boolean flag, a nullable parameter, a callback
                or `*args` / `**kwargs` and omits `Args:` altogether, callers have to read the
                body to learn what `None` falls back to, what each branch of the flag does, when
                the callback runs, or where variadic arguments are forwarded."},
            known_problems: Some(indoc::indoc! {r#"
                - Parameters whose non-obvious contract comes from a string or integer sentinel
                  default (`mode: str = "auto"`, `limit: int = -1`) or from a type alias hiding
                  `Optional`, `bool` or `Callable` are not recognized.
                - If the summary sentence already explains the parameter inline in prose without
                  an `Args:` section, the function is still flagged."#}),
            references: &[
                Reference {
                    title: "Google Python Style Guide §3.8.3: Functions and Methods",
                    url: "https://google.github.io/styleguide/pyguide.html#383-functions-and-methods",
                },
                Reference {
                    title: "Ruff rule `missing-argument-description` (D417)",
                    url: "https://docs.astral.sh/ruff/rules/undocumented-param/",
                },
            ],
            examples: &[Example {
                language: Language::Python,
                flagged: indoc::indoc! {r#"
                    def sync_records(
                        records: list[Record],
                        *,
                        dry_run: bool = False,
                        batch_size: int | None = None,
                    ) -> int:
                        """Synchronizes records with the remote store."""
                        if not records:
                            return 0
                        limit = batch_size or len(records)
                        sent = 0
                        for index in range(0, len(records), limit):
                            chunk = records[index : index + limit]
                            if not dry_run and chunk:
                                push_chunk(chunk)
                            sent += len(chunk)
                        return sent
                "#},
                flagged_span: "sync_records",
                fixed: indoc::indoc! {r#"
                    def sync_records(
                        records: list[Record],
                        *,
                        dry_run: bool = False,
                        batch_size: int | None = None,
                    ) -> int:
                        """Synchronizes records with the remote store.

                        Args:
                            records: Records to send.
                            dry_run: If True, counts batches without pushing them.
                            batch_size: Maximum records per push, or None to push all at once.
                        """
                        if not records:
                            return 0
                        limit = batch_size or len(records)
                        sent = 0
                        for index in range(0, len(records), limit):
                            chunk = records[index : index + limit]
                            if not dry_run and chunk:
                                push_chunk(chunk)
                            sent += len(chunk)
                        return sent
                "#},
            }],
        },
    },
    target: RuleTarget::SourceOnly,
    check: check_file,
};

const FUNCTION: &str = "function";
const PARAMETERS: &str = "parameters";

fn check_file(
    rule: &CodeRule<(CountOption, CountOption)>,
    path: &Path,
    file: &ParsedFile,
    (min_statements, min_cognitive_complexity): (usize, usize),
) -> Vec<Diagnostic> {
    ast::python::extract_function_signatures(file)
        .into_iter()
        .filter(|signature| {
            !signature.is_nested()
                && !signature.is_exempt_from_docstring_rules()
                && signature.is_nontrivial(min_statements, min_cognitive_complexity)
        })
        .filter_map(|signature| {
            let own_docstring = signature.docstring();
            let constructor_class = signature
                .is_constructor()
                .then(|| signature.enclosing_class())
                .flatten()
                .map(PythonClassInfo::from_declaration);
            let class_docstring = constructor_class
                .as_ref()
                .and_then(|class| class.declaration().docstring.as_ref());
            if own_docstring.is_none() && class_docstring.is_none() {
                return None;
            }
            if own_docstring
                .as_ref()
                .is_some_and(|doc| doc.has_section(DocstringSectionKind::Args))
                || class_docstring.is_some_and(|doc| doc.has_section(DocstringSectionKind::Args))
            {
                return None;
            }
            let documented_unchanged_attrs = constructor_class
                .as_ref()
                .filter(|_| signature.name == "__init__")
                .map(PythonClassInfo::documented_unchanged_constructor_parameters)
                .unwrap_or_default();
            let missing: Vec<String> = signature
                .parameters
                .iter()
                .filter(|parameter| {
                    !parameter.name.starts_with('_')
                        && parameter.is_non_obvious()
                        && !documented_unchanged_attrs.contains(&parameter.name)
                })
                .map(|parameter| format!("`{}`", parameter.display_name()))
                .collect();
            if missing.is_empty() {
                return None;
            }
            Some(rule.diagnostic_at_node(
                path,
                &signature.name_node,
                &[
                    (FUNCTION, &signature.name),
                    (PARAMETERS, &missing.join(", ")),
                ],
            ))
        })
        .collect()
}

#[cfg(test)]
crate::test_utils::rule_test!(
    RULE,
    {
        Python => {
            pass: [
                args_section_present_exempt => r#"
                    def sync_records(records: list[int], *, dry_run: bool = False) -> int:
                        """Synchronizes records.

                        Args:
                            records: Records to send.
                            dry_run: If True, skips network writes.
                        """
                        if not records:
                            return 0
                        total = 0
                        for item in records:
                            if item > 0:
                                if not dry_run:
                                    send(item)
                                total += 1
                        return total
                "#,
                ordinary_parameters_exempt => r#"
                    def merge_ranges(ranges: list[list[int]], step: int) -> list[list[int]]:
                        """Merges overlapping ranges."""
                        if not ranges:
                            return []
                        merged = []
                        for start, end in sorted(ranges):
                            if merged and start <= merged[-1][1] + step:
                                merged[-1][1] = max(merged[-1][1], end)
                            else:
                                merged.append([start, end])
                        return merged
                "#,
                below_complexity_gate_exempt => r#"
                    def set_verbose(enabled: bool = False) -> bool:
                        """Sets verbosity."""
                        return enabled
                "#,
                undocumented_function_exempt => r"
                    def sync_records(records: list[int], *, dry_run: bool = False) -> int:
                        if not records:
                            return 0
                        total = 0
                        for item in records:
                            if item > 0:
                                if not dry_run:
                                    send(item)
                                total += 1
                        return total
                ",
                nested_function_exempt => r#"
                    def outer(items: list[int]) -> int:
                        """Processes items."""
                        def inner(flag: bool = False) -> int:
                            """Helper."""
                            total = 0
                            for item in items:
                                if item > 0:
                                    if flag:
                                        total += item
                                    else:
                                        total -= item
                            return total
                        return inner()
                "#,
                override_method_exempt => r#"
                    from typing import override

                    class Worker(BaseWorker):
                        @override
                        def run(self, items: list[int], *, dry_run: bool = False) -> int:
                            """Runs the worker."""
                            total = 0
                            for item in items:
                                if item > 0:
                                    if not dry_run:
                                        send(item)
                                    total += 1
                            return total
                "#,
                underscore_prefixed_parameter_exempt => r#"
                    def sync_records(records: list[int], _dry_run: bool = False) -> int:
                        """Synchronizes records."""
                        total = 0
                        for item in records:
                            if item > 0:
                                if item % 2 == 0:
                                    total += item
                                else:
                                    total += 1
                        return total
                "#,
                unchanged_constructor_attribute_exempt => r#"
                    class Client:
                        """HTTP client.

                        Attributes:
                            timeout: Request timeout in seconds, or None for no timeout.
                        """

                        def __init__(self, timeout: float | None = None) -> None:
                            self.timeout = timeout
                            if timeout is not None:
                                if timeout < 0:
                                    if timeout < -1:
                                        raise ValueError(timeout)
                "#,
                class_args_section_satisfies_constructor_exempt => r#"
                    class Client:
                        """HTTP client.

                        Args:
                            timeout: Request timeout in seconds, or None for no timeout.
                        """

                        def __init__(self, timeout: float | None = None) -> None:
                            self.timeout = timeout or 5.0
                            if self.timeout < 0:
                                if self.timeout < -1:
                                    if self.timeout < -2:
                                        raise ValueError(self.timeout)
                "#,
            ],
            fail: [
                optional_and_bool_parameters_flagged => r#"
                    def sync_records(
                        records: list[int],
                        *,
                        dry_run: bool = False,
                        batch_size: int | None = None,
                    ) -> int:
                        """Synchronizes records."""
                        if not records:
                            return 0
                        total = 0
                        for item in records:
                            if item > 0:
                                if not dry_run:
                                    send(item, batch_size)
                                total += 1
                        return total
                "# => "sync_records",
                callable_and_variadic_parameters_flagged => r#"
                    from collections.abc import Callable

                    def dispatch(
                        items: list[int],
                        on_item: Callable[[int], None],
                        *args: int,
                        **kwargs: str,
                    ) -> int:
                        """Dispatches items."""
                        total = 0
                        for item in items:
                            if item > 0:
                                if item % 2 == 0:
                                    on_item(item)
                                total += 1
                        return total
                "# => "dispatch",
                transformed_constructor_parameter_flagged => r#"
                    class Client:
                        """HTTP client.

                        Attributes:
                            timeout: Request timeout in seconds, or None for no timeout.
                            retries: Retry count after clamping.
                        """

                        def __init__(
                            self,
                            timeout: float | None = None,
                            retries: int | None = None,
                        ) -> None:
                            self.timeout = timeout
                            self.retries = retries or 3
                            if self.retries < 0:
                                if self.retries < -1:
                                    if self.retries < -2:
                                        raise ValueError(self.retries)
                "# => "__init__",
                rebound_constructor_parameter_flagged => r#"
                    class Client:
                        """HTTP client.

                        Attributes:
                            endpoint: Normalized endpoint URL.
                        """

                        def __init__(self, endpoint: str | None = None) -> None:
                            endpoint = endpoint or "http://localhost"
                            self.endpoint = endpoint
                            if not self.endpoint:
                                if len(self.endpoint) == 0:
                                    if self.endpoint == "":
                                        raise ValueError(self.endpoint)
                "# => "__init__",
            ]
        }
    }
);
