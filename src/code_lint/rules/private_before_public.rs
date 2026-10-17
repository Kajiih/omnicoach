//! Flags private functions, methods, and types defined above their owning or calling public entrypoints.

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
    suggestion: "Move `{function}` below `{caller}`, for example below `{anchor}` (ends on line {line}).",
};

/// The rule's declaration.
pub const RULE: CodeRule = CodeRule {
    declaration: Declaration {
        name: RuleName("private-before-public"),
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
            summary: "Flags private functions, methods, and types defined above their owning or calling public entrypoints.",
            what_it_does: indoc::indoc! {r"
                Flags a private function, method, or type (`_name` in Python, or a non-`pub` `fn`,
                `struct`, `enum`, `union`, or `trait` in Rust) in a module, Python `class`, or Rust
                inherent `impl` block declared above a public declaration that references it, or,
                when no public declaration reaches it, above the last public one. In Python, items
                evaluated at import or class-definition time (such as top-level calls, decorators,
                parameter defaults, base classes, or runtime annotations without
                `from __future__ import annotations`) and their transitive callees are exempt when
                pinned above a consumer. Helpers below their callers are checked by
                `uncolocated-helper` and private-only call order by `callee-before-caller`, so each
                misplaced declaration is reported once."},
            why_is_this_bad: indoc::indoc! {r"
                A private helper function or type is a lower-abstraction building block than the
                public entrypoint that uses it, and a helper shared across multiple public
                entrypoints is at a lower abstraction level still. Placing private helpers above
                their public consumers inverts the abstraction hierarchy and forces readers to wade
                through internal mechanics before seeing the scope's public interface.

                Place single-use helpers either immediately below their owning public entrypoint or
                in the trailing private helper section, and place shared helpers in the trailing
                private helper section below all public declarations."},
            known_problems: Some(indoc::indoc! {r"
                - Module-level constants (`const`, `static`, `UPPER_CASE = ...`) and type aliases
                  are not ordered.
                - In Rust macro arguments, a pattern binding or named argument that shares a sibling
                  declaration's name counts as a reference."}),
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
            let line = finding.anchor.line.to_string();
            rule.diagnostic_at_node(
                path,
                &finding.name_node,
                &[
                    ("function", &finding.function),
                    ("caller", &finding.caller),
                    ("anchor", &finding.anchor.name),
                    ("line", &line),
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
                property_accessors_grouped_at_first_definition => r#"
                    from typing import overload

                    class Session:
                        @property
                        def token(self) -> str:
                            return "ok"

                        def _reset(self) -> None:
                            pass

                        @overload
                        def token(self, value: str) -> None: ...

                        @overload
                        def token(self, value: bytes) -> None: ...

                        @token.setter
                        def token(self, value: str | bytes) -> None:
                            pass
                "#,
                import_time_pinned_helper_exempt => r#"
                    def _build_table() -> dict[str, int]:
                        return {"a": _default_weight()}

                    def _default_weight() -> int:
                        return 1

                    TABLE = _build_table()

                    def lookup(key: str) -> int:
                        return TABLE.get(key, _default_weight())
                "#,
                decorator_pinned_helper_exempt => r#"
                    def _trace(fn):
                        return fn

                    @_trace
                    def execute(value: int) -> int:
                        return value
                "#,
                base_class_pinned_before_subclass_exempt => r#"
                    class _BaseClient:
                        def _send(self) -> int:
                            return 1

                    class HttpClient(_BaseClient):
                        def fetch(self) -> int:
                            return self._send()
                "#,
                runtime_annotation_pinned_without_future_annotations => r#"
                    class _Config:
                        pass

                    def configure(cfg: _Config) -> None:
                        pass
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
                private_class_before_public_class_using_it => r#"
                    class _Entry:
                        pass

                    class Cache:
                        def create(self) -> object:
                            return _Entry()
                "# => "_Entry",
                future_annotations_unpins_private_annotation_type => r#"
                    from __future__ import annotations

                    class _Config:
                        pass

                    def configure(cfg: _Config) -> None:
                        pass
                "# => "_Config",
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
                private_helper_after_pub_struct_impl_allowed => r#"
                    pub struct Service;

                    impl Service {
                        pub fn execute(&self) -> usize {
                            compute_step()
                        }
                    }

                    fn compute_step() -> usize {
                        1
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
                private_struct_before_pub_struct_using_it => r#"
                    struct RawEntry {
                        count: usize,
                    }

                    pub struct Table {
                        entry: RawEntry,
                    }
                "# => "RawEntry",
                private_fn_before_pub_struct_impl_caller => r#"
                    fn compute_hash(input: &str) -> usize {
                        input.len()
                    }

                    pub struct Hasher;

                    impl Hasher {
                        pub fn hash(&self, input: &str) -> usize {
                            compute_hash(input)
                        }
                    }
                "# => "compute_hash",
            ],
        },
    }
);
