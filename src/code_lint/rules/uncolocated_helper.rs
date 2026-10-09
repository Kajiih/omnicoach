//! Flags private helpers that are neither colocated right after their single public consumer nor
//! placed in the trailing helper section at the end of the scope.

use crate::code_lint::ast::{self, ParsedFile};
use crate::code_lint::contract::{CodeRule, RuleTarget};
use crate::diagnostic::{Diagnostic, Language, RuleName, ViolationTemplate, violation_template};
use crate::rule_declaration::{
    Classification, Consensus, Declaration, Example, ImpactedQuality, Precision, Reference,
    RuleDoc, RuleOptions, Topic,
};
use std::path::Path;

const TEMPLATE: ViolationTemplate = violation_template! {
    summary: "Private helper `{function}` is separated from the helper cluster of `{caller}`.",
    rationale: "Stranding `{function}` between unrelated public functions or splitting the helpers of `{caller}` between the inline cluster and the trailing helper section fractures the layout of the scope.",
    suggestion: "Move `{function}` either immediately below `{caller}` (if single-use) or together with the helpers of `{caller}` into the trailing private helper section after all public functions.",
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
            summary: "Flags private helpers that are neither colocated right after their single public consumer nor placed in the trailing helper section at the end of the scope.",
            what_it_does: indoc::indoc! {r"
                Checks module scopes, Python `class` definitions, and Rust inherent `impl` blocks
                that every private helper `h` (once placed after its public callers) occupies one of
                only **two valid places**:
                1. **Immediately after its consumer** (for a single-user helper, `Roots(h) = {p}`):
                   inside the contiguous helper cluster `[p, _h1, _h2, ...]` directly below `p`.
                2. **At the end of the scope** (for a single-user or multi-user helper,
                   `|Roots(h)| >= 1`): in the trailing private helper section after all public
                   functions or methods in the scope (`pos(h) > last_pub`), provided a single-user
                   helper does not split its owner's helpers between both places.

                This supports both vertical-slice modules (`[pub_a, _a_helpers, pub_b, _b_helpers]`)
                and public-first scopes (`[pub_a, pub_b, _a_helpers, _b_helpers, _shared_helpers]`),
                while flagging helpers stranded in the middle of a scope between unrelated public
                functions. Uncalled private functions (`Roots(h) = empty`) and helpers declared
                above a public caller are handled by `private-before-public-function`. Python
                `@overload` signatures and `@property` accessors (`getter`, `setter`, `deleter`)
                are grouped at their first definition; Rust trait `impl` blocks, test files, and
                `#[cfg(test)]` / `#[test]` items are not checked."},
            why_is_this_bad: indoc::indoc! {r"
                A private helper belongs either directly underneath the single public entrypoint it
                implements (vertical slice) or in the trailing private implementation section at the
                bottom of the scope. Stranding a single-use or shared helper in the middle of a
                scope between unrelated public functions forces readers to jump over internal
                helpers while scanning the public API and splits related helpers apart.

                Place each single-use helper either immediately after its consumer or in the
                trailing helper section at the end of the scope, and always place multi-use helpers
                at the end of the scope."},
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

                        def parse_body(raw: str) -> str:
                            return raw.strip()

                        def _strip_prefix(raw: str) -> str:
                            return raw.removeprefix('H:')

                        def parse_footer(raw: str) -> str:
                            return raw.rstrip()
                    "},
                    flagged_span: "_strip_prefix",
                    fixed: indoc::indoc! {r"
                        def parse_header(raw: str) -> str:
                            return _strip_prefix(raw)

                        def _strip_prefix(raw: str) -> str:
                            return raw.removeprefix('H:')

                        def parse_body(raw: str) -> str:
                            return raw.strip()

                        def parse_footer(raw: str) -> str:
                            return raw.rstrip()
                    "},
                },
                Example {
                    language: Language::Rust,
                    flagged: indoc::indoc! {r"
                        pub struct PacketDecoder;

                        impl PacketDecoder {
                            pub fn decode_header(&self, raw: &str) -> bool {
                                Self::is_non_empty(raw)
                            }

                            pub fn decode_body(&self, raw: &str) -> bool {
                                Self::is_non_empty(raw)
                            }

                            fn is_non_empty(raw: &str) -> bool {
                                !raw.is_empty()
                            }

                            pub fn decode_footer(&self, raw: &str) -> usize {
                                raw.len()
                            }
                        }
                    "},
                    flagged_span: "is_non_empty",
                    fixed: indoc::indoc! {r"
                        pub struct PacketDecoder;

                        impl PacketDecoder {
                            pub fn decode_header(&self, raw: &str) -> bool {
                                Self::is_non_empty(raw)
                            }

                            pub fn decode_body(&self, raw: &str) -> bool {
                                Self::is_non_empty(raw)
                            }

                            pub fn decode_footer(&self, raw: &str) -> usize {
                                raw.len()
                            }

                            fn is_non_empty(raw: &str) -> bool {
                                !raw.is_empty()
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
                        return build_header(_normalize_token(raw))

                    def _normalize_token(raw: str) -> str:
                        return raw.strip()
                "#,
                all_private_helpers_at_end_of_scope => r#"
                    class Compiler:
                        def compile_all(self, source: str) -> str:
                            return self._prepare_batch(source)

                        def compile_unit(self, source: str) -> str:
                            return self._emit_bytecode(source)

                        def _prepare_batch(self, source: str) -> str:
                            return source.lstrip()

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

                    class Pool:
                        def __new__(cls, limit: int) -> "Pool":
                            cls._check_limit(limit)
                            return super().__new__(cls)

                        def __init__(self, limit: int) -> None:
                            self.limit = limit

                        def acquire(self) -> int:
                            return self.limit

                        @staticmethod
                        def _check_limit(limit: int) -> bool:
                            return limit > 0
                "#,
                class_method_local_binding_does_not_bridge_module_helper => r#"
                    class Formatter:
                        def render(self, _strip_header: str) -> str:
                            return _strip_header

                    def format_header(raw: str) -> str:
                        return _strip_header(raw)

                    def _strip_header(raw: str) -> str:
                        return raw.strip()

                    def format_with_formatter(raw: str) -> str:
                        return Formatter().render(raw)
                "#,
            ],
            fail: [
                single_use_helper_stranded_in_middle_of_module => r#"
                    def render_title(text: str) -> str:
                        return _clean_title(text)

                    def render_body(text: str) -> str:
                        return text

                    def _clean_title(text: str) -> str:
                        return text.strip()

                    def render_footer(text: str) -> str:
                        return text.rstrip()
                "# => "_clean_title",
                shared_helper_stranded_before_trailing_public_method => r#"
                    class Tracker:
                        def load(self, text: str) -> str:
                            return self._parse_line(text)

                        def reload(self, text: str) -> str:
                            return self._parse_line(text)

                        def _parse_line(self, text: str) -> str:
                            return text.strip()

                        def clear(self) -> None:
                            pass
                "# => "_parse_line",
                exclusive_helper_cluster_split_between_both_places => r#"
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
                inherent_impl_both_valid_places_and_trait_impl_exempt => r#"
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
                        pub fn new() -> Self {
                            Self::validate_seed();
                            Self
                        }

                        pub fn with_capacity() -> Self {
                            Self
                        }

                        pub fn parse_first(&self, raw: &str) -> bool {
                            Self::check_first(raw)
                        }

                        #[cfg(test)]
                        fn test_only_helper() {}

                        pub fn parse_second(&self, raw: &str) -> usize {
                            raw.len()
                        }

                        fn validate_seed() {}

                        fn check_first(raw: &str) -> bool {
                            !raw.is_empty()
                        }
                    }
                "#,
            ],
            fail: [
                single_use_helper_stranded_in_middle_of_module => r#"
                    pub fn encode_key(key: &str) -> usize {
                        hash_key(key)
                    }

                    pub fn encode_value(value: &str) -> usize {
                        value.len()
                    }

                    fn hash_key(key: &str) -> usize {
                        key.len()
                    }

                    pub fn encode_footer(footer: &str) -> usize {
                        footer.len()
                    }
                "# => "hash_key",
                shared_helper_stranded_before_last_pub_method => r#"
                    pub struct Router;

                    impl Router {
                        pub fn route_get(&self, path: &str) -> bool {
                            self.is_valid_path(path)
                        }

                        pub fn route_post(&self, path: &str) -> bool {
                            self.is_valid_path(path)
                        }

                        fn is_valid_path(&self, path: &str) -> bool {
                            path.starts_with('/')
                        }

                        pub fn route_delete(&self, path: &str) -> bool {
                            !path.is_empty()
                        }
                    }
                "# => "is_valid_path",
            ],
        },
    }
);
