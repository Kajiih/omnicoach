//! Flags private functions and methods defined above their owning or calling public entrypoints.

use crate::code_lint::ast::{self, ParsedFile};
use crate::code_lint::contract::{CodeRule, RuleTarget};
use crate::diagnostic::{Diagnostic, Language, RuleName, ViolationTemplate, violation_template};
use crate::rule_declaration::{
    Classification, Consensus, Declaration, Example, ImpactedQuality, Precision, Reference,
    RuleDoc, RuleOptions, Topic,
};
use std::path::Path;

const TEMPLATE: ViolationTemplate = violation_template! {
    summary: "Private helper `{function}` is defined before public entrypoint `{caller}`.",
    rationale: "Placing lower-abstraction private helpers above `{caller}` buries the public contract of the scope behind implementation details.",
    suggestion: "Move `{function}` below `{caller}` (either immediately after `{caller}` if single-use, or into the trailing private helper section).",
};

/// The rule's declaration.
pub const RULE: CodeRule = CodeRule {
    declaration: Declaration {
        name: RuleName("private-before-public-function"),
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
            summary: "Flags private functions and methods defined above their owning or calling public entrypoints.",
            what_it_does: indoc::indoc! {r"
                Checks module scopes, Python `class` definitions, and Rust inherent `impl` blocks
                for private functions or methods declared before the public entrypoints they serve.

                A private helper's public callers are the public entrypoints that reach it,
                directly or through other private helpers (the walk stops at public functions):
                - A helper used by **one** public entrypoint is flagged when declared above it, and
                  is allowed below it (either immediately after it or in the trailing private
                  helper section after all public functions, as checked by `uncolocated-helper`).
                - A helper **shared** by several public entrypoints belongs to a lower abstraction
                  layer than any of them and is flagged when declared above any of them.
                - A private function **no public entrypoint reaches** is flagged when declared
                  above the last public entrypoint in the scope.

                In Python, public functions and dunder methods (`__name__`) are public;
                single-underscore (`_name`) and name-mangled (`__name`) names are private, and
                `@overload` signatures and `@property` accessors (`getter`, `setter`, `deleter`)
                are grouped at their first definition. In Rust, items with a visibility qualifier
                (`pub`, `pub(crate)`, `pub(super)`, `pub(in ...)`) or `fn main` are public; bare
                `fn` items are private. Scopes with only private functions, Rust trait `impl`
                blocks, test files, and `#[cfg(test)]` / `#[test]` items are not checked. Together
                with `uncolocated-helper` and `callee-before-caller`, this rule forms a disjoint
                three-stage call-cluster check. Calls are resolved by name within the file,
                assuming the code compiles (Rust) or type-checks (Python)."},
            why_is_this_bad: indoc::indoc! {r"
                A private helper is a lower-abstraction building block than the public entrypoint
                that calls it, and a helper shared across multiple public entrypoints is at a lower
                abstraction level still. Placing private helpers above their public callers inverts
                the abstraction hierarchy and forces readers to wade through internal mechanics
                before seeing the scope's public interface.

                Place single-use helpers either immediately below their owning public entrypoint or
                in the trailing private helper section, and place shared helpers in the trailing
                private helper section below all public functions."},
            references: &[
                Reference {
                    title: "Robert C. Martin: Clean Code — Chapter 5 & Chapter 10 (The Newspaper Metaphor, The Stepdown Rule, Class Organization)",
                    url: "https://www.oreilly.com/library/view/clean-code-a/9780136083238/",
                },
                Reference {
                    title: "wemake-python-styleguide: WPS338 WrongMethodOrderViolation",
                    url: "https://wemake-python-styleguide.readthedocs.io/en/latest/pages/usage/violations/consistency.html",
                },
            ],
            examples: &[
                Example {
                    language: Language::Python,
                    flagged: indoc::indoc! {r"
                        class TokenVerifier:
                            def _normalize(self, raw: str) -> str:
                                return raw.strip()

                            def verify(self, raw: str) -> bool:
                                return bool(self._normalize(raw))
                    "},
                    flagged_span: "_normalize",
                    fixed: indoc::indoc! {r"
                        class TokenVerifier:
                            def verify(self, raw: str) -> bool:
                                return bool(self._normalize(raw))

                            def _normalize(self, raw: str) -> str:
                                return raw.strip()
                    "},
                },
                Example {
                    language: Language::Rust,
                    flagged: indoc::indoc! {r"
                        fn trim_ascii(input: &str) -> &str {
                            input.trim()
                        }

                        pub fn parse_token(input: &str) -> bool {
                            !trim_ascii(input).is_empty()
                        }
                    "},
                    flagged_span: "trim_ascii",
                    fixed: indoc::indoc! {r"
                        pub fn parse_token(input: &str) -> bool {
                            !trim_ascii(input).is_empty()
                        }

                        fn trim_ascii(input: &str) -> &str {
                            input.trim()
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
        .private_before_public
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
                colocated_exclusive_helpers_between_public_entrypoints_allowed => r#"
                    def parse_header(raw: str) -> str:
                        return _clean_header(raw)

                    def _clean_header(raw: str) -> str:
                        return raw.strip()

                    def parse_body(raw: str) -> str:
                        return _clean_body(raw)

                    def _clean_body(raw: str) -> str:
                        return raw.lower()
                "#,
                shared_helper_below_all_its_public_callers_allowed => r#"
                    class Formatter:
                        def format_title(self, text: str) -> str:
                            return self._normalize(text)

                        def format_summary(self, text: str) -> str:
                            return self._normalize(text)

                        def _normalize(self, text: str) -> str:
                            return text.strip()
                "#,
                dunder_method_is_public => r#"
                    class Formatter:
                        def __repr__(self) -> str:
                            return "Formatter"

                        def format_title(self, text: str) -> str:
                            return text
                "#,
                private_only_scope_exempt => r#"
                    def _first_internal(value: int) -> int:
                        return _second_internal(value)

                    def _second_internal(value: int) -> int:
                        return value + 1
                "#,
                property_accessors_and_overloads_grouped_at_first_definition => r#"
                    from typing import overload

                    class Session:
                        @property
                        def token(self) -> str:
                            return self._sanitize()

                        @overload
                        def token(self, value: str) -> None: ...

                        @overload
                        def token(self, value: bytes) -> None: ...

                        @token.setter
                        def token(self, value: str | bytes) -> None:
                            pass

                        def _sanitize(self) -> str:
                            return "ok"
                "#,
            ],
            fail: [
                exclusive_helper_before_owning_public_function => r#"
                    def _prepare(value: int) -> int:
                        return value + 1

                    def compute(value: int) -> int:
                        return _prepare(value)
                "# => "_prepare",
                shared_helper_between_its_two_public_callers => r#"
                    class Verifier:
                        def verify_header(self, raw: str) -> bool:
                            return bool(self._strip(raw))

                        def _strip(self, raw: str) -> str:
                            return raw.strip()

                        def verify_footer(self, raw: str) -> bool:
                            return bool(self._strip(raw))
                "# => "_strip",
                uncalled_private_method_before_public_method => r#"
                    class Formatter:
                        def __trim(self) -> str:
                            return "x"

                        def __repr__(self) -> str:
                            return "Formatter"
                "# => "__trim",
            ],
        },
        Rust => {
            pass: [
                colocated_exclusive_helper_between_public_methods_allowed => r#"
                    pub struct SpanNode {
                        offset: usize,
                    }

                    impl SpanNode {
                        pub fn shifted(&self) -> usize {
                            self.compute_shift()
                        }

                        fn compute_shift(&self) -> usize {
                            self.offset + 1
                        }

                        pub fn raw_offset(&self) -> usize {
                            self.offset
                        }
                    }
                "#,
                restricted_visibility_is_public => r#"
                    pub struct SpanNode {
                        offset: usize,
                    }

                    impl SpanNode {
                        pub(crate) fn from_offset(offset: usize) -> Self {
                            Self { offset }
                        }

                        pub(super) fn raw_offset(&self) -> usize {
                            self.offset
                        }

                        pub fn shifted(&self) -> usize {
                            self.offset + 1
                        }
                    }
                "#,
                inline_test_fn_exempt => r#"
                    pub struct Worker;

                    impl Worker {
                        /// Test-only helper method.
                        #[cfg(test)]
                        fn test_only_helper(&self) {}

                        pub fn validate(&self) -> bool {
                            true
                        }
                    }
                "#,
            ],
            fail: [
                module_private_helper_before_pub_caller => r#"
                    fn normalize_key(key: &str) -> &str {
                        key.trim()
                    }

                    pub fn lookup_key(key: &str) -> usize {
                        normalize_key(key).len()
                    }
                "# => "normalize_key",
                shared_impl_helper_before_second_pub_caller => r#"
                    pub struct Parser;

                    impl Parser {
                        pub fn parse_left(&self, input: &str) -> usize {
                            Self::trimmed_len(input)
                        }

                        fn trimmed_len(input: &str) -> usize {
                            input.trim().len()
                        }

                        pub(crate) fn parse_right(&self, input: &str) -> usize {
                            Self::trimmed_len(input)
                        }
                    }
                "# => "trimmed_len",
                uncalled_private_method_before_pub_method => r#"
                    pub struct Tracker {
                        count: usize,
                    }

                    impl Tracker {
                        fn reset_internal(&mut self) {
                            self.count = 0;
                        }

                        pub fn total(&self) -> usize {
                            self.count
                        }
                    }
                "# => "reset_internal",
            ],
        },
    }
);
