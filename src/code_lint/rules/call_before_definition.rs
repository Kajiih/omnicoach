//! Flags functions and methods that call a sibling function or method defined later in the same
//! scope.

use crate::code_lint::ast::ParsedFile;
use crate::code_lint::ast::python::collect_forward_calls;
use crate::code_lint::contract::{CodeRule, RuleTarget};
use crate::diagnostic::{Diagnostic, RuleName, ViolationTemplate, violation_template};
use crate::rule_declaration::{
    Classification, Consensus, Declaration, Example, ImpactedQuality, Precision, Reference,
    RuleDoc, RuleOptions, Topic,
};
use ast_grep_language::SupportLang;
use std::path::Path;

const TEMPLATE: ViolationTemplate = violation_template! {
    summary: "Function `{function}` calls `{callee}()` before `{callee}` is defined.",
    rationale: "Calling a helper before its declaration forces readers scanning top-to-bottom to jump ahead in the file to learn its contract before finishing the caller.",
    suggestion: "Move the definition of `{callee}` above `{function}`.",
};

/// The rule's declaration.
pub const RULE: CodeRule = CodeRule {
    declaration: Declaration {
        name: RuleName("call-before-definition"),
        template: &TEMPLATE,
        languages: &[SupportLang::Python],
        options: RuleOptions::code_rule(()),
        classification: Classification {
            topics: &[Topic::DECLARATION_ORDER],
            precision: Precision::Heuristic,
            consensus: Consensus::Opinionated,
            impacted_quality: ImpactedQuality::Maintainability,
        },
        doc: RuleDoc {
            summary: "Flags functions and methods that call a sibling function or method defined \
                      later in the same module or class.",
            what_it_does: "Checks Python module and class bodies for functions and methods that \
                           call a sibling function (`helper()`) or method (`self.helper()`, \
                           `cls.helper()`) whose definition appears later in the same scope. \
                           At most one finding is reported per caller and callee pair, at the \
                           first forward call site.\n\n\
                           Multi-part definitions that share a name (`@overload` stubs and \
                           their implementation, or a `@property` getter and its setter or \
                           deleter) are grouped at the position of their first `def`. \
                           Several constructs are not flagged: direct and mutual recursion \
                           (where two or more functions call each other in a cycle, so one must \
                           appear first), calls inside class constructors (`__init__`, \
                           `__new__`, `__post_init__`, which stay at the top of the class), \
                           calls where the callee name is shadowed by a local variable, \
                           parameter or import, and non-call references such as type \
                           annotations, default parameter values, decorators, attribute \
                           assignments (`self.value = 1`), and first-class function callbacks. \
                           Top-level module statements are not checked (covered by Ruff \
                           `F821`). Test files are not checked.",
            why_is_this_bad: "In a codebase organized bottom-up (leaf helpers first, callers \
                              and entrypoints such as `main()` at the bottom), a forward call \
                              breaks sequential reading order: a reader scanning from top to \
                              bottom encounters a call to a local helper before seeing its \
                              signature, parameters or docstring, and has to jump down and \
                              back up to follow the control flow.\n\n\
                              Move the helper function or method definition above the first \
                              function or method that calls it.",
            references: &[
                Reference {
                    title: "ESLint: no-use-before-define",
                    url: "https://eslint.org/docs/latest/rules/no-use-before-define",
                },
                Reference {
                    title: "Ruff F821: undefined-name",
                    url: "https://docs.astral.sh/ruff/rules/undefined-name/",
                },
            ],
            examples: &[Example {
                language: SupportLang::Python,
                flagged: indoc::indoc! {r"
                    def load_port(raw: str) -> int:
                        return parse_int(raw.strip())

                    def parse_int(value: str) -> int:
                        return int(value)
                "},
                flagged_span: "parse_int(raw.strip())",
                fixed: indoc::indoc! {r"
                    def parse_int(value: str) -> int:
                        return int(value)

                    def load_port(raw: str) -> int:
                        return parse_int(raw.strip())
                "},
            }],
        },
    },
    target: RuleTarget::SourceOnly,
    check: check_file,
};

fn check_file(rule: &CodeRule, path: &Path, file: &ParsedFile, (): ()) -> Vec<Diagnostic> {
    collect_forward_calls(file)
        .into_iter()
        .map(|call| {
            rule.diagnostic_at_node(
                path,
                &call.node,
                &[
                    ("function", &call.caller_name),
                    ("callee", &call.callee_name),
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
                defined_before_use_module_and_class => r#"
                    def helper(x: int) -> int:
                        return x + 1

                    def run(x: int) -> int:
                        return helper(x)

                    class Processor:
                        def step(self, x: int) -> int:
                            return x * 2

                        def execute(self, x: int) -> int:
                            return self.step(x)
                "#,
                direct_self_recursion => r#"
                    def factorial(n: int) -> int:
                        return 1 if n <= 1 else n * factorial(n - 1)

                    class Tree:
                        def walk(self, depth: int) -> int:
                            return 0 if depth <= 0 else self.walk(depth - 1)
                "#,
                mutual_recursion_pair_module_and_class => r#"
                    def is_even(n: int) -> bool:
                        return True if n == 0 else is_odd(n - 1)

                    def is_odd(n: int) -> bool:
                        return False if n == 0 else is_even(n - 1)

                    class Parity:
                        def check_even(self, n: int) -> bool:
                            return True if n == 0 else self.check_odd(n - 1)

                        def check_odd(self, n: int) -> bool:
                            return False if n == 0 else self.check_even(n - 1)
                "#,
                mutual_recursion_three_way_cycle => r#"
                    def parse_expr(tokens: list[str]) -> str:
                        return parse_term(tokens)

                    def parse_term(tokens: list[str]) -> str:
                        return parse_atom(tokens)

                    def parse_atom(tokens: list[str]) -> str:
                        return parse_expr(tokens)
                "#,
                class_constructors_calling_later_helpers_exempt => r#"
                    class Service:
                        def __new__(cls):
                            cls._prepare()
                            return super().__new__(cls)

                        def __init__(self, raw: str) -> None:
                            self.value = self._validate(raw)

                        def __post_init__(self) -> None:
                            self._finalize()

                        @classmethod
                        def _prepare(cls) -> None:
                            pass

                        def _validate(self, raw: str) -> str:
                            return raw.strip()

                        def _finalize(self) -> None:
                            pass
                "#,
                overload_stubs_and_implementation_grouped => r#"
                    from typing import overload

                    @overload
                    def parse(raw: str) -> int: ...

                    @overload
                    def parse(raw: bytes) -> int: ...

                    def run(raw: str) -> int:
                        return parse(raw)

                    def parse(raw: str | bytes) -> int:
                        return int(raw)
                "#,
                property_getter_and_setter_pair => r#"
                    class Box:
                        @property
                        def value(self) -> int:
                            return self._value

                        def reset(self) -> None:
                            self.value(0)

                        @value.setter
                        def value(self, new_val: int) -> None:
                            self._value = new_val
                "#,
                shadowed_by_parameter_or_nested_lambda_parameter => r#"
                    from typing import Any

                    def run_with_param(helper) -> int:
                        return helper()

                    def run_with_typed_varargs(*helper: Any) -> int:
                        return helper()

                    def run_with_lambda(items: list[int]) -> list[int]:
                        return list(map(lambda helper: helper(1), items))

                    def helper(x: int = 0) -> int:
                        return x
                "#,
                shadowed_by_local_assignments_and_unpacking => r#"
                    def via_assign(factory) -> int:
                        helper = factory()
                        return helper()

                    def via_unpack(pair) -> int:
                        first, helper = pair
                        return helper(first)

                    def via_star_unpack(items) -> int:
                        [helper, *rest] = items
                        return helper(len(rest))

                    def via_walrus(factory) -> int:
                        if (helper := factory()):
                            return helper()
                        return 0

                    def helper(x: int = 0) -> int:
                        return x
                "#,
                shadowed_by_for_with_except_match_and_import => r#"
                    def via_loops(fns) -> list[int]:
                        for helper in fns:
                            helper()
                        return [helper() for helper in fns]

                    def via_with_and_except(ctx) -> int:
                        try:
                            with ctx() as helper:
                                return helper()
                        except RuntimeError as helper_err:
                            return helper_err()

                    def via_match(action) -> int:
                        match action:
                            case helper:
                                return helper()

                    def via_import_and_nested_def() -> int:
                        from math import cos as helper
                        def local_fn() -> int:
                            return 1
                        return int(helper(0)) + local_fn()

                    def helper() -> int:
                        return 0

                    def helper_err() -> int:
                        return -1

                    def local_fn() -> int:
                        return 2
                "#,
                non_call_references_and_annotations_not_flagged => r#"
                    class Box:
                        def set_state(self) -> None:
                            self.helper = 1
                            _ = self.helper

                        def helper(self) -> int:
                            return 0

                    def register(cb):
                        return cb

                    def annotate(x: helper, fn_arg=helper) -> helper:
                        return register(helper)

                    def helper() -> int:
                        return 1
                "#,
                other_receiver_and_staticmethod_not_flagged => r#"
                    class Worker:
                        def delegate(self, other) -> int:
                            return other.step()

                        @staticmethod
                        def static_call(self) -> int:
                            return self.step()

                        def step(self) -> int:
                            return 1
                "#,
                top_level_statements_not_checked => r#"
                    if __name__ == "__main__":
                        main()

                    def main() -> int:
                        return 0
                "#,
            ],
            fail: [
                module_function_calls_later_function => r#"
                    def run(x: int) -> int:
                        return helper(x)

                    def helper(x: int) -> int:
                        return x + 1
                "# => r#"helper(x)"#,
                async_function_calls_later_async_function => r#"
                    async def fetch() -> int:
                        return await load()

                    async def load() -> int:
                        return 42
                "# => r#"load()"#,
                class_method_calls_later_self_method => r#"
                    class Runner:
                        def run(self) -> int:
                            return self.step()

                        def step(self) -> int:
                            return 1
                "# => r#"self.step()"#,
                classmethod_calls_later_cls_method => r#"
                    class Config:
                        @classmethod
                        def from_raw(cls, text: str) -> str:
                            return cls.normalize(text)

                        @classmethod
                        def normalize(cls, text: str) -> str:
                            return text.strip()
                "# => r#"cls.normalize(text)"#,
                call_inside_comprehension_to_later_function => r#"
                    def process(items: list[int]) -> list[int]:
                        return [transform(item) for item in items]

                    def transform(item: int) -> int:
                        return item * 2
                "# => r#"transform(item)"#,
                call_after_non_shadowing_nested_lambda_parameter => r#"
                    def run(items: list[int]) -> int:
                        _ = list(map(lambda helper: helper, items))
                        return helper()

                    def helper() -> int:
                        return 1
                "# => r#"helper()"#,
                attribute_assignment_does_not_shadow_module_function => r#"
                    def run(holder, pair) -> int:
                        first, holder.helper = pair
                        return helper()

                    def helper() -> int:
                        return 2
                "# => r#"helper()"#,
                global_declaration_keeps_module_function_unshadowed => r#"
                    def run() -> int:
                        global helper
                        return helper()

                    def helper() -> int:
                        return 1
                "# => r#"helper()"#,
                multiple_calls_to_same_later_function_reported_once => r#"
                    def run() -> int:
                        first = helper(1)
                        second = helper(2)
                        return first + second

                    def helper(x: int) -> int:
                        return x
                "# => r#"helper(1)"#,
                one_way_call_into_mutual_recursion_cycle_flagged => r#"
                    def entry(n: int) -> bool:
                        return is_even(n)

                    def is_even(n: int) -> bool:
                        return True if n == 0 else is_odd(n - 1)

                    def is_odd(n: int) -> bool:
                        return False if n == 0 else is_even(n - 1)
                "# => r#"is_even(n)"#,
            ],
        }
    }
);
