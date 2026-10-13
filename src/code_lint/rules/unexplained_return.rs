//! Flags nontrivial Python functions whose docstring does not describe a non-obvious result.
//!
//! Design: `docs/dev/return_contracts/03_design.md` (D7).

use crate::code_lint::ast::python::{PythonDocstring, PythonFunctionSignature};
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
    doc: "Minimum statements in a function body for a non-obvious result to need a description; \
          0 disables this trigger.",
    default: LanguageDefaults::new(12, &[]),
};

const MIN_COGNITIVE_COMPLEXITY: CountOption = CountOption {
    key: "min-cognitive-complexity",
    doc: "Minimum cognitive complexity of a function for a non-obvious result to need a \
          description; 0 disables this trigger.",
    default: LanguageDefaults::new(6, &[]),
};

const TEMPLATE: ViolationTemplate = violation_template! {
    summary: "Docstring of `{function}` does not describe what it returns.",
    rationale: "An optional, boolean, union or unannotated result does not say what each outcome means, such as when `None` is returned or what `True` reports.",
    suggestion: "Add a `Returns:` section to the docstring of `{function}` that states what each possible result means, or a `Yields:` section for a generator.",
};

/// The rule's declaration.
pub const RULE: CodeRule<(CountOption, CountOption)> = CodeRule {
    declaration: Declaration {
        name: RuleName("unexplained-return"),
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
            summary: "Requires a `Returns:` section when a nontrivial function has an optional, boolean, union or unannotated result.",
            what_it_does: indoc::indoc! {r"
                Flags a documented function or method that reaches `min-statements` or
                `min-cognitive-complexity`, whose docstring has no `Returns:` / `Yields:` section
                and no summary opening with `Return(s)` or `Yield(s)`, and whose result is not
                stated by its type alone:

                - an optional result (`User | None`, `Optional[User]`);
                - a union of several types (`Path | str`);
                - `bool`, unless the name is a predicate (`is_*`, `has_*`, … as in
                  `non-boolean-predicate`) or the summary states the condition (`whether`,
                  `True if`, `True when`, `False if`, `False when`);
                - `Any` or `object`;
                - no annotation, when the body returns a value or yields.

                A single concrete type such as `int`, `Config` or `list[str]` is not flagged, nor
                is a tuple, which `positional-tuple-return` covers. Not checked: functions without
                a docstring (left to `undocumented-nontrivial-function`), nested functions,
                property getters, `@override` methods, property setters and deleters, data-model
                dunder methods other than `__init__`, `__new__` and `__call__`, and test files."},
            why_is_this_bad: indoc::indoc! {r"
                Google style lets a docstring omit `Returns:` when the summary and signature say
                enough. A self-describing type does (`-> Config`); an optional, a boolean or a
                union does not: the caller cannot tell when `None` comes back (not found? not
                ready? invalid input?), whether `True` from an action means success or a change,
                or which branch of `Path | str` produces which type, without reading the body."},
            known_problems: Some(indoc::indoc! {r"
                - Type aliases hiding `Optional` or a union are not expanded.
                - A summary that explains the result in prose without opening with `Return(s)`
                  still needs a `Returns:` section."}),
            references: &[
                Reference {
                    title: "Google Python Style Guide §3.8.3: Functions and Methods",
                    url: "https://google.github.io/styleguide/pyguide.html#383-functions-and-methods",
                },
                Reference {
                    title: "Ruff rule `docstring-missing-returns` (DOC201)",
                    url: "https://docs.astral.sh/ruff/rules/docstring-missing-returns/",
                },
            ],
            examples: &[Example {
                language: Language::Python,
                flagged: indoc::indoc! {r#"
                    def find_owner(paths: list[str], user: str) -> str | None:
                        """Finds the first path owned by `user`."""
                        for path in paths:
                            if path.startswith("/tmp"):
                                if not path.endswith(".lock"):
                                    continue
                            if owner_of(path) == user:
                                return path
                        return None
                "#},
                flagged_span: "find_owner",
                fixed: indoc::indoc! {r#"
                    def find_owner(paths: list[str], user: str) -> str | None:
                        """Finds the first path owned by `user`.

                        Returns:
                            The first matching path, or None when `user` owns none of `paths`.
                        """
                        for path in paths:
                            if path.startswith("/tmp"):
                                if not path.endswith(".lock"):
                                    continue
                            if owner_of(path) == user:
                                return path
                        return None
                "#},
            }],
        },
    },
    target: RuleTarget::SourceOnly,
    check: check_file,
};

const FUNCTION: &str = "function";

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
                && !signature.is_property_getter()
                && signature.docstring().is_some_and(|docstring| {
                    !docstring.describes_result() && has_non_obvious_result(signature, &docstring)
                })
                && signature.is_nontrivial(min_statements, min_cognitive_complexity)
        })
        .map(|signature| {
            rule.diagnostic_at_node(path, &signature.name_node, &[(FUNCTION, &signature.name)])
        })
        .collect()
}

/// Returns true if the result of `signature` needs a description beyond its type (D7).
fn has_non_obvious_result(
    signature: &PythonFunctionSignature<'_>,
    docstring: &PythonDocstring<'_>,
) -> bool {
    let Some(union) = signature.return_type_union() else {
        let results = signature.body_results();
        return results.returns_value || results.yields;
    };
    union.is_nullable()
        || union.branches.len() >= 2
        || union.is_opaque()
        || (union.is_boolean()
            && !ast::is_predicate_name(&signature.name)
            && !docstring.has_condition_summary())
}

#[cfg(test)]
crate::test_utils::rule_test!(
    RULE,
    {
        Python => {
            pass: [
                single_concrete_type_exempt => r#"
                    def collect(items: list[int]) -> list[int]:
                        """Collects even positive items."""
                        kept = []
                        for item in items:
                            if item > 0:
                                if item % 2 == 0:
                                    kept.append(item)
                        return kept

                    def pair(items: list[int]) -> tuple[int, int]:
                        """Pairs the extremes."""
                        low = high = 0
                        for item in items:
                            if item > 0:
                                if item % 2 == 0:
                                    high = item
                        return low, high
                "#,
                returns_section_exempt => r#"
                    def find(items: list[int]) -> int | None:
                        """Finds an item.

                        Returns:
                            The first even positive item, or None when there is none.
                        """
                        for item in items:
                            if item > 0:
                                if item % 2 == 0:
                                    return item
                        return None
                "#,
                result_verb_summary_exempt => r#"
                    def find(items: list[int]) -> int | None:
                        """Returns the first even positive item, or None when there is none."""
                        for item in items:
                            if item > 0:
                                if item % 2 == 0:
                                    return item
                        return None
                "#,
                predicate_name_boolean_exempt => r#"
                    def has_even(items: list[int]) -> bool:
                        """Scans items."""
                        for item in items:
                            if item > 0:
                                if item % 2 == 0:
                                    return True
                        return False
                "#,
                condition_summary_boolean_exempt => r#"
                    def sync(items: list[int]) -> bool:
                        """Synchronizes items and reports whether any was sent."""
                        sent = False
                        for item in items:
                            if item > 0:
                                if item % 2 == 0:
                                    sent = True
                        return sent
                "#,
                below_gate_exempt => r#"
                    def find(items: list[int]) -> int | None:
                        """Finds an item."""
                        return items[0] if items else None
                "#,
                undocumented_function_exempt => r"
                    def find(items: list[int]) -> int | None:
                        for item in items:
                            if item > 0:
                                if item % 2 == 0:
                                    return item
                        return None
                ",
                nested_function_exempt => r#"
                    def outer(items: list[int]) -> int:
                        """Counts items."""
                        def find() -> int | None:
                            """Finds an item."""
                            for item in items:
                                if item > 0:
                                    if item % 2 == 0:
                                        return item
                            return None
                        return 0 if find() is None else 1
                "#,
                docstring_exempt_method_exempt => r#"
                    from typing import override

                    class Finder(BaseFinder):
                        @override
                        def find(self, items: list[int]) -> int | None:
                            """Finds an item."""
                            for item in items:
                                if item > 0:
                                    if item % 2 == 0:
                                        return item
                            return None
                "#,
                property_getter_exempt => r#"
                    class Finder:
                        @property
                        def first_even(self) -> int | None:
                            """The first even positive item, if any."""
                            for item in self.items:
                                if item > 0:
                                    if item % 2 == 0:
                                        return item
                            return None
                "#,
                unannotated_without_value_exempt => r#"
                    def report(items):
                        """Prints even positive items."""
                        for item in items:
                            if item > 0:
                                if item % 2 == 0:
                                    print(item)
                "#,
            ],
            fail: [
                optional_result => r#"
                    def find(items: list[int]) -> int | None:
                        """Finds an item."""
                        for item in items:
                            if item > 0:
                                if item % 2 == 0:
                                    return item
                        return None
                "# => "find",
                action_boolean_result => r#"
                    def sync(items: list[int]) -> bool:
                        """Synchronizes items."""
                        sent = False
                        for item in items:
                            if item > 0:
                                if item % 2 == 0:
                                    sent = True
                        return sent
                "# => "sync",
                union_result => r#"
                    from pathlib import Path

                    def resolve(items: list[str]) -> Path | str:
                        """Resolves the target."""
                        for item in items:
                            if item:
                                if item.startswith("/"):
                                    return Path(item)
                        return "missing"
                "# => "resolve",
                opaque_result => r#"
                    from typing import Any

                    def load(items: list[str]) -> Any:
                        """Loads the payload."""
                        for item in items:
                            if item:
                                if item.startswith("{"):
                                    return item
                        return {}
                "# => "load",
                unannotated_value_result => r#"
                    def pick(items):
                        """Picks an item."""
                        for item in items:
                            if item > 0:
                                if item % 2 == 0:
                                    return item
                        return 0
                "# => "pick",
                unannotated_generator => r#"
                    def evens(items):
                        """Iterates over items."""
                        for item in items:
                            if item > 0:
                                if item % 2 == 0:
                                    yield item
                "# => "evens",
            ]
        }
    }
);
