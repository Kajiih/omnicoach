//! Flags exclusive private helpers separated from their owning public function or method by
//! unrelated definitions.

use crate::code_lint::ast::{self, ParsedFile};
use crate::code_lint::contract::{CodeRule, RuleTarget};
use crate::diagnostic::{Diagnostic, Language, RuleName, ViolationTemplate, violation_template};
use crate::rule_declaration::{
    Classification, Consensus, Declaration, Example, ImpactedQuality, Precision, Reference,
    RuleDoc, RuleOptions, Topic,
};
use std::path::Path;

const TEMPLATE: ViolationTemplate = violation_template! {
    summary: "Exclusive helper `{function}` is separated from its owning public entrypoint `{caller}` by an unrelated function.",
    rationale: "Splitting `{function}` away from `{caller}` fractures the component unit of `{caller}` across the scope and forces readers to jump over unrelated definitions.",
    suggestion: "Move `{function}` into the contiguous helper cluster immediately below `{caller}`.",
};

/// The rule's declaration.
pub const RULE: CodeRule = CodeRule {
    declaration: Declaration {
        name: RuleName("uncolocated-helper"),
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
            summary: "Flags exclusive private helpers separated from their owning public function or method by unrelated definitions.",
            what_it_does: indoc::indoc! {r"
                Checks module scopes, Python `class` definitions, and Rust inherent `impl` blocks
                for exclusive private helpers separated from their owning public entrypoint by a
                function or method from another component unit or the shared helper layer.

                For each private callable `h`, the call-cluster analyzer computes `Roots(h)`: the
                set of public entrypoints in the same scope that reach `h` through private call
                paths (stopping at public boundaries). When `Roots(h)` contains a single public
                entrypoint `p`, `h` is an **exclusive helper** of `p` and belongs to the
                contiguous component unit `[p, _h1, _h2, ...]` immediately below `p`.

                Shared private helpers reachable from two or more public entrypoints
                (`|Roots(h)| >= 2`) belong to a lower abstraction layer at the end of the scope and
                are exempt. Together with `private-before-public-function` (Priority 2) and
                `callee-before-caller` (Priority 3), this rule forms a three-tier precedence system
                where `uncolocated-helper` runs at Priority 1."},
            why_is_this_bad: indoc::indoc! {r"
                A public entrypoint and its exclusive private helpers form an in-file component
                unit. Scattering an exclusive helper below unrelated public functions or shared
                utilities breaks locality of behavior and forces readers to scroll across unrelated
                abstractions to trace a single feature.

                Keep every exclusive private helper contiguous with its owning public entrypoint,
                and reserve the bottom of the scope for helpers shared by multiple entrypoints."},
            references: &[
                Reference {
                    title: "Robert C. Martin: Clean Code — Chapter 5: Formatting (Vertical Distance & Dependent Functions)",
                    url: "https://www.oreilly.com/library/view/clean-code-a/9780136083238/",
                },
                Reference {
                    title: "Hitz & Montazeri (1995): Measuring Coupling and Cohesion in Object-Oriented Systems (LCOM4)",
                    url: "https://www.researchgate.net/publication/242402032_Measuring_Coupling_and_Cohesion_in_Object-Oriented_Systems",
                },
            ],
            examples: &[
                Example {
                    language: Language::Python,
                    flagged: indoc::indoc! {r"
                        def parse_header(raw: str) -> str:
                            return _strip_prefix(raw)

                        def parse_footer(raw: str) -> str:
                            return raw.strip()

                        def _strip_prefix(raw: str) -> str:
                            return raw.removeprefix('H:')
                    "},
                    flagged_span: "_strip_prefix",
                    fixed: indoc::indoc! {r"
                        def parse_header(raw: str) -> str:
                            return _strip_prefix(raw)

                        def _strip_prefix(raw: str) -> str:
                            return raw.removeprefix('H:')

                        def parse_footer(raw: str) -> str:
                            return raw.strip()
                    "},
                },
                Example {
                    language: Language::Rust,
                    flagged: indoc::indoc! {r"
                        pub struct PacketDecoder;

                        impl PacketDecoder {
                            pub fn decode_header(&self, raw: &str) -> bool {
                                Self::has_magic_prefix(raw)
                            }

                            pub fn decode_payload(&self, raw: &str) -> usize {
                                raw.len()
                            }

                            fn has_magic_prefix(raw: &str) -> bool {
                                raw.starts_with(':')
                            }
                        }
                    "},
                    flagged_span: "has_magic_prefix",
                    fixed: indoc::indoc! {r"
                        pub struct PacketDecoder;

                        impl PacketDecoder {
                            pub fn decode_header(&self, raw: &str) -> bool {
                                Self::has_magic_prefix(raw)
                            }

                            fn has_magic_prefix(raw: &str) -> bool {
                                raw.starts_with(':')
                            }

                            pub fn decode_payload(&self, raw: &str) -> usize {
                                raw.len()
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
    ast::collect_call_cluster_findings(file)
        .uncolocated_helpers
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
                colocated_exclusive_clusters_and_shared_footer => r#"
                    def build_header(raw: str) -> str:
                        return _format_header(_normalize_token(raw))

                    def _format_header(token: str) -> str:
                        return _wrap_brackets(token)

                    def _wrap_brackets(token: str) -> str:
                        return f"[{token}]"

                    def build_footer(raw: str) -> str:
                        return _normalize_token(raw)

                    def _normalize_token(raw: str) -> str:
                        return raw.strip()
                "#,
                public_calling_public_keeps_helper_exclusive => r#"
                    class Compiler:
                        def compile_all(self, source: str) -> str:
                            return self.compile_unit(source)

                        def compile_unit(self, source: str) -> str:
                            return self._emit_bytecode(source)

                        def _emit_bytecode(self, source: str) -> str:
                            return source.strip()
                "#,
                uncalled_private_and_constructor_clusters_exempt => r#"
                    class Session:
                        def __new__(cls, host: str) -> "Session":
                            return cls._allocate()

                        def __init__(self, host: str) -> None:
                            self.host = host

                        @classmethod
                        def _allocate(cls) -> "Session":
                            return super().__new__(cls)

                        def connect(self) -> str:
                            return self.host

                        def _unused_hook(self) -> None:
                            pass
                "#,
            ],
            fail: [
                module_exclusive_helper_separated_by_second_public => r#"
                    def render_title(text: str) -> str:
                        return _clean_title(text)

                    def render_body(text: str) -> str:
                        return text

                    def _clean_title(text: str) -> str:
                        return text.strip()
                "# => "_clean_title",
                class_exclusive_helper_separated_by_sibling_method => r#"
                    class Tracker:
                        def load(self, text: str) -> str:
                            return self._parse_line(text)

                        def clear(self) -> None:
                            pass

                        def _parse_line(self, text: str) -> str:
                            return text.strip()
                "# => "_parse_line",
                transitive_exclusive_helper_separated_by_shared_helper => r#"
                    def export_json(raw: str) -> str:
                        return _serialize_json(_trim_input(raw))

                    def _serialize_json(clean: str) -> str:
                        return _quote_json(clean)

                    def export_yaml(raw: str) -> str:
                        return _trim_input(raw)

                    def _trim_input(raw: str) -> str:
                        return raw.strip()

                    def _quote_json(clean: str) -> str:
                        return f'"{clean}"'
                "# => "_quote_json",
            ],
        },
        Rust => {
            pass: [
                colocated_units_and_shared_helper_at_end => r#"
                    pub fn parse_header(input: &str) -> &str {
                        strip_header_prefix(trim_ascii(input))
                    }

                    fn strip_header_prefix(input: &str) -> &str {
                        input.trim_start_matches("H:")
                    }

                    pub fn parse_footer(input: &str) -> &str {
                        trim_ascii(input)
                    }

                    fn trim_ascii(input: &str) -> &str {
                        input.trim()
                    }
                "#,
                inherent_impl_colocated_clusters_and_trait_impl_exempt => r#"
                    pub trait Decoder {
                        fn decode(&self, raw: &str) -> usize;
                    }

                    pub struct FrameParser;

                    impl Decoder for FrameParser {
                        fn decode(&self, raw: &str) -> usize {
                            raw.len()
                        }
                    }

                    impl FrameParser {
                        pub fn parse_first(&self, raw: &str) -> bool {
                            Self::check_first(raw)
                        }

                        fn check_first(raw: &str) -> bool {
                            !raw.is_empty()
                        }

                        pub fn parse_second(&self, raw: &str) -> usize {
                            raw.len()
                        }
                    }
                "#,
            ],
            fail: [
                module_exclusive_helper_after_unrelated_pub_fn => r#"
                    pub fn encode_key(key: &str) -> usize {
                        hash_key(key)
                    }

                    pub fn encode_value(value: &str) -> usize {
                        value.len()
                    }

                    fn hash_key(key: &str) -> usize {
                        key.len()
                    }
                "# => "hash_key",
                impl_exclusive_helper_separated_by_other_pub_method => r#"
                    pub struct Router;

                    impl Router {
                        pub fn route_get(&self, path: &str) -> bool {
                            self.is_get_path(path)
                        }

                        pub fn route_post(&self, path: &str) -> bool {
                            !path.is_empty()
                        }

                        fn is_get_path(&self, path: &str) -> bool {
                            path.starts_with('/')
                        }
                    }
                "# => "is_get_path",
            ],
        },
    }
);
