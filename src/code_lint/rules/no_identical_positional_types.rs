//! Enforces keyword-only parameters when a function has multiple positional parameters of identical type (`no-identical-positional-types`).

use crate::code_lint::ast_python::{PythonParameterInfo, extract_parameters, has_decorator};
use crate::code_lint::{AstNode, CodeRule, RuleTarget, SourceDoc};
use crate::core::{Config, DynamicRuleConfig, LanguageDefaults, Rule, RuleName, ThresholdConfig};
use crate::diagnostic::{Diagnostic, ViolationTemplate, violation_template};
use crate::rules::Tag;
use ast_grep_core::AstGrep;
use ast_grep_language::SupportLang;
use std::path::Path;

/// Default minimum number of positional parameters (excluding `self`/`cls`) before checking for duplicate types (`3`).
const DEFAULT_MIN_ARGS: LanguageDefaults<usize> = LanguageDefaults::new(3, &[]);

/// Configuration for the `NoIdenticalPositionalTypes` rule.
pub type NoIdenticalPositionalTypesConfig = DynamicRuleConfig<ThresholdConfig>;

const TEMPLATE: ViolationTemplate = violation_template! {
    summary: "Function `{func}` has multiple positional parameters with identical types ({duplicates}).",
    rationale: "Multiple positional parameters of the same type easily lead to silent argument transposition bugs at call sites that static type checkers cannot detect.",
    suggestion: "Make them keyword-only using `*` (e.g. `def {func}(..., *, {params}):`) to prevent accidental argument swapping.",
};

/// Rule that flags functions with `>= min_args` positional parameters where 2 or more share an identical type annotation.
pub struct NoIdenticalPositionalTypes;

impl Rule for NoIdenticalPositionalTypes {
    fn name(&self) -> RuleName {
        RuleName("no-identical-positional-types")
    }

    fn tags(&self) -> &'static [Tag] {
        &[Tag::Typing, Tag::Safety, Tag::Opinionated]
    }

    fn supported_languages(&self) -> &'static [SupportLang] {
        &[SupportLang::Python]
    }

    fn violation_template(&self) -> &'static ViolationTemplate {
        &TEMPLATE
    }
}

/// Returns true if `func_node` is a Python Data Model dunder method with a fixed runtime positional signature
/// (all `__*__` methods except constructors `__init__` and `__new__`).
fn is_exempt_dunder(func_name: &str) -> bool {
    func_name.starts_with("__")
        && func_name.ends_with("__")
        && func_name.len() > 4
        && !matches!(func_name, "__init__" | "__new__")
}

/// Returns true if `func_node` is decorated with `@override`, `@overload`, `@abstractmethod`, or `@fixture`.
fn has_exempt_decorator(func_node: &AstNode<'_>) -> bool {
    has_decorator(func_node, |terminal| {
        matches!(
            terminal,
            "override" | "overload" | "abstractmethod" | "fixture"
        )
    })
}

/// Groups typed positional parameters by type annotation and returns groups with `>= 2` parameters.
fn collect_duplicate_type_groups(params: &[PythonParameterInfo<'_>]) -> Vec<(String, Vec<String>)> {
    let mut groups: Vec<(String, Vec<String>)> = Vec::new();
    for param in params {
        if let Some(ref type_annotation) = param.type_text {
            if let Some((_, existing)) = groups
                .iter_mut()
                .find(|(seen_type, _)| seen_type == type_annotation)
            {
                existing.push(param.name.clone());
            } else {
                groups.push((type_annotation.clone(), vec![param.name.clone()]));
            }
        }
    }

    groups
        .into_iter()
        .filter(|(_, params)| params.len() >= 2)
        .collect()
}

/// Evaluates a single Python `"function_definition"` node and returns a consolidated diagnostic if violated.
fn check_function_definition(
    rule: &NoIdenticalPositionalTypes,
    func_node: &AstNode<'_>,
    path: &Path,
    min_args: usize,
) -> Option<Diagnostic> {
    let name_node = func_node.field("name")?;
    let params_node = func_node.field("parameters")?;
    let func_name = name_node.text();

    if is_exempt_dunder(&func_name) || has_exempt_decorator(func_node) {
        return None;
    }

    let all_params = extract_parameters(&params_node);
    let positional_params: Vec<_> = all_params
        .into_iter()
        .filter(PythonParameterInfo::is_positional)
        .collect();

    if positional_params.len() < min_args {
        return None;
    }

    let duplicate_groups = collect_duplicate_type_groups(&positional_params);
    if duplicate_groups.is_empty() {
        return None;
    }

    let duplicates = duplicate_groups
        .iter()
        .map(|(type_annotation, params)| format!("`{}: {type_annotation}`", params.join(", ")))
        .collect::<Vec<_>>()
        .join(", ");
    let all_params = duplicate_groups
        .iter()
        .flat_map(|(_, params)| params.iter().map(String::as_str))
        .collect::<Vec<_>>()
        .join(", ");

    Some(rule.diagnostic_at_node(
        path,
        &name_node,
        &[
            ("func", &func_name),
            ("duplicates", &duplicates),
            ("params", &all_params),
        ],
    ))
}

impl CodeRule for NoIdenticalPositionalTypes {
    fn target(&self) -> RuleTarget {
        RuleTarget::SourceOnly
    }

    fn check_file(
        &self,
        path: &Path,
        grep: &AstGrep<SourceDoc>,
        config: &Config,
    ) -> Vec<Diagnostic> {
        let lang = *grep.lang();
        let rule_config: NoIdenticalPositionalTypesConfig = config.get_rule_config(self.name().0);
        let min_args = rule_config.effective_min_for_lang(lang, &DEFAULT_MIN_ARGS);

        grep.root()
            .dfs()
            .filter(|node| node.kind() == "function_definition")
            .filter_map(|node| check_function_definition(self, &node, path, min_args))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::{assert_code_rule_snapshot, assert_code_rule_snapshot_with_config};

    #[test]
    fn test_identical_positional_types_flagged_and_exemptions() {
        let source = r#"
from typing import override, overload

# Flagged: 4 positional args, duplicate `str` and duplicate `int`
def transfer(source_id: str, target_id: str, amount: int, fee: int) -> None:
    pass

# Flagged: non-adjacent duplicate types with >= 3 positional args
def create_item(owner_id: str, weight: float, item_id: str = "default") -> None:
    pass

# OK: only 2 positional args (< default min_args of 3)
def add(a: int, b: int) -> int:
    return a + b

# OK: separated by keyword-only `*`
def safe_transfer(source_id: str, *, target_id: str, amount: int, fee: int) -> None:
    pass

# OK: args after `*args` are already keyword-only
def vararg_fn(first: str, *args: int, second: str, third: str) -> None:
    pass

class AccountService:
    # Flagged: __init__ has 3 positional args (excluding `self`) with duplicate `str`
    def __init__(self, host: str, port: int, api_key: str) -> None:
        pass

    # OK: `self` excluded, so only 2 positional args (`a`, `b`)
    def compute(self, a: int, b: int) -> int:
        return a + b

    # OK: runtime dunder method with fixed positional protocol
    def __exit__(self, exc_type: object, exc_val: object, exc_tb: object) -> None:
        pass

    # OK: @override method constrained by parent signature
    @override
    def sync(self, primary: str, secondary: str, retries: int) -> None:
        pass

@overload
def overloaded_fn(a: str, b: str, c: int) -> int: ...
"#;

        insta::assert_snapshot!(
            assert_code_rule_snapshot(&NoIdenticalPositionalTypes, source, "src/service.py"),
            @"
        [no-identical-positional-types] Line 5, Col 5: Function `transfer` has multiple positional parameters with identical types (`source_id, target_id: str`, `amount, fee: int`).
        [no-identical-positional-types] Line 9, Col 5: Function `create_order` has multiple positional parameters with identical types (`owner_id, item_id: str`).
        [no-identical-positional-types] Line 26, Col 9: Function `__init__` has multiple positional parameters with identical types (`host, api_key: str`).
        "
        );
    }

    #[test]
    fn test_custom_min_args_config() {
        let source = r"
def connect(host: str, token: str) -> None:
    pass
";
        let config_toml = r"
[rules.no-identical-positional-types]
min_args = 2
";
        let config: Config = toml::from_str(config_toml).unwrap();
        insta::assert_snapshot!(
            assert_code_rule_snapshot_with_config(&NoIdenticalPositionalTypes, source, "src/net.py", &config),
            @"[no-identical-positional-types] Line 2, Col 5: Function `connect` has multiple positional parameters with identical types (`host, token: str`)."
        );
    }
}
