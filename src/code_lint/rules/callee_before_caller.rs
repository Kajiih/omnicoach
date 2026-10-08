//! Flags private functions and methods defined before their private callers.

use crate::code_lint::ast::{self, ParsedFile};
use crate::code_lint::contract::{CodeRule, RuleTarget};
use crate::diagnostic::{Diagnostic, Language, RuleName, ViolationTemplate, violation_template};
use crate::rule_declaration::{
    Classification, Consensus, Declaration, Example, ImpactedQuality, Precision, Reference,
    RuleDoc, RuleOptions, Topic,
};
use std::path::Path;

const TEMPLATE: ViolationTemplate = violation_template! {
    summary: "Private callee `{function}` is defined before its private caller `{caller}`.",
    rationale: "Defining lower-abstraction private helpers above `{caller}` inverts the top-down reading flow.",
    suggestion: "Move `{function}` below `{caller}` so higher-level callers precede lower-level callees.",
};

/// The rule's declaration.
pub const RULE: CodeRule = CodeRule {
    declaration: Declaration {
        name: RuleName("callee-before-caller"),
        template: &TEMPLATE,
        languages: &[Language::Python, Language::Rust],
        options: RuleOptions::code_rule(()),
        classification: Classification {
            topics: &[Topic::DECLARATION_ORDER],
            precision: Precision::Exact,
            consensus: Consensus::Opinionated,
            impacted_quality: ImpactedQuality::Maintainability,
        },
        doc: RuleDoc {
            summary: "Flags private functions and methods defined before their private callers.",
            what_it_does: indoc::indoc! {r"
                Checks module scopes, Python `class` definitions, and Rust inherent `impl` blocks
                for a private function or method `callee` declared above its private caller
                `caller` (`Private -> Private`).

                Public functions and methods (`Public -> Public`) are not constrained because
                public APIs legitimately order either top-down (orchestrator before step) or
                core-primitive-first (fundamental accessor before convenience wrapper).
                Self-recursive and mutually recursive functions (Strongly Connected Components in
                the scope's call graph) and cross-tier calls (`Public -> Private`, governed by
                `private-before-public-function` and `uncolocated-helper`) are also exempt. Any
                function already flagged by `private-before-public-function` or
                `uncolocated-helper` is skipped so a misplaced helper is never reported twice."},
            why_is_this_bad: indoc::indoc! {r"
                Unlike public entrypoints, a private helper exists solely as an internal
                decomposition step of its callers. Defining private callees above their private
                callers forces readers to encounter low-level leaf utilities before the
                higher-level helper logic that gives them context.

                Order private helpers from higher-level callers down to lower-level callees."},
            references: &[
                Reference {
                    title: "Robert C. Martin: Clean Code — Chapter 5: Formatting (The Stepdown Rule)",
                    url: "https://www.oreilly.com/library/view/clean-code-a/9780136083238/",
                },
                Reference {
                    title: "Soloway & Ehrlich (1984): Empirical Studies of Programming Knowledge",
                    url: "https://ieeexplore.ieee.org/document/5010283",
                },
            ],
            examples: &[
                Example {
                    language: Language::Python,
                    flagged: indoc::indoc! {r"
                        def parse_document(raw: str) -> list[str]:
                            return _split_lines(raw)

                        def _trim_line(line: str) -> str:
                            return line.strip()

                        def _split_lines(raw: str) -> list[str]:
                            return [_trim_line(line) for line in raw.splitlines()]
                    "},
                    flagged_span: "_trim_line",
                    fixed: indoc::indoc! {r"
                        def parse_document(raw: str) -> list[str]:
                            return _split_lines(raw)

                        def _split_lines(raw: str) -> list[str]:
                            return [_trim_line(line) for line in raw.splitlines()]

                        def _trim_line(line: str) -> str:
                            return line.strip()
                    "},
                },
                Example {
                    language: Language::Rust,
                    flagged: indoc::indoc! {r"
                        pub fn parse_config(raw: &str) -> Vec<&str> {
                            parse_lines(raw)
                        }

                        fn strip_line(line: &str) -> &str {
                            line.trim()
                        }

                        fn parse_lines(raw: &str) -> Vec<&str> {
                            raw.lines().map(strip_line).collect()
                        }
                    "},
                    flagged_span: "strip_line",
                    fixed: indoc::indoc! {r"
                        pub fn parse_config(raw: &str) -> Vec<&str> {
                            parse_lines(raw)
                        }

                        fn parse_lines(raw: &str) -> Vec<&str> {
                            raw.lines().map(strip_line).collect()
                        }

                        fn strip_line(line: &str) -> &str {
                            line.trim()
                        }
                    "},
                },
            ],
        },
    },
    target: RuleTarget::SourceOnly,
    check: check_file,
};

fn check_file(rule: &CodeRule, path: &Path, file: &ParsedFile, (): ()) -> Vec<Diagnostic> {
    ast::collect_call_cluster_findings(file)
        .callee_before_caller
        .iter()
        .map(|finding| {
            rule.diagnostic_at_node(
                path,
                &finding.name_node,
                &[("function", &finding.function), ("caller", &finding.caller)],
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
                top_down_caller_before_callee_across_tiers => r#"
                    def compile_all(raw: str) -> str:
                        return compile_single(raw)

                    def compile_single(raw: str) -> str:
                        return _parse_tokens(raw)

                    def _parse_tokens(raw: str) -> str:
                        return _strip_whitespace(raw)

                    def _strip_whitespace(raw: str) -> str:
                        return raw.strip()
                "#,
                public_callee_before_public_caller_exempt => r#"
                    def parse_item(raw: str) -> str:
                        return raw.strip()

                    def parse_batch(items: list[str]) -> list[str]:
                        return [parse_item(item) for item in items]
                "#,
                mutual_recursion_and_constructors_exempt => r#"
                    def _even(number: int) -> bool:
                        return True if number == 0 else _odd(number - 1)

                    def _odd(number: int) -> bool:
                        return False if number == 0 else _even(number - 1)

                    class Buffer:
                        def __init__(self, size: int) -> None:
                            self.size = size

                        def reset(self) -> None:
                            self.__init__(0)
                "#,
                local_shadowing_and_exclusive_to_shared_calls_exempt => r#"
                    def action_one(raw: str) -> str:
                        return _exclusive_one(raw)

                    def _exclusive_one(raw: str) -> str:
                        return _shared_leaf(raw)

                    def action_two(raw: str) -> str:
                        _exclusive_one = str.strip
                        return _shared_caller(raw) if False else _exclusive_one(raw)

                    def _shared_caller(raw: str) -> str:
                        return _shared_leaf(raw)

                    def _shared_leaf(raw: str) -> str:
                        return raw.strip()
                "#,
            ],
            fail: [
                exclusive_helper_callee_before_caller => r#"
                    def run_pipeline(raw: str) -> str:
                        return _step_one(raw)

                    def _step_two(raw: str) -> str:
                        return raw.strip()

                    def _step_one(raw: str) -> str:
                        return _step_two(raw)
                "# => "_step_two",
                class_private_callee_before_private_caller => r#"
                    class Decoder:
                        def decode(self, raw: str) -> str:
                            return self._decode_frame(raw)

                        def _parse_byte(self, raw: str) -> str:
                            return raw.strip()

                        def _decode_frame(self, raw: str) -> str:
                            return self._parse_byte(raw)
                "# => "_parse_byte",
            ],
        },
        Rust => {
            pass: [
                top_down_module_and_impl_order => r#"
                    pub struct Compiler;

                    impl Compiler {
                        pub fn new() -> Self {
                            Self
                        }

                        pub fn rebuild(&self) -> Self {
                            Self::new()
                        }

                        pub fn compile(&self, src: &str) -> usize {
                            self.lower_ast(src)
                        }

                        fn lower_ast(&self, src: &str) -> usize {
                            Self::count_bytes(src)
                        }

                        fn count_bytes(src: &str) -> usize {
                            src.len()
                        }
                    }
                "#,
                mutual_recursion_and_local_binding_shadowing_exempt => r#"
                    fn is_even(n: usize) -> bool {
                        if n == 0 { true } else { is_odd(n - 1) }
                    }

                    fn is_odd(n: usize) -> bool {
                        if n == 0 { false } else { is_even(n - 1) }
                    }

                    pub fn evaluate(n: usize) -> bool {
                        let evaluate_shadow = |x: usize| x > 0;
                        evaluate_shadow(n) && is_even(n)
                    }
                "#,
            ],
            fail: [
                private_callee_before_private_caller_in_module => r#"
                    fn leaf_helper(input: &str) -> &str {
                        input.trim()
                    }

                    fn middle_helper(input: &str) -> usize {
                        leaf_helper(input).len()
                    }
                "# => "leaf_helper",
                impl_private_callee_before_private_caller => r#"
                    pub struct Lexer;

                    impl Lexer {
                        pub fn tokenize(&self, input: &str) -> usize {
                            self.scan_tokens(input)
                        }

                        fn is_delimiter(ch: char) -> bool {
                            ch.is_whitespace()
                        }

                        fn scan_tokens(&self, input: &str) -> usize {
                            input.chars().filter(|&ch| Self::is_delimiter(ch)).count()
                        }
                    }
                "# => "is_delimiter",
            ],
        },
    }
);
