//! Enforces strongly typed durations over numeric variables with time-unit suffixes.

use crate::code_lint::ast::ParsedFile;
use crate::code_lint::rule::{CodeRule, RuleTarget};
use crate::diagnostic::{Diagnostic, RuleName, ViolationTemplate, violation_template};
use crate::rule_declaration::{
    Classification, Consensus, Declaration, FilterListDefaults, ImpactedQuality, ListKind,
    ListOption, Precision, Reference, RuleDoc, RuleOptions, Topic,
};
use ast_grep_language::SupportLang;
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
        base: "Rename `{name}` to `{stem}` and give it a duration type.",
        Python => "Rename `{name}` to `{stem}` and type it as `datetime.timedelta` (or `whenever.TimeDelta`).",
        Rust => "Rename `{name}` to `{stem}` and type it as `std::time::Duration`.",
    },
};

/// The rule's declaration.
pub const RULE: CodeRule<ListOption> = CodeRule {
    declaration: Declaration {
        name: RuleName("primitive-duration"),
        template: &TEMPLATE,
        languages: &[SupportLang::Python, SupportLang::Rust],
        options: RuleOptions::code_rule(BANNED),
        classification: Classification {
            topics: &[Topic::TYPE_ENCODED_NAMES, Topic::DURATIONS],
            precision: Precision::Heuristic,
            consensus: Consensus::Opinionated,
            impacted_quality: ImpactedQuality::Reliability,
        },
        doc: RuleDoc {
            summary: "Flags durations held as plain numbers, detected by a time-unit suffix such as `timeout_secs` or `delay_ms`.",
            what_it_does: "Flags variables, parameters, loop and pattern bindings, and \
                           constants whose name ends, ignoring case, with a time-unit suffix: \
                           `_seconds`, `_secs`, `_sec`, `_minutes`, `_mins`, `_min`, `_hours`, \
                           `_hrs`, `_hr`, `_days`, `_millis`, `_ms`, `_micros`, `_us`, \
                           `_nanos` or `_ns` by default. The check reads the name only, not \
                           the type, so a suffixed name is flagged even when it already holds \
                           a `timedelta` or `Duration`. A name that is only the suffix, such \
                           as `_ms`, is not flagged. Functions, classes, structs, imports \
                           (aliased or not), attributes and struct fields are not checked.",
            why_is_this_bad: "A plain number with a unit in its name relies on every caller \
                              reading the name: nothing stops passing milliseconds to a \
                              `timeout_secs` parameter, and each boundary needs a manual \
                              conversion that can be wrong by a factor of 1000.\n\n\
                              Use a duration type, `datetime.timedelta` in Python or \
                              `std::time::Duration` in Rust, and drop the suffix \
                              (`timeout: timedelta`). The unit is then chosen once, where the \
                              value is created (`timedelta(seconds=10)`, \
                              `Duration::from_millis(250)`).",
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
                import_and_alias_exempt => r#"
                    import os_seconds
                    from datetime import timedelta as delta_secs
                "#,
                class_and_method_exempt => r#"
                    class DurationMinutes:
                        def parse_seconds(self):
                            pass
                "#,
                typed_duration_and_unsuffixed => r#"
                    from datetime import timedelta
                    timeout = timedelta(seconds=10)
                    delay = 500
                "#,
                exact_suffix_without_prefix_exempt => r#"
                    _ms = 10
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
            ],
        },
        Rust => {
            pass: [
                import_alias_exempt => r#"
                    use std::time::Duration as timeout_seconds;
                "#,
                struct_and_fn_exempt => r#"
                    struct TimeoutSeconds;
                    fn calculate_seconds() {}
                "#,
                typed_duration_and_unsuffixed => r#"
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
            ],
        },
    }
);
