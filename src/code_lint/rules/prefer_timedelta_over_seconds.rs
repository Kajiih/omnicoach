//! Enforces strongly typed durations over numeric variables with time-unit suffixes.

use crate::code_lint::ast::ParsedFile;
use crate::code_lint::rule::CodeRule;
use crate::core::{FilterListDefaults, Rule, Tag};
use crate::diagnostic::{Diagnostic, RuleName, ViolationTemplate, violation_template};
use ast_grep_language::SupportLang;
use std::path::Path;

/// Static defaults for banned time-unit suffixes.
const DEFAULT_BANNED_SUFFIXES: FilterListDefaults = FilterListDefaults {
    base: &[
        "_seconds", "_secs", "_sec", "_minutes", "_mins", "_min", "_hours", "_hrs", "_hr", "_days",
        "_millis", "_ms", "_micros", "_us", "_nanos", "_ns",
    ],
    extend: &[],
    exempt: &[],
};

const TEMPLATE: ViolationTemplate = violation_template! {
    summary: "Identifier `{name}` ends with raw time unit suffix `{actual_suffix}`.",
    rationale: "Representing time durations as primitive numbers with unit suffixes (`timeout_s`, `delay_ms`) risks unit-conversion bugs across call boundaries.",
    suggestion: {
        base: "Rename `{name}` to `{base_name}` and type it as `datetime.timedelta` (Python) or `std::time::Duration` (Rust).",
        Python => "Rename `{name}` to `{base_name}` and type it as `datetime.timedelta` (or `whenever.TimeDelta`).",
        Rust => "Rename `{name}` to `{base_name}` and type it as `std::time::Duration`.",
    },
};

/// Rule that flags numeric variables encoding time unit suffixes.
pub struct PreferTimedeltaOverSeconds;

impl Rule for PreferTimedeltaOverSeconds {
    fn name(&self) -> RuleName {
        RuleName("prefer-timedelta-over-seconds")
    }

    fn tags(&self) -> &'static [Tag] {
        &[Tag::Style, Tag::Naming, Tag::Typing]
    }

    fn supported_languages(&self) -> &'static [SupportLang] {
        &[SupportLang::Python, SupportLang::Rust]
    }

    fn violation_template(&self) -> &'static ViolationTemplate {
        &TEMPLATE
    }
}

impl CodeRule for PreferTimedeltaOverSeconds {
    fn check_file(
        &self,
        path: &Path,
        file: &ParsedFile,
        config: &crate::core::Config,
    ) -> Vec<Diagnostic> {
        self.check_banned_suffixes(path, file, config, &DEFAULT_BANNED_SUFFIXES)
    }
}

#[cfg(test)]
crate::test_utils::rule_test!(
    PreferTimedeltaOverSeconds,
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
