//! Flags private helper methods defined before public methods in a class or inherent `impl` block.

use crate::code_lint::ast::{self, MethodVisibility, ParsedFile, TypeMethod, TypeMethodScope};
use crate::code_lint::contract::{CodeRule, RuleTarget};
use crate::diagnostic::{Diagnostic, Language, RuleName, ViolationTemplate, violation_template};
use crate::rule_declaration::{
    Classification, Consensus, Declaration, Example, ImpactedQuality, Precision, Reference,
    RuleDoc, RuleOptions, Topic,
};
use std::path::Path;

const TEMPLATE: ViolationTemplate = violation_template! {
    summary: "Private method `{function}` of `{class}` is defined before a public method.",
    rationale: "Placing internal helpers above public methods buries the external interface of `{class}` behind implementation details.",
    suggestion: {
        base: "Move `{function}` below all public methods in `{class}`.",
        Python => "Move `{function}` below all public and dunder methods in `{class}`.",
        Rust => "Move `{function}` below all `pub` methods in the `impl {class}` block.",
    },
};

/// The rule's declaration.
pub const RULE: CodeRule = CodeRule {
    declaration: Declaration {
        name: RuleName("private-before-public-method"),
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
            summary: "Flags private helper methods defined before public methods in a class or inherent `impl` block.",
            what_it_does: indoc::indoc! {r"
                Checks Python `class` definitions and Rust inherent `impl` blocks for private
                helper methods defined before a public or exported method in the same scope.

                In Python, public methods (`name`) and special dunder methods (`__name__`) belong
                to the public contract tier, while single-underscore (`_name`) and name-mangled
                (`__name`) methods belong to the private tier. `@overload` signatures and
                `@<name>.setter` / `@<name>.deleter` property accessors are grouped with their
                first definition.

                In Rust inherent `impl` blocks, any method or associated function carrying a
                visibility qualifier (`pub`, `pub(crate)`, `pub(super)`, `pub(in ...)`) belongs to
                the exported tier, while bare `fn` items belong to the private tier. Trait
                implementations (`impl Trait for Type`), free functions at module level, test
                files, and `#[cfg(test)]` / `#[test]` items are not checked."},
            why_is_this_bad: indoc::indoc! {r"
                Readers inspect a class or `impl` block from the top down to discover its public
                interface before diving into internal helpers. Interleaving private helper methods
                above or between public entrypoints forces readers to sift through internal
                mechanics to assemble the type's public contract.

                Keep all public and exported methods in the upper section of the `class` or
                inherent `impl` block, and place private helper methods below them."},
            references: &[
                Reference {
                    title: "Robert C. Martin: Clean Code — Chapter 10: Classes (Class Organization / Stepdown Rule)",
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
                        pub struct HeaderParser;

                        impl HeaderParser {
                            fn trim_ascii(input: &str) -> &str {
                                input.trim()
                            }

                            pub fn parse(&self, input: &str) -> bool {
                                !Self::trim_ascii(input).is_empty()
                            }
                        }
                    "},
                    flagged_span: "trim_ascii",
                    fixed: indoc::indoc! {r"
                        pub struct HeaderParser;

                        impl HeaderParser {
                            pub fn parse(&self, input: &str) -> bool {
                                !Self::trim_ascii(input).is_empty()
                            }

                            fn trim_ascii(input: &str) -> &str {
                                input.trim()
                            }
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
    ast::collect_type_method_scopes(file)
        .iter()
        .flat_map(private_methods_before_public)
        .map(|(type_name, method)| {
            rule.diagnostic_at_node(
                path,
                &method.name_node,
                &[("function", &method.name), ("class", type_name)],
            )
        })
        .collect()
}

/// Returns every private method in `scope` that appears before the last public method.
fn private_methods_before_public<'a>(
    scope: &'a TypeMethodScope<'a>,
) -> Vec<(&'a str, &'a TypeMethod<'a>)> {
    let Some(last_public_index) = scope
        .methods
        .iter()
        .rposition(|method| method.visibility == MethodVisibility::Public)
    else {
        return Vec::new();
    };
    scope.methods[..last_public_index]
        .iter()
        .filter(|method| method.visibility == MethodVisibility::Private)
        .map(|method| (scope.type_name.as_str(), method))
        .collect()
}

#[cfg(test)]
crate::test_utils::rule_test!(
    RULE,
    {
        Python => {
            pass: [
                public_and_dunder_before_private => r#"
                    class Connection:
                        def __init__(self, host: str) -> None:
                            self.host = host

                        def connect(self) -> str:
                            return self._resolve_host()

                        def __repr__(self) -> str:
                            return self.host

                        def _resolve_host(self) -> str:
                            return self.__sanitize()

                        def __sanitize(self) -> str:
                            return self.host.strip()
                "#,
                public_property_setter_after_private_helper_exempt => r#"
                    from typing import overload

                    class Session:
                        def __init__(self, token: str) -> None:
                            self._raw_token = token

                        @property
                        def token(self) -> str:
                            return self._normalize_token()

                        def _normalize_token(self) -> str:
                            return self._raw_token.strip()

                        @overload
                        def token(self, value: str) -> None: ...

                        @overload
                        def token(self, value: bytes) -> None: ...

                        @token.setter
                        def token(self, value: str | bytes) -> None:
                            self._raw_token = str(value)
                "#,
                module_functions_and_nested_classes_isolated => r#"
                    def _module_helper(value: int) -> int:
                        return value + 1

                    def public_entrypoint(value: int) -> int:
                        return _module_helper(value)

                    class Outer:
                        def execute(self) -> None:
                            pass

                        def _outer_helper(self) -> None:
                            pass

                        class Inner:
                            def run_inner(self) -> None:
                                pass

                            def _inner_helper(self) -> None:
                                pass
                "#,
            ],
            fail: [
                private_method_before_public_method => r#"
                    class Verifier:
                        def _prepare(self) -> int:
                            return 1

                        def verify(self) -> int:
                            return self._prepare()
                "# => "_prepare",
                mangled_method_before_dunder_method => r#"
                    class Formatter:
                        def __init__(self, label: str) -> None:
                            self.label = label

                        def __trim(self) -> str:
                            return self.label.strip()

                        def __repr__(self) -> str:
                            return self.__trim()
                "# => "__trim",
                private_property_grouped_once_before_public_method => r#"
                    class Credential:
                        @property
                        def _secret(self) -> str:
                            return "a"

                        @_secret.setter
                        def _secret(self, value: str) -> None:
                            pass

                        def is_valid(self) -> bool:
                            return bool(self._secret)
                "# => "_secret",
            ],
        },
        Rust => {
            pass: [
                exported_tiers_before_private_methods => r#"
                    pub struct SpanNode {
                        offset: usize,
                    }

                    impl SpanNode {
                        pub(crate) fn from_offset(offset: usize) -> Self {
                            Self { offset }
                        }

                        pub fn offset(&self) -> usize {
                            self.offset
                        }

                        pub(super) fn shifted(&self) -> usize {
                            self.compute_shift()
                        }

                        fn compute_shift(&self) -> usize {
                            self.offset + 1
                        }
                    }
                "#,
                trait_impl_and_separate_impl_blocks_exempt => r#"
                    pub trait Handler {
                        fn handle(&self);
                    }

                    pub struct Worker;

                    impl Handler for Worker {
                        fn handle(&self) {}
                    }

                    impl Worker {
                        fn helper_in_first_block(&self) {}
                    }

                    impl Worker {
                        pub fn public_in_second_block(&self) {}
                    }
                "#,
                inline_test_before_pub_method_exempt => r#"
                    pub struct Validator;

                    impl Validator {
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
                private_helper_before_pub_method => r#"
                    pub struct Tracker {
                        count: usize,
                    }

                    impl Tracker {
                        pub fn new() -> Self {
                            Self { count: 0 }
                        }

                        fn reset_internal(&mut self) {
                            self.count = 0;
                        }

                        pub fn total(&self) -> usize {
                            self.count
                        }
                    }
                "# => "reset_internal",
                private_method_before_pub_crate_method => r#"
                    pub struct Literal {
                        bits: u64,
                    }

                    impl Literal {
                        fn from_bits(bits: u64) -> Self {
                            Self { bits }
                        }

                        pub(crate) fn inverted(&self) -> Self {
                            Self::from_bits(self.bits)
                        }
                    }
                "# => "from_bits",
            ],
        },
    }
);
