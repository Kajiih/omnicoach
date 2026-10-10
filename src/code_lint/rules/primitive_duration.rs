//! Enforces strongly typed durations over numeric variables with time-unit suffixes.

use crate::code_lint::ast::ParsedFile;
use crate::code_lint::contract::{CodeRule, RuleTarget};
use crate::diagnostic::{Diagnostic, Language, RuleName, ViolationTemplate, violation_template};
use crate::rule_declaration::{
    Classification, Consensus, Declaration, Example, FilterListDefaults, ImpactedQuality, ListKind,
    ListOption, Precision, Reference, RuleDoc, RuleOptions, Topic,
};
use std::collections::HashSet;
use std::path::Path;

const BANNED: ListOption = ListOption {
    kind: ListKind::Deny,
    doc: "Time-unit suffixes flagged at the end of an identifier.",
    default: FilterListDefaults {
        base: &[
            "_seconds", "_secs", "_sec", "_minutes", "_mins", "_min", "_hours", "_hrs", "_hr",
            "_days", "_millis", "_ms", "_micros", "_us", "_nanos", "_ns",
        ],
        extend: &[],
        remove: &[],
    },
};

const TEMPLATE: ViolationTemplate = violation_template! {
    summary: "Name `{name}` ends with the time unit `{suffix}`.",
    rationale: "A duration kept as a plain number with its unit in the name is converted by hand at every call boundary, and one missed conversion is a silent bug.",
    suggestion: {
        Python => "Rename `{name}` to `{stem}` and type it as `datetime.timedelta` (or `whenever.TimeDelta`).",
        Rust => "Rename `{name}` to `{stem}` and type it as `std::time::Duration`.",
    },
};

/// The rule's declaration.
pub const RULE: CodeRule<ListOption> = CodeRule {
    declaration: Declaration {
        name: RuleName("primitive-duration"),
        template: &TEMPLATE,
        languages: &[Language::Python, Language::Rust],
        options: RuleOptions::code_rule(BANNED),
        classification: Classification {
            topics: &[Topic::TYPE_ENCODED_NAMES, Topic::DURATIONS],
            precision: Precision::Heuristic,
            consensus: Consensus::Opinionated,
            impacted_quality: ImpactedQuality::Reliability,
        },
        doc: RuleDoc {
            summary: "Flags durations held as plain numbers, detected by a time-unit suffix such as `timeout_secs` or `delay_ms`.",
            what_it_does: indoc::indoc! {r"
                Flags variables, parameters, constants and attributes whose name ends with a
                time unit, such as `timeout_secs` or `delay_ms`. Function and type names are not
                checked."},
            why_is_this_bad: indoc::indoc! {r"
                A plain number with a unit in its name relies on every caller reading the name:
                nothing stops passing milliseconds to a `timeout_secs` parameter, and each boundary
                needs a manual conversion that can be wrong by a factor of 1000.

                Use a duration type, `datetime.timedelta` in Python or `std::time::Duration` in
                Rust, and drop the suffix (`timeout: timedelta`). The unit is then chosen once,
                where the value is created (`timedelta(seconds=10)`, `Duration::from_millis(250)`)."},
            known_problems: Some(indoc::indoc! {r"
                Only the name is read, not the type, so a suffixed name is flagged even when it
                already holds a `timedelta` or `Duration`."}),
            references: &[
                Reference {
                    title: "Python docs: datetime.timedelta",
                    url: "https://docs.python.org/3/library/datetime.html#timedelta-objects",
                },
                Reference {
                    title: "Rust docs: std::time::Duration",
                    url: "https://doc.rust-lang.org/std/time/struct.Duration.html",
                },
            ],
            examples: &[
                Example {
                    language: Language::Python,
                    flagged: indoc::indoc! {r"
                        def wait_until_healthy(service, timeout_secs: float) -> None:
                            service.poll_health(timeout_secs)
                    "},
                    flagged_span: "timeout_secs",
                    fixed: indoc::indoc! {r"
                        from datetime import timedelta


                        def wait_until_healthy(service, timeout: timedelta) -> None:
                            service.poll_health(timeout)
                    "},
                },
                Example {
                    language: Language::Rust,
                    flagged: indoc::indoc! {r"
                        fn wait_until_healthy(service: &Service, timeout_secs: u64) {
                            service.poll_health(timeout_secs);
                        }
                    "},
                    flagged_span: "timeout_secs",
                    fixed: indoc::indoc! {r"
                        use std::time::Duration;

                        fn wait_until_healthy(service: &Service, timeout: Duration) {
                            service.poll_health(timeout);
                        }
                    "},
                },
            ],
        },
    },
    target: RuleTarget::All,
    check: check_file,
};

fn check_file(
    rule: &CodeRule<ListOption>,
    path: &Path,
    file: &ParsedFile,
    banned: &HashSet<String>,
) -> Vec<Diagnostic> {
    rule.check_banned_suffixes(path, file, banned)
}

#[cfg(test)]
crate::test_utils::rule_test!(
    RULE,
    {
        Python => {
            pass: [
                unaliased_import_exempt => r#"
                    import os_seconds
                "#,
                aliased_import_exempt => r#"
                    from datetime import timedelta as delta_secs
                "#,
                class_exempt => r#"
                    class Duration_Minutes:
                        pass
                "#,
                method_exempt => r#"
                    def parse_seconds(self):
                        pass
                "#,
                unsuffixed_duration_variables => r#"
                    from datetime import timedelta
                    timeout = timedelta(seconds=10)
                    delay = 500
                "#,
                exact_suffix_without_prefix_exempt => r#"
                    _ms = 10
                "#,
                attribute_writes_outside_declarations_not_checked => r#"
                    class Worker:
                        def update(self) -> None:
                            self.timeout_secs = 10
                "#,
            ],
            fail: [
                variable_with_seconds_suffix => r#"
                    timeout_secs = 10
                "# => "timeout_secs",
                variable_with_millis_suffix => r#"
                    delay_ms = 250
                "# => "delay_ms",
                function_parameter_suffix => r#"
                    def wait(timeout_secs):
                        pass
                "# => "timeout_secs",
                init_attribute_declaration => r#"
                    class Worker:
                        def __init__(self) -> None:
                            self.timeout_secs = 10
                "# => "timeout_secs",
            ],
        },
        Rust => {
            pass: [
                import_alias_exempt => r#"
                    use std::time::Duration as timeout_seconds;
                "#,
                struct_exempt => r#"
                    struct Timeout_Seconds;
                "#,
                fn_exempt => r#"
                    fn calculate_seconds() {}
                "#,
                unsuffixed_duration_variables => r#"
                    fn run() {
                        let timeout = std::time::Duration::from_secs(30);
                        let retry_delay = 500;
                    }
                "#,
                exact_suffix_without_prefix_exempt => r#"
                    fn run() {
                        let _secs = 5;
                    }
                "#,
            ],
            fail: [
                let_binding_seconds => r#"
                    fn run() {
                        let timeout_seconds = 30;
                    }
                "# => "timeout_seconds",
                let_binding_millis => r#"
                    fn run() {
                        let retry_delay_ms = 500;
                    }
                "# => "retry_delay_ms",
                const_binding_mins => r#"
                    const MAX_WAIT_MINS: u64 = 5;
                "# => "MAX_WAIT_MINS",
                function_parameter_suffix => r#"
                    fn wait(timeout_seconds: u64) {}
                "# => "timeout_seconds",
                struct_field => r#"
                    struct ClientConfig {
                        timeout_seconds: u64,
                    }
                "# => "timeout_seconds",
            ],
        },
    }
);
