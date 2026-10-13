//! Requires a docstring on Python functions of nontrivial size or logic.
//!
//! Design: `docs/dev/complex_function_docstring/03_design.md`, with the gate of
//! `03b_complexity_gate.md`: a function is nontrivial when its statement count or its cognitive
//! complexity reaches a threshold. Nested functions are not checked: their logic already counts
//! toward the enclosing function.

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
    doc: "Minimum statements in a function body for a docstring to be required; 0 disables this \
          trigger.",
    default: LanguageDefaults::new(12, &[]),
};

const MIN_COGNITIVE_COMPLEXITY: CountOption = CountOption {
    key: "min-cognitive-complexity",
    doc: "Minimum cognitive complexity of a function for a docstring to be required; 0 disables \
          this trigger.",
    default: LanguageDefaults::new(6, &[]),
};

const TEMPLATE: ViolationTemplate = violation_template! {
    summary: "Function `{function}` has a {metric} of {count} and no docstring.",
    rationale: "Without a docstring, a caller has to read the whole body to learn what the function does and how to call it.",
    suggestion: "Add a docstring to `{function}` that states what it does, what each parameter means and what it returns.",
};

/// The rule's declaration.
pub const RULE: CodeRule<(CountOption, CountOption)> = CodeRule {
    declaration: Declaration {
        name: RuleName("undocumented-nontrivial-function"),
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
            summary: "Requires a docstring on functions with many statements or a high cognitive complexity.",
            what_it_does: indoc::indoc! {r"
                Flags a function or method without a docstring that has at least `min-statements`
                statements or a cognitive complexity of at least `min-cognitive-complexity`. The
                message names the first threshold reached, statements first.

                Every statement of the body counts 1, compound statements and each `elif`
                included; the docstring does not count, and a nested `def` or `class` counts 1
                without its body. Cognitive complexity follows the SonarSource white paper:
                `if`, ternaries, loops, `except` and `match` add 1 plus their nesting depth,
                `elif` and `else` add 1, each run of `and` / `or` adds 1, and a recursive call
                adds 1; comprehensions, `with` and `try` add nothing.

                Private functions are checked. Not checked: functions nested in another function
                (their logic counts toward the enclosing one), data-model dunder methods other
                than `__init__`, `__new__` and `__call__`, `@override` methods, property setters
                and deleters, `__init__` and `__new__` of a class that has a docstring, and test
                files."},
            why_is_this_bad: indoc::indoc! {r"
                Google style requires a docstring on a function of nontrivial size or with
                non-obvious logic, whether public or private. Without one, a caller has to read
                the body to learn what the function does, what each argument means and what comes
                back, and a maintainer cannot tell intended behavior from accident."},
            known_problems: Some(indoc::indoc! {r"
                - Both measures stand in for what makes a docstring necessary, such as an
                  unusual return value, mutated arguments or side effects. Flat `elif` dispatch
                  and guard chains score high; short numeric pipelines and parsers with
                  surprising results score low.
                - Recursion is matched by name only: a call to the function's own name, or to
                  `self.<name>` / `cls.<name>` in a method.
                - Only the first threshold reached is named when both are."}),
            references: &[
                Reference {
                    title: "Google Python Style Guide §3.8.3: Functions and Methods",
                    url: "https://google.github.io/styleguide/pyguide.html#383-functions-and-methods",
                },
                Reference {
                    title: "SonarSource: Cognitive Complexity, a new way of measuring understandability",
                    url: "https://www.sonarsource.com/docs/CognitiveComplexity.pdf",
                },
            ],
            examples: &[Example {
                language: Language::Python,
                flagged: indoc::indoc! {r"
                    def merge_ranges(ranges):
                        if not ranges:
                            return []
                        merged = []
                        for start, end in sorted(ranges):
                            if merged and start <= merged[-1][1]:
                                merged[-1][1] = max(merged[-1][1], end)
                            else:
                                merged.append([start, end])
                        return merged
                "},
                flagged_span: "merge_ranges",
                fixed: indoc::indoc! {r#"
                    def merge_ranges(ranges):
                        """Merges overlapping `[start, end]` ranges.

                        Args:
                            ranges: Ranges in any order; touching ranges are merged too.

                        Returns:
                            The merged ranges as lists, sorted by start.
                        """
                        if not ranges:
                            return []
                        merged = []
                        for start, end in sorted(ranges):
                            if merged and start <= merged[-1][1]:
                                merged[-1][1] = max(merged[-1][1], end)
                            else:
                                merged.append([start, end])
                        return merged
                "#},
            }],
        },
    },
    target: RuleTarget::SourceOnly,
    check: check_file,
};

const FUNCTION: &str = "function";
const METRIC: &str = "metric";
const COUNT: &str = "count";

fn check_file(
    rule: &CodeRule<(CountOption, CountOption)>,
    path: &Path,
    file: &ParsedFile,
    (min_statements, min_cognitive_complexity): (usize, usize),
) -> Vec<Diagnostic> {
    ast::python::extract_function_signatures(file)
        .into_iter()
        .filter(|signature| {
            signature.docstring().is_none()
                && !signature.is_nested()
                && !signature.is_exempt_from_docstring_rules()
                && !signature.is_documented_by_enclosing_class()
        })
        .filter_map(|signature| {
            let metric = signature.nontrivial_metric(min_statements, min_cognitive_complexity)?;
            Some(rule.diagnostic_at_node(
                path,
                &signature.name_node,
                &[
                    (FUNCTION, &signature.name),
                    (METRIC, metric.name()),
                    (COUNT, &metric.value().to_string()),
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
                documented_function => r#"
                    def merge_ranges(ranges):
                        """Merges overlapping ranges."""
                        if not ranges:
                            return []
                        merged = []
                        for start, end in sorted(ranges):
                            if merged and start <= merged[-1][1]:
                                merged[-1][1] = max(merged[-1][1], end)
                            else:
                                merged.append([start, end])
                        return merged
                "#,
                below_both_thresholds => r#"
                    def configure(config, verbose):
                        config.name = "server"
                        config.port = 8080
                        config.host = "localhost"
                        config.workers = 4
                        if verbose:
                            config.log_level = "debug"
                        elif config.quiet:
                            config.log_level = "error"
                        else:
                            config.log_level = "info"
                        config.metrics = config.enabled and config.exported or config.forced
                        return config
                "#,
                nested_function_exempt => r#"
                    def make_merger():
                        """Returns a function that merges overlapping ranges."""
                        def merge(ranges):
                            if not ranges:
                                return []
                            merged = []
                            for start, end in sorted(ranges):
                                if merged and start <= merged[-1][1]:
                                    merged[-1][1] = max(merged[-1][1], end)
                                else:
                                    merged.append([start, end])
                            return merged
                        return merge
                "#,
                override_method_exempt => r#"
                    from typing import override

                    class RangeSet(Base):
                        @override
                        def merge(self, ranges):
                            if not ranges:
                                return []
                            merged = []
                            for start, end in sorted(ranges):
                                if merged and start <= merged[-1][1]:
                                    merged[-1][1] = max(merged[-1][1], end)
                                else:
                                    merged.append([start, end])
                            return merged
                "#,
                data_model_dunder_exempt => r#"
                    class Version:
                        def __lt__(self, other):
                            if self.major != other.major:
                                return self.major < other.major
                            if self.minor != other.minor:
                                return self.minor < other.minor
                            if self.patch != other.patch:
                                return self.patch < other.patch
                            if self.pre and other.pre:
                                return self.pre < other.pre
                            return self.pre and not other.pre
                "#,
                property_accessors_exempt => r#"
                    class Server:
                        @property
                        def port(self):
                            """The listening port."""
                            return self._port

                        @port.setter
                        def port(self, value):
                            if not isinstance(value, int):
                                raise TypeError(value)
                            if value < 0 or value > 65535:
                                raise ValueError(value)
                            for listener in self._listeners:
                                if listener.port == value:
                                    raise ValueError(value)
                            self._port = value

                        @port.deleter
                        def port(self):
                            for listener in self._listeners:
                                if listener.port == self._port:
                                    if listener.active:
                                        listener.close()
                            self._port = None
                "#,
                constructor_of_documented_class_exempt => r#"
                    class Server:
                        """A TCP server listening on one port."""

                        def __init__(self, port, listeners):
                            if not isinstance(port, int):
                                raise TypeError(port)
                            if port < 0 or port > 65535:
                                raise ValueError(port)
                            for listener in listeners:
                                if listener.port == port:
                                    raise ValueError(port)
                            self._port = port
                "#,
            ],
            fail: [
                statement_count_reaches_threshold => r#"
                    def _configure(config):
                        config.name = "server"
                        config.port = 8080
                        config.host = "localhost"
                        config.workers = 4
                        config.timeout = 30
                        config.retries = 3
                        config.backoff = 2
                        config.log_level = "info"
                        config.metrics = True
                        config.tracing = False
                        config.cache_size = 128
                        return config
                "# => "_configure",
                cognitive_complexity_reaches_threshold => r#"
                    def merge_ranges(ranges):
                        if not ranges:
                            return []
                        merged = []
                        for start, end in sorted(ranges):
                            if merged and start <= merged[-1][1]:
                                merged[-1][1] = max(merged[-1][1], end)
                            else:
                                merged.append([start, end])
                        return merged
                "# => "merge_ranges",
                async_function => r#"
                    async def drain(queue, sink):
                        while not queue.empty():
                            item = await queue.get()
                            if item is None:
                                break
                            if item.valid and not item.duplicate:
                                await sink.send(item)
                "# => "drain",
                constructor_of_undocumented_class => r#"
                    class Server:
                        def __init__(self, port, listeners):
                            if not isinstance(port, int):
                                raise TypeError(port)
                            if port < 0 or port > 65535:
                                raise ValueError(port)
                            for listener in listeners:
                                if listener.port == port:
                                    raise ValueError(port)
                            self._port = port
                "# => "__init__",
                call_dunder => r#"
                    class Handler:
                        """Handles requests."""

                        def __call__(self, request):
                            if request.method == "GET" and request.cached:
                                return self.get(request)
                            elif request.method == "POST":
                                return self.post(request)
                            elif request.method == "PUT":
                                return self.put(request)
                            elif request.method == "DELETE":
                                return self.delete(request)
                            else:
                                return self.reject(request)
                "# => "__call__",
                method_of_class_nested_in_function => r#"
                    def make_handler():
                        """Builds a request handler class."""
                        class Handler:
                            def handle(self, request):
                                if request.method == "GET" and request.cached:
                                    return self.get(request)
                                elif request.method == "POST":
                                    return self.post(request)
                                elif request.method == "PUT":
                                    return self.put(request)
                                elif request.method == "DELETE":
                                    return self.delete(request)
                                else:
                                    return self.reject(request)
                        return Handler
                "# => "handle",
            ],
        },
    }
);
