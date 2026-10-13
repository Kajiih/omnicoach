//! Flags functions that return or yield a positional tuple instead of a named record.
//!
//! Design: `docs/dev/return_contracts/03_design.md`.

use crate::code_lint::ast::{self, ParsedFile, ReturnShape};
use crate::code_lint::contract::{CodeRule, RuleTarget};
use crate::diagnostic::{Diagnostic, Language, RuleName, ViolationTemplate, violation_template};
use crate::rule_declaration::{
    Classification, Consensus, CountOption, Declaration, Example, ImpactedQuality,
    LanguageDefaults, Precision, Reference, RuleDoc, RuleOptions, Topic,
};
use std::path::Path;

const MIN_ELEMENTS: CountOption = CountOption {
    key: "min-elements",
    doc: "Minimum elements in a returned tuple for it to be flagged.",
    default: LanguageDefaults::new(2, &[]),
};

const TEMPLATE: ViolationTemplate = violation_template! {
    summary: "Function `{function}` returns a positional tuple of {count} elements.",
    rationale: "A tuple names none of its elements, so every caller depends on their order, and swapping two elements of the same type raises no error.",
    suggestion: {
        Python => "Replace the tuple returned by `{function}` with a `NamedTuple` or a frozen dataclass whose fields name each element.",
        Rust => "Replace the tuple returned by `{function}` with a struct whose fields name each element.",
    },
};

/// The rule's declaration.
pub const RULE: CodeRule<CountOption> = CodeRule {
    declaration: Declaration {
        name: RuleName("positional-tuple-return"),
        template: &TEMPLATE,
        languages: &[Language::Python, Language::Rust],
        options: RuleOptions::code_rule(MIN_ELEMENTS),
        classification: Classification {
            topics: &[Topic::POSITIONAL_MEANING],
            precision: Precision::Exact,
            consensus: Consensus::Opinionated,
            impacted_quality: ImpactedQuality::Maintainability,
        },
        doc: RuleDoc {
            summary: "Flags functions that return a positional tuple instead of a named record.",
            what_it_does: indoc::indoc! {r"
                Flags a function or method whose return type is a fixed-length tuple of at least
                `min-elements` elements: `tuple[str, int]` in Python or `(String, u16)` in Rust.
                The tuple is also found inside an optional (`| None`, `Option`), a union, a Rust
                `Result` / `Poll` or smart pointer, and a Python generator's yield type
                (`Iterator[tuple[int, str]]`). An unannotated Python function is flagged when its
                own body returns or yields a tuple display such as `return low, high`.

                A homogeneous `tuple[int, ...]` and a tuple inside a collection
                (`list[tuple[int, str]]`, `Vec<(u32, u32)>`) are not flagged. Signatures imposed
                from outside are skipped: Python `@override`, `@overload`, `@abstractmethod`,
                data-model dunders and `Protocol` / `ABC` members, and Rust trait items and trait
                implementations."},
            why_is_this_bad: indoc::indoc! {r"
                A tuple's type states what each element is, not what it means: `tuple[str, str]`
                and `(PathBuf, PathBuf)` leave callers to remember which position is which. Every
                call site unpacks by position, so swapping two elements of the same type compiles
                and type-checks, and adding an element breaks every caller.

                A `NamedTuple` or frozen dataclass (Python) or a struct (Rust) puts the field names
                in the signature, where callers, readers and type checkers see them. A Python
                `NamedTuple` still unpacks like a tuple, so existing callers keep working."},
            known_problems: Some(indoc::indoc! {r"
                - Type aliases (`Pair = tuple[int, int]`, `type Pair = (u32, u32);`) are not
                  expanded, and Rust `impl Iterator<Item = (A, B)>` is not read.
                - A symmetric pair whose order is its meaning, such as the two halves returned by
                  `split_once`, is flagged too."}),
            references: &[
                Reference {
                    title: "Effective Python, Item 19: Never unpack more than three variables when functions return multiple values",
                    url: "https://effectivepython.com/",
                },
                Reference {
                    title: "Python documentation: `typing.NamedTuple`",
                    url: "https://docs.python.org/3/library/typing.html#typing.NamedTuple",
                },
                Reference {
                    title: "The Rust Programming Language: Defining and Instantiating Structs",
                    url: "https://doc.rust-lang.org/book/ch05-01-defining-structs.html",
                },
                Reference::NAME_RESOLUTION,
            ],
            examples: &[
                Example {
                    language: Language::Python,
                    flagged: indoc::indoc! {r#"
                        def split_host_port(address: str) -> tuple[str, int]:
                            host, port = address.rsplit(":", 1)
                            return host, int(port)
                    "#},
                    flagged_span: "split_host_port",
                    fixed: indoc::indoc! {r#"
                        from typing import NamedTuple

                        class HostPort(NamedTuple):
                            host: str
                            port: int

                        def split_host_port(address: str) -> HostPort:
                            host, port = address.rsplit(":", 1)
                            return HostPort(host, int(port))
                    "#},
                },
                Example {
                    language: Language::Rust,
                    flagged: indoc::indoc! {r"
                        fn split_host_port(address: &str) -> Option<(&str, u16)> {
                            let (host, port) = address.rsplit_once(':')?;
                            Some((host, port.parse().ok()?))
                        }
                    "},
                    flagged_span: "split_host_port",
                    fixed: indoc::indoc! {r"
                        struct HostPort<'a> {
                            host: &'a str,
                            port: u16,
                        }

                        fn split_host_port(address: &str) -> Option<HostPort<'_>> {
                            let (host, port) = address.rsplit_once(':')?;
                            Some(HostPort { host, port: port.parse().ok()? })
                        }
                    "},
                },
            ],
        },
    },
    target: RuleTarget::SourceOnly,
    check: check_file,
};

const FUNCTION: &str = "function";
const COUNT: &str = "count";

fn check_file(
    rule: &CodeRule<CountOption>,
    path: &Path,
    file: &ParsedFile,
    min_elements: usize,
) -> Vec<Diagnostic> {
    ast::collect_function_returns(file)
        .into_iter()
        .filter_map(|function| match function.shape {
            ReturnShape::PositionalTuple { arity } if arity >= min_elements => {
                Some(rule.diagnostic_at_node(
                    path,
                    &function.name_node,
                    &[(FUNCTION, &function.name), (COUNT, &arity.to_string())],
                ))
            }
            _ => None,
        })
        .collect()
}

#[cfg(test)]
crate::test_utils::rule_test!(
    RULE,
    {
        Python => {
            pass: [
                named_record_returns => r#"
                    from typing import NamedTuple

                    class HostPort(NamedTuple):
                        host: str
                        port: int

                    def split_host_port(address: str) -> HostPort:
                        host, port = address.rsplit(":", 1)
                        return HostPort(host, int(port))

                    def list_ports(addresses: list[str]) -> list[int]:
                        return [split_host_port(address).port for address in addresses]
                "#,
                variadic_or_single_element_tuple_exempt => r"
                    def freeze(items: list[int]) -> tuple[int, ...]:
                        return tuple(items)

                    def wrap(item: int) -> tuple[int]:
                        return (item,)
                ",
                tuple_inside_collection_exempt => r"
                    def pairs(items: list[str]) -> list[tuple[int, str]]:
                        return list(enumerate(items))
                ",
                imposed_signature_exempt => r"
                    from typing import override

                    class Parser(BaseParser):
                        @override
                        def parse(self, raw: str) -> tuple[str, int]:
                            return raw, 0

                        def __divmod__(self, other: int) -> tuple[int, int]:
                            return 0, 0
                ",
                nested_scope_tuple_display_exempt => r"
                    from typing import override

                    def make_splitters(separator):
                        @override
                        def split(raw):
                            return raw.split(separator)[0], 80
                        key = lambda pair: (pair[0], pair[1])
                        return [split, key]
                ",
                starred_tuple_display_exempt => r"
                    def prepend(head, rest):
                        return head, *rest
                ",
            ],
            fail: [
                annotated_tuple_return => r"
                    def split_host_port(address: str) -> tuple[str, int]:
                        return address, 80
                " => "split_host_port",
                typing_tuple_in_optional => r"
                    from typing import Optional, Tuple

                    def parse_header(line: str) -> Optional[Tuple[str, str]]:
                        return None
                " => "parse_header",
                async_tuple_return => r"
                    async def fetch_page(url: str) -> tuple[bytes, int]:
                        return b'', 200
                " => "fetch_page",
                yielded_tuple => r"
                    from collections.abc import Iterator

                    def iter_indexed(items: list[str]) -> Iterator[tuple[int, str]]:
                        yield from enumerate(items)
                " => "iter_indexed",
                unannotated_tuple_display_return => r"
                    def bounds(values):
                        return min(values), max(values)
                " => "bounds",
                unannotated_tuple_display_yield => r"
                    def iter_indexed(items):
                        for index, item in enumerate(items):
                            yield index, item
                " => "iter_indexed",
                method_tuple_return => r"
                    class Grid:
                        def size(self) -> tuple[int, int]:
                            return 3, 4
                " => "size",
            ],
        },
        Rust => {
            pass: [
                named_record_returns => r"
                    struct HostPort<'a> {
                        host: &'a str,
                        port: u16,
                    }

                    fn split_host_port(address: &str) -> Option<HostPort<'_>> {
                        let (host, port) = address.rsplit_once(':')?;
                        Some(HostPort { host, port: port.parse().ok()? })
                    }

                    fn unit() {}

                    fn ports() -> Vec<u16> {
                        Vec::new()
                    }
                ",
                short_tuple_exempt => r"
                    fn wrap(value: u32) -> (u32,) {
                        (value,)
                    }

                    fn nothing() -> () {}

                    fn checked() -> Result<(), String> {
                        Ok(())
                    }
                ",
                tuple_inside_collection_exempt => r"
                    fn pairs(items: &[String]) -> Vec<(usize, String)> {
                        items.iter().cloned().enumerate().collect()
                    }
                ",
                trait_contract_exempt => r"
                    trait Bounds {
                        fn bounds(&self) -> (u32, u32);
                    }

                    impl Bounds for Range {
                        fn bounds(&self) -> (u32, u32) {
                            (self.start, self.end)
                        }
                    }
                ",
                inline_test_exempt => r"
                    #[cfg(test)]
                    mod checks {
                        fn sample() -> (u32, u32) {
                            (1, 2)
                        }
                    }
                ",
            ],
            fail: [
                tuple_return => r"
                    fn bounds(values: &[u32]) -> (u32, u32) {
                        (values[0], values[1])
                    }
                " => "bounds",
                tuple_in_option => r"
                    fn split_host_port(address: &str) -> Option<(&str, u16)> {
                        let (host, port) = address.rsplit_once(':')?;
                        Some((host, port.parse().ok()?))
                    }
                " => "split_host_port",
                tuple_in_result_alias => r"
                    fn load_pair(path: &str) -> io::Result<(String, String)> {
                        let _ = path;
                        Ok((String::new(), String::new()))
                    }
                " => "load_pair",
                boxed_tuple => r"
                    fn boxed() -> Box<(u8, u8, u8)> {
                        Box::new((1, 2, 3))
                    }
                " => "boxed",
                inherent_method_tuple_return => r"
                    struct Grid;

                    impl Grid {
                        fn size(&self) -> (usize, usize) {
                            (3, 4)
                        }
                    }
                " => "size",
            ],
        },
    }
);
