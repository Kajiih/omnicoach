//! Enforces strongly typed durations over numeric variables with time-unit suffixes.

use crate::code_lint::CodeRule;
use crate::core::{DenyListConfig, DynamicRuleConfig, FilterListDefaults, Rule, RuleName};
use crate::diagnostic::{Diagnostic, ViolationTemplate, violation_template};
use crate::rules::Tag;
use ast_grep_core::AstGrep;
use ast_grep_language::SupportLang;
use std::path::Path;

/// Configuration for the `PreferTimedeltaOverSeconds` rule.
pub type PreferTimedeltaOverSecondsConfig = DynamicRuleConfig<DenyListConfig>;

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
    summary: "Identifier `{name}` encodes a raw time unit suffix `{actual_suffix}`.",
    rationale: "Representing durations as raw numeric primitives with unit suffixes invites unit-conversion bugs (e.g. mixing milliseconds and seconds) and weakens type safety.",
    suggestion: {
        base: "Rename `{name}` to `{base_name}` and use a strongly typed duration such as `datetime.timedelta` (or `whenever.TimeDelta`).",
        Rust => "Rename `{name}` to `{base_name}` and use `std::time::Duration`.",
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
        grep: &AstGrep<crate::code_lint::SourceDoc>,
        config: &crate::core::Config,
    ) -> Vec<Diagnostic> {
        let rule_config: PreferTimedeltaOverSecondsConfig = config.get_rule_config(self.name().0);
        let effective_banned =
            rule_config.effective_banned_for_lang(*grep.lang(), &DEFAULT_BANNED_SUFFIXES);

        crate::code_lint::find_suffixed_bindings(grep, &effective_banned)
            .into_iter()
            .map(|matched| {
                self.diagnostic_at_node(
                    path,
                    &matched.node,
                    &[
                        ("name", &matched.name),
                        ("actual_suffix", &matched.actual_suffix),
                        ("base_name", &matched.base_name),
                    ],
                )
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::assert_code_rule_snapshot;

    #[test]
    fn test_rust_snapshots() {
        let rule = PreferTimedeltaOverSeconds;

        let source = r"
            use std::time::Duration as timeout_seconds; // OK (import alias)
            struct TimeoutSeconds; // OK (struct definition)
            fn calculate_seconds() { // OK (function name)
                let timeout_seconds = 30;
                let retry_delay_ms = 500;
                const MAX_WAIT_MINS: u64 = 5;
                let timeout = std::time::Duration::from_secs(30); // OK
            }
        ";
        insta::assert_snapshot!(assert_code_rule_snapshot(&rule, source, "test.rs"), @"
        [prefer-timedelta-over-seconds] Line 5, Col 21: Identifier `timeout_seconds` encodes a raw time unit suffix `_seconds`.
        [prefer-timedelta-over-seconds] Line 6, Col 21: Identifier `retry_delay_ms` encodes a raw time unit suffix `_ms`.
        [prefer-timedelta-over-seconds] Line 7, Col 23: Identifier `MAX_WAIT_MINS` encodes a raw time unit suffix `_MINS`.
        ");
    }

    #[test]
    fn test_python_snapshots() {
        let rule = PreferTimedeltaOverSeconds;

        let source = r"
import os_seconds # OK (import)
from datetime import timedelta as delta_secs # OK (import alias)
class DurationMinutes: # OK (class definition)
    def parse_seconds(self): # OK (method definition)
        timeout_secs = 10
        delay_ms = 250
        ttl_hours = 24
        duration = timedelta(seconds=10) # OK
        ";
        insta::assert_snapshot!(assert_code_rule_snapshot(&rule, source, "test.py"), @"
        [prefer-timedelta-over-seconds] Line 6, Col 9: Identifier `timeout_secs` encodes a raw time unit suffix `_secs`.
        [prefer-timedelta-over-seconds] Line 7, Col 9: Identifier `delay_ms` encodes a raw time unit suffix `_ms`.
        [prefer-timedelta-over-seconds] Line 8, Col 9: Identifier `ttl_hours` encodes a raw time unit suffix `_hours`.
        ");
    }
}
