//! Declarations of generic rules targeting multiple languages.

use crate::code_lint::ast::ParsedFile;
use crate::code_lint::rule::CodeDetector;
use crate::core::{Detector, FilterListDefaults};
use crate::diagnostic::{Diagnostic, RuleName, ViolationTemplate, violation_template};
use crate::rule_documentation::{ConfigShape, Reference, RuleDoc};
use crate::rule_taxonomy::{Classification, Consensus, ImpactedQuality, Precision, Topic};
use ast_grep_language::SupportLang;
use std::path::Path;

/// Static defaults for single-letter variable names.
const DEFAULT_ALLOWED: FilterListDefaults = FilterListDefaults {
    base: &["i", "j", "x", "f"],
    extend: &[(SupportLang::Rust, &["c"])],
    exempt: &[],
};

const TEMPLATE: ViolationTemplate = violation_template! {
    summary: "Variable name `{name}` is a single-letter.",
    rationale: "Single-letter variable names obscure domain intent, reduce code readability, and break grep/searchability by matching common characters indiscriminately across the codebase.",
    suggestion: "Rename `{name}` to a descriptive noun representing its domain role in an explicit and self explanatory way.",
};

/// Rule that bans single-letter variable names.
pub struct SingleLetterVariableName;

impl SingleLetterVariableName {
    /// The rule's declared facets (ADR 007).
    pub(crate) const CLASSIFICATION: Classification = Classification {
        topics: &[Topic::ABBREVIATED_NAMES],
        precision: Precision::Exact,
        consensus: Consensus::Opinionated,
        impacted_quality: ImpactedQuality::Maintainability,
    };

    /// The rule's user-facing doc.
    pub(crate) const DOC: RuleDoc = RuleDoc {
        summary: "Flags names made of a single letter.",
        what_it_does: "Flags single-letter names the code defines: variables, \
                       parameters (including lambda and closure parameters), loop, \
                       comprehension, `except ... as`, walrus and pattern bindings, and \
                       function, class and constant names. Allowed by default: `i`, `j`, \
                       `x` and `f`, plus `c` in Rust; `_` is never flagged. Unaliased \
                       imports, members of a Rust `impl Trait for Type` block, Python \
                       methods marked `@override`, type parameters such as `T`, and \
                       references to existing names are not checked.",
        why_is_this_bad: "A single letter says nothing about what the value is, so the \
                          reader has to trace where it comes from, and the meaning gets \
                          lost as the scope grows. Single letters are also impossible to \
                          search for: a search for `d` matches almost every line.\n\n\
                          Use a noun that says what the value is (`index`, `user`, \
                          `error`). Keep the allowed letters for conventional cases such \
                          as loop counters or coordinates.",
        configuration: &[ConfigShape::AllowList],
        references: &[Reference {
            title: "Google Python Style Guide: Naming",
            url: "https://google.github.io/styleguide/pyguide.html#316-naming",
        }],
    };
}

impl Detector for SingleLetterVariableName {
    fn name(&self) -> RuleName {
        RuleName("single-letter-variable-name")
    }

    fn supported_languages(&self) -> &'static [SupportLang] {
        &[SupportLang::Python, SupportLang::Rust]
    }

    fn violation_template(&self) -> &'static ViolationTemplate {
        &TEMPLATE
    }
}
impl CodeDetector for SingleLetterVariableName {
    fn check_file(
        &self,
        path: &Path,
        file: &ParsedFile,
        config: &crate::core::Config,
    ) -> Vec<Diagnostic> {
        let effective_allowed = self.effective_allowed_set(file.lang(), config, &DEFAULT_ALLOWED);

        let mut diagnostics = Vec::new();
        for node in crate::code_lint::semantic::bindings::collect_renameable_bindings(file) {
            let name = node.text();
            if name.len() == 1 && name != "_" && !effective_allowed.contains(&*name) {
                diagnostics.push(self.diagnostic_at_node(path, &node, &[("name", &name)]));
            }
        }
        diagnostics
    }
}

#[cfg(test)]
crate::test_utils::rule_test!(
    SingleLetterVariableName,
    {
        Python => {
            pass: [
                allowed_names => r#"
                    x = 1
                    i = 0
                    j = 0
                    f = open("file.txt")
                "#,
                descriptive_variable_names => r#"
                    index = 0
                    item = "data"
                    user = "alice"
                    error = None
                "#,
                parameter_allowed => r#"
                    def foo(i: int = 1):
                        pass
                "#,
                wildcard_ignored => r#"
                    _ = 1
                "#,
            ],
            fail: [
                rust_only_allowed_char_flagged_in_python => r#"
                    c = 2
                "# => "c",
                parameter_annotation => r#"
                    def foo(b: int = 1):
                        pass
                "# => "b",
                multi_assignment => r#"
                    x, d = 3, 4
                "# => "d",
                comprehension => r#"
                    [y for y in range(10)]
                "# => "y",
                exception_alias => r#"
                    try:
                        pass
                    except Exception as g:
                        pass
                "# => "g",
                walrus_expression => r#"
                    (v := 1)
                "# => "v",
            ],
        },
        Rust => {
            pass: [
                allowed_bindings => r#"
                    fn main() {
                        let i = 0;
                        let j = 0;
                        let x = 1;
                        let f = 2;
                        let c = 'a';
                    }
                "#,
                descriptive_variable_names => r#"
                    fn main() {
                        let index = 0;
                        let item = "data";
                        let user = "alice";
                        let error = None::<()>;
                    }
                "#,
                closure_parameter_allowed => r#"
                    fn main() {
                        let f = |x: i32| x + 1;
                    }
                "#,
                fn_parameter_allowed => r#"
                    fn test(i: i32, j: i32) {}
                "#,
                wildcard_ignored => r#"
                    fn main() {
                        let _ = 1;
                    }
                "#,
                single_letter_reference_not_flagged => r#"
                    fn main() {
                        let total = q + 1;
                    }
                "#,
            ],
            fail: [
                let_binding => r#"
                    fn main() {
                        let a = 1;
                    }
                "# => "a",
                mutable_binding => r#"
                    fn main() {
                        let mut b = 2;
                    }
                "# => "b",
                tuple_destructuring => r#"
                    fn main() {
                        let (c, d) = (1, 2);
                    }
                "# => "d",
                struct_destructuring_explicit => r#"
                    fn main() {
                        let Point { x: e, y: _ } = p;
                    }
                "# => "e",
                struct_destructuring_shorthand => r#"
                    fn main() {
                        let Point { f, g } = p;
                    }
                "# => "g",
                loop_target => r#"
                    fn main() {
                        for h in 0..10 {}
                    }
                "# => "h",
                match_pattern_variants => r#"
                    fn main() {
                        match val {
                            Some(k) => {}
                            None => {}
                        }
                    }
                "# => "k",
                if_let_pattern => r#"
                    fn main() {
                        if let Some(a) = y {}
                    }
                "# => "a",
                while_let_pattern => r#"
                    fn main() {
                        while let Some(z) = y {}
                    }
                "# => "z",
                match_pattern_guard => r#"
                    fn main() {
                        match val {
                            Some(z) if z > 0 => {}
                        }
                    }
                "# => "z",
            ],
        },
    }
);
