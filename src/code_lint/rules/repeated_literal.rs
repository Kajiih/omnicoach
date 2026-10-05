//! Flags a literal repeated inline in one file instead of being named once.

use crate::code_lint::ast::{self, AstNode, LiteralRole, LiteralValue, ParsedFile};
use crate::code_lint::contract::{CodeRule, RuleTarget};
use crate::code_lint::policy::is_trivial_literal;
use crate::diagnostic::{Diagnostic, RuleName, ViolationTemplate, violation_template};
use crate::rule_declaration::{
    Classification, Consensus, CountOption, Declaration, Example, ImpactedQuality,
    LanguageDefaults, Precision, Reference, RuleDoc, RuleOptions, Topic,
};
use ast_grep_language::SupportLang;
use std::collections::HashMap;
use std::path::Path;

const MIN_OCCURRENCES: CountOption = CountOption {
    key: "min-occurrences",
    doc: "Minimum occurrences of a literal in a file, its constant definitions included, for it \
          to be flagged. Values below 2 behave as 2.",
    default: LanguageDefaults::new(2, &[]),
};

const TEMPLATE: ViolationTemplate = violation_template! {
    summary: "Literal `{expression}` appears {count} times in this file.",
    rationale: "A value copied inline has no name, and changing it means finding every copy: missing one silently desynchronizes the file.",
    suggestion: "Extract `{expression}` into one named constant (or reuse the existing one) and reference it at every site.",
};

/// The rule's declaration.
pub const RULE: CodeRule<CountOption> = CodeRule {
    declaration: Declaration {
        name: RuleName("repeated-literal"),
        template: &TEMPLATE,
        languages: &[SupportLang::Python, SupportLang::Rust],
        options: RuleOptions::code_rule(MIN_OCCURRENCES),
        classification: Classification {
            topics: &[Topic::LITERALS],
            precision: Precision::Heuristic,
            consensus: Consensus::Opinionated,
            impacted_quality: ImpactedQuality::Maintainability,
        },
        doc: RuleDoc {
            summary: "Flags a string or number literal repeated inline in one file instead of \
                      being named once.",
            what_it_does: indoc::indoc! {r#"
                Groups the string, byte-string and number literals of a file by value: quote style,
                raw prefixes, digit separators, base prefixes and Rust type suffixes do not matter
                (`'id'` equals `"id"`, `1_000` equals `1000`, `30u64` equals `30`), but strings and
                bytes, integers and floats, and a number and its negation are distinct. Values not
                worth naming are ignored: strings shorter than 2 characters or without a letter or
                digit (`", "`, `"\n"`), and the numbers -1, 0, 1, 2, -1.0, 0.0, 1.0 and 2.0. When a
                scalar constant holds the value (`MAX_RETRIES = 3` or a `Final` at Python module or
                class level, also inside a module-level `if`, `try` or `with`; a Rust `const`,
                non-`mut` `static` or enum discriminant), every inline use is flagged once
                definitions and inline uses reach `min-occurrences`; otherwise every inline use
                after the first is flagged once inline uses reach it. Two constants sharing a value
                are not flagged, and a constant whose initializer is not a single literal
                (`HOSTS = ["a", "b"]`, `Regex::new("…")`) is not collected. Positions that are not
                values are skipped: Python docstrings and annotations, Rust tuple positions
                (`pair.0`). So are literals the language requires: Python `Literal[...]` and the
                first argument of `TypeVar`, `NewType`, `ParamSpec`, `TypeVarTuple`, `NamedTuple`,
                `TypedDict` and `cast`; Rust attributes, `extern` ABI strings and the arguments of
                formatting, assertion, logging and compile-time macros (`format!`, `println!`,
                `assert_eq!`, `panic!`, `bail!`, `warn!`, `include_str!` and similar). Interpolated
                Python f-strings are templates and skipped, but literals inside their `{...}` count,
                as do `case` and `match` patterns and `matches!`. A negative number written as a
                bare `-` token, inside a Rust macro or a Python mapping, keyword or `|` pattern, is
                skipped. Test files and Rust `#[cfg(test)]` and `#[test]` code are not checked. The
                reported count covers only the occurrences collected under these rules."#},
            why_is_this_bad: indoc::indoc! {r#"
                A literal copied across a file is a value without a name: the reader must guess what
                `"primary-db"` or `30` stands for, and changing it means finding every copy. Missing
                one leaves the file silently inconsistent, and a search cannot tell the copies of
                this value from an unrelated literal that happens to be equal.

                Extract the value into one named constant and reference it at every site, or reuse
                the constant that already holds it. In a Python `case` pattern, match an `Enum`
                member or class constant by dotted name (`case Mode.READ:`): a bare name captures
                any value instead of comparing."#},
            references: &[
                Reference {
                    title: "SonarSource RSPEC-1192: String literals should not be duplicated",
                    url: "https://rules.sonarsource.com/python/RSPEC-1192/",
                },
                Reference {
                    title: "goconst: find repeated strings that could be constants",
                    url: "https://github.com/jgautheron/goconst",
                },
                Reference {
                    title: "Checkstyle: MultipleStringLiterals",
                    url: "https://checkstyle.sourceforge.io/checks/coding/multiplestringliterals.html",
                },
            ],
            examples: &[
                Example {
                    language: SupportLang::Python,
                    flagged: indoc::indoc! {r#"
                        def fetch_orders(session):
                            return session.get("https://api.example.com/orders")

                        def create_order(session, order):
                            return session.post("https://api.example.com/orders", json=order)
                    "#},
                    flagged_span: "\"https://api.example.com/orders\"",
                    fixed: indoc::indoc! {r#"
                        ORDERS_URL = "https://api.example.com/orders"

                        def fetch_orders(session):
                            return session.get(ORDERS_URL)

                        def create_order(session, order):
                            return session.post(ORDERS_URL, json=order)
                    "#},
                },
                Example {
                    language: SupportLang::Rust,
                    flagged: indoc::indoc! {r#"
                        fn read_config() -> io::Result<String> {
                            fs::read_to_string("omni.toml")
                        }

                        fn has_config() -> bool {
                            Path::new("omni.toml").exists()
                        }
                    "#},
                    flagged_span: "\"omni.toml\"",
                    fixed: indoc::indoc! {r#"
                        const CONFIG_FILE: &str = "omni.toml";

                        fn read_config() -> io::Result<String> {
                            fs::read_to_string(CONFIG_FILE)
                        }

                        fn has_config() -> bool {
                            Path::new(CONFIG_FILE).exists()
                        }
                    "#},
                },
            ],
        },
    },
    target: RuleTarget::SourceOnly,
    check: check_file,
};

/// The occurrences of one literal value in a file.
struct LiteralGroup<'a> {
    definitions: usize,
    inline_uses: Vec<AstNode<'a>>,
}

impl<'a> LiteralGroup<'a> {
    /// The inline uses to flag: all of them once a constant holds the value, every use after
    /// the first otherwise. A group without definitions has at least one inline use.
    fn flagged_uses(&self, min_occurrences: usize) -> &[AstNode<'a>] {
        let inline_count = self.inline_uses.len();
        if self.definitions > 0 && self.definitions + inline_count >= min_occurrences {
            &self.inline_uses
        } else if self.definitions == 0 && inline_count >= min_occurrences {
            &self.inline_uses[1..]
        } else {
            &[]
        }
    }
}

/// Groups the file's non-trivial literals by value, in first-seen order. Rust test code is
/// left out so a test literal cannot pair with a production one.
fn group_literals(file: &ParsedFile) -> Vec<LiteralGroup<'_>> {
    let test_ranges = if file.lang() == SupportLang::Rust {
        ast::rust::collect_inline_test_ranges(file)
    } else {
        Vec::new()
    };
    let mut index_by_value: HashMap<LiteralValue, usize> = HashMap::new();
    let mut groups: Vec<LiteralGroup<'_>> = Vec::new();
    for occurrence in ast::collect_literal_occurrences(file) {
        let start = occurrence.node.span().start;
        if is_trivial_literal(&occurrence.value)
            || test_ranges.iter().any(|range| range.contains(&start))
        {
            continue;
        }
        let index = *index_by_value.entry(occurrence.value).or_insert_with(|| {
            groups.push(LiteralGroup {
                definitions: 0,
                inline_uses: Vec::new(),
            });
            groups.len() - 1
        });
        let group = &mut groups[index];
        match occurrence.role {
            LiteralRole::ConstantDefinition => group.definitions += 1,
            LiteralRole::Inline => group.inline_uses.push(occurrence.node),
        }
    }
    groups
}

fn check_file(
    rule: &CodeRule<CountOption>,
    path: &Path,
    file: &ParsedFile,
    min_occurrences: usize,
) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    for group in group_literals(file) {
        let count = (group.definitions + group.inline_uses.len()).to_string();
        for node in group.flagged_uses(min_occurrences) {
            diagnostics.push(rule.diagnostic_at_node(
                path,
                node,
                &[("expression", &node.text()), ("count", &count)],
            ));
        }
    }
    diagnostics
}

#[cfg(test)]
crate::test_utils::rule_test!(
    RULE,
    repeat: DistinctLiterals,
    {
        Python => {
            pass: [
                single_occurrence => r#"
                    connect("primary-db")
                "#,
                trivial_integers => r#"
                    f(0, 1, -1, 2)
                    g(0, 1, -1, 2)
                "#,
                trivial_floats => r#"
                    f(0.0, 1.0, -1.0, 2.0)
                    g(0.0, 1.0, -1.0, 2.0)
                "#,
                single_char_strings => r#"
                    f("a") + f("a")
                "#,
                non_alphanumeric_strings => r#"
                    ", ".join(a) + ", ".join(b)
                "#,
                constants_sharing_a_value => r#"
                    MAX_RETRIES = 3
                    TUPLE_ARITY = 3
                "#,
                composite_constant_does_not_pair_with_single_use => r#"
                    HOSTS = ["primary-db", "replica-db"]
                    connect("primary-db")
                "#,
                str_and_bytes_are_distinct => r#"
                    send("ping")
                    send(b"ping")
                "#,
                number_and_negation_are_distinct => r#"
                    shift(42)
                    shift(-42)
                "#,
                raw_and_plain_strings_with_backslashes_are_distinct => r#"
                    re.split(r"a\nb", text)
                    text.split("a\nb")
                "#,
                repeated_docstrings => r#"
                    def first():
                        """Return the value."""

                    def second():
                        """Return the value."""
                "#,
                repeated_string_annotations => r#"
                    def first(node: "Node"): ...
                    def second(node: "Node"): ...
                "#,
                repeated_literal_type_values => r#"
                    first = Literal["on"]
                    second = Literal["on"]
                "#,
                repeated_type_names_in_cast => r#"
                    first = cast("Node", a)
                    second = cast("Node", b)
                "#,
                repeated_interpolated_fstrings => r#"
                    log(f"user {name} logged in")
                    log(f"user {name} logged in")
                "#,
                known_gap_negative_numbers_in_mapping_and_keyword_patterns => r#"
                    match response:
                        case {-404: _}:
                            pass
                        case Response(status=-404):
                            pass
                "#,
            ],
            fail: [
                repeated_string_flags_second_use => r#"
                    connect("primary-db")
                    reconnect("primary-db")
                "# => r#""primary-db""#,
                repeated_number_flags_second_use => r#"
                    retry(timeout=30)
                    wait(30)
                "# => "30",
                repeated_negative_number => r#"
                    offset(-42)
                    shift(-42)
                "# => "-42",
                constant_then_inline_use_flags_inline => r#"
                    MODE = "fast"
                    run("fast")
                "# => r#""fast""#,
                constant_after_inline_use_flags_inline => r#"
                    run('fast')
                    MODE = "fast"
                "# => "'fast'",
                constant_in_module_if_body_flags_inline_use => r#"
                    connect(mode='safe')
                    if sys.platform == "win32":
                        MODE = "safe"
                "# => "'safe'",
                parenthesized_constant_flags_inline_use => r#"
                    log('connection refused')
                    MESSAGE = (
                        "connection refused"
                    )
                "# => "'connection refused'",
                quote_styles_are_the_same_literal => r#"
                    open('data.csv')
                    read("data.csv")
                "# => r#""data.csv""#,
                raw_and_plain_string_are_the_same_literal => r#"
                    open(r"data.csv")
                    read("data.csv")
                "# => r#""data.csv""#,
                digit_separators_are_the_same_number => r#"
                    limit(1_000)
                    cap(1000)
                "# => "1000",
                literal_inside_interpolation_counts => r#"
                    print(f"{row['status']}")
                    check(row['status'])
                "# => "'status'",
                case_pattern_literal_counts => r#"
                    match command:
                        case "deploy":
                            run()
                        case _:
                            log("deploy")
                "# => r#""deploy""#,
                repeated_byte_string => r#"
                    send(b"ping")
                    expect(b"ping")
                "# => r#"b"ping""#,
            ],
        },
        Rust => {
            pass: [
                single_occurrence => r#"
                    fn f() { connect("primary-db"); }
                "#,
                trivial_integers => r#"
                    fn f() {
                        g(0, 1, -1, 2);
                        h(0, 1, -1, 2);
                    }
                "#,
                trivial_floats => r#"
                    fn f() {
                        g(0.0, 1.0, -1.0, 2.0);
                        h(0.0, 1.0, -1.0, 2.0);
                    }
                "#,
                single_char_strings => r#"
                    fn f() {
                        g("a");
                        g("a");
                    }
                "#,
                non_alphanumeric_strings => r#"
                    fn f() {
                        a.join(", ");
                        b.join(", ");
                    }
                "#,
                constants_sharing_a_value => r#"
                    const MAX_RETRIES: u32 = 3;
                    const TUPLE_ARITY: usize = 3;
                "#,
                composite_constant_does_not_pair_with_single_use => r#"
                    const HOSTS: &[&str] = &["primary-db", "replica-db"];
                    fn f() { connect("primary-db"); }
                "#,
                str_and_bytes_are_distinct => r#"
                    fn f() {
                        send("ping");
                        send(b"ping");
                    }
                "#,
                number_and_negation_are_distinct => r#"
                    fn f() {
                        shift(42);
                        shift(-42);
                    }
                "#,
                raw_and_plain_strings_with_backslashes_are_distinct => r#"
                    fn f() {
                        split(r"a\tb");
                        split("a\tb");
                    }
                "#,
                test_module_literals_are_ignored => r#"
                    #[cfg(test)]
                    mod integration {
                        fn t() { connect("primary-db"); }
                    }

                    fn f() { connect("primary-db"); }
                "#,
                repeated_attribute_arguments => r#"
                    #[cfg(feature = "cli")]
                    fn first() {}

                    #[cfg(feature = "cli")]
                    fn second() {}
                "#,
                repeated_format_macro_arguments => r#"
                    fn f() {
                        println!("invalid count");
                        println!("invalid count");
                    }
                "#,
                repeated_assert_macro_arguments => r#"
                    fn f(count: u32) {
                        assert!(count > 0, "invalid count");
                        assert!(count < 9, "invalid count");
                    }
                "#,
                tuple_field_positions_are_not_literals => r#"
                    fn f(t: (u32, u32, u32, u32)) -> u32 {
                        t.3 + t.3
                    }
                "#,
                repeated_extern_abi => r#"
                    extern "C" { fn first(); }
                    extern "C" { fn second(); }
                "#,
                known_gap_values_inside_exempt_macros => r#"
                    fn f(status: &str) {
                        assert_eq!(status, "expected");
                        assert_eq!(status, "expected");
                    }
                "#,
                known_gap_negative_numbers_in_macros => r#"
                    fn f() {
                        vec![-42];
                        vec![-42];
                    }
                "#,
            ],
            fail: [
                repeated_string_flags_second_use => r#"
                    fn f() {
                        connect("primary-db");
                        reconnect("primary-db");
                    }
                "# => r#""primary-db""#,
                repeated_number_flags_second_use => r#"
                    fn f() {
                        retry(30);
                        wait(30);
                    }
                "# => "30",
                repeated_negative_number => r#"
                    fn f() {
                        offset(-42);
                        shift(-42);
                    }
                "# => "-42",
                constant_then_inline_use_flags_inline => r#"
                    const JJ: &str = "jj";
                    fn is_jj(program: &str) -> bool { program != "jj" }
                "# => r#""jj""#,
                constant_after_inline_use_flags_inline => r#"
                    fn is_jj(program: &str) -> bool { program != r"jj" }
                    const JJ: &str = "jj";
                "# => r#"r"jj""#,
                static_after_inline_use_flags_inline => r#"
                    fn f() { greet(r"omni"); }
                    static NAME: &str = "omni";
                "# => r#"r"omni""#,
                enum_discriminant_after_inline_use_flags_inline => r#"
                    fn f() { reply(404u16); }
                    enum Status { NotFound = 404 }
                "# => "404u16",
                static_mut_initializer_is_an_inline_use => r#"
                    fn f() { wait(30u32); }
                    static mut LIMIT: u32 = 30;
                "# => "30",
                raw_and_plain_string_are_the_same_literal => r#"
                    fn f() {
                        open(r"data.csv");
                        read("data.csv");
                    }
                "# => r#""data.csv""#,
                digit_separators_are_the_same_number => r#"
                    fn f() {
                        limit(1_000);
                        cap(1000);
                    }
                "# => "1000",
                type_suffix_is_the_same_number => r#"
                    fn f() {
                        retry(30u64);
                        wait(30);
                    }
                "# => "30",
                match_arm_literal_counts => r#"
                    fn f(command: &str) {
                        match command {
                            "deploy" => run(),
                            _ => log("deploy"),
                        }
                    }
                "# => r#""deploy""#,
                matches_pattern_literal_counts => r#"
                    fn f(command: &str) -> bool {
                        matches!(command, "deploy") || command == "deploy"
                    }
                "# => r#""deploy""#,
                repeated_byte_string => r#"
                    fn f() {
                        send(b"ping");
                        expect(b"ping");
                    }
                "# => r#"b"ping""#,
            ],
        },
    }
);
