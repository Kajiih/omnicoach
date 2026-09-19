//! Flags compound boolean conditions (`&&`, `and`) and boolean tuple equality packing in test assertions (`no-assertion-packing`).

use crate::code_lint::{AstNode, CodeRule, RuleTarget, SourceDoc};
use crate::core::{Config, Rule, RuleName};
use crate::diagnostic::{violation_template, Diagnostic, ViolationTemplate};
use crate::rules::Tag;
use ast_grep_core::AstGrep;
use ast_grep_language::SupportLang;
use std::path::Path;

const TEMPLATE: ViolationTemplate = violation_template! {
    summary: {
        base: "{construct} in assertion.",
        Python => "{construct} in `assert` statement.",
        Rust => "{construct} in `{macro_name}!` assertion.",
    },
    rationale: "Packing multiple conditions or synthesized boolean tuples into a single assertion obscures which check failed, yields unhelpful diffs, and circumvents assertion limits.",
    suggestion: {
        base: "Split into separate atomic assertions or assert directly on domain objects/collections.",
        Python => "Split into separate atomic assertions (e.g. `assert a\\nassert b`) or assert directly on the domain model (`assert actual == expected`).",
        Rust => "Split into separate atomic assertions (`assert!(...); assert!(...);`) or assert directly on the domain model (`assert_eq!(actual, expected)`).",
    },
};

/// Rule that bans compound boolean conditions and boolean tuple packing in assertions.
pub struct NoAssertionPacking;

impl Rule for NoAssertionPacking {
    fn name(&self) -> RuleName {
        RuleName("no-assertion-packing")
    }

    fn tags(&self) -> &'static [Tag] {
        &[Tag::Testing, Tag::Opinionated, Tag::Correctness]
    }

    fn supported_languages(&self) -> &'static [SupportLang] {
        &[SupportLang::Python, SupportLang::Rust]
    }

    fn violation_template(&self) -> &'static ViolationTemplate {
        &TEMPLATE
    }
}

/// Returns true if a Rust macro node is an assertion macro (`assert!`, `assert_*!`, `debug_assert!`, etc.).
fn is_rust_assertion_macro(macro_node: &AstNode<'_>) -> bool {
    let raw_text = macro_node.text();
    let prefix = raw_text.split('!').next().unwrap_or("");
    let terminal = prefix.rsplit("::").next().unwrap_or("").trim();
    terminal == "assert"
        || terminal.starts_with("assert_")
        || terminal == "debug_assert"
        || terminal.starts_with("debug_assert_")
}

/// Returns the terminal macro name (e.g. `assert`, `assert_eq`).
fn rust_macro_terminal(macro_node: &AstNode<'_>) -> String {
    let raw_text = macro_node.text();
    let prefix = raw_text.split('!').next().unwrap_or("");
    prefix.rsplit("::").next().unwrap_or("").trim().to_string()
}

/// Checks if a Rust `token_tree` contains a top-level `&&` operator.
fn has_rust_top_level_and(token_tree: &AstNode<'_>) -> bool {
    let meaningful_children: Vec<_> =
        token_tree.children().filter(|c| c.kind() != "(" && c.kind() != ")").collect();

    // Direct top-level: assert!(a && b)
    if meaningful_children.iter().any(|c| c.kind() == "&&") {
        return true;
    }

    // Outer paren wrapped: assert!((a && b)) where the only child is a paren token_tree
    if meaningful_children.len() == 1 && meaningful_children[0].kind() == "token_tree" {
        return meaningful_children[0].children().any(|c| c.kind() == "&&");
    }

    false
}

/// Checks if a Rust node represents a tuple or array consisting solely of boolean literals (>= 2 elements).
fn is_rust_boolean_tuple_or_array(node: &AstNode<'_>) -> bool {
    let kind = node.kind();
    if kind != "token_tree" && kind != "array_expression" && kind != "tuple_expression" {
        return false;
    }

    let items: Vec<AstNode<'_>> = node
        .children()
        .filter(|child| {
            let child_kind = child.kind();
            child_kind != "("
                && child_kind != ")"
                && child_kind != "["
                && child_kind != "]"
                && child_kind != ","
        })
        .collect();

    if items.len() < 2 {
        return false;
    }

    items.iter().all(|item| {
        let item_kind = item.kind();
        item_kind == "boolean_literal"
            || item.children().any(|child| child.kind() == "true" || child.kind() == "false")
    })
}

/// Splits the arguments inside a Rust macro invocation's `token_tree`.
fn extract_rust_macro_args<'a>(token_tree: &AstNode<'a>) -> Vec<AstNode<'a>> {
    let mut args = Vec::new();
    for child in token_tree.children() {
        let child_kind = child.kind();
        if child_kind != "("
            && child_kind != ")"
            && child_kind != "["
            && child_kind != "]"
            && child_kind != ","
        {
            args.push(child);
        }
    }
    args
}

/// Checks if a Python node represents a tuple or list consisting solely of boolean literals (>= 2 elements).
fn is_python_boolean_sequence(node: &AstNode<'_>) -> bool {
    let kind = node.kind();
    if kind != "tuple" && kind != "list" {
        return false;
    }

    let items: Vec<AstNode<'_>> = node
        .children()
        .filter(|child| {
            let child_kind = child.kind();
            child_kind != "("
                && child_kind != ")"
                && child_kind != "["
                && child_kind != "]"
                && child_kind != ","
        })
        .collect();

    if items.len() < 2 {
        return false;
    }

    items.iter().all(|item| {
        let item_kind = item.kind();
        item_kind == "true" || item_kind == "false"
    })
}

/// Recursively inspects a Rust test function body for packed assertions.
fn check_rust_node(node: &AstNode<'_>, diagnostics: &mut Vec<Diagnostic>, path: &Path) {
    let kind = node.kind();
    if kind == "function_item" {
        return;
    }

    if kind == "macro_invocation" && is_rust_assertion_macro(node) {
        let macro_name = rust_macro_terminal(node);
        let Some(token_tree) = node.children().find(|c| c.kind() == "token_tree") else {
            return;
        };

        // 1. Compound boolean condition: assert!(a && b)
        if (macro_name == "assert" || macro_name == "debug_assert")
            && has_rust_top_level_and(&token_tree)
        {
            diagnostics.push(NoAssertionPacking.diagnostic_at_node(
                path,
                node,
                &[("construct", "Compound boolean condition (`&&`)"), ("macro_name", &macro_name)],
            ));
            return;
        }

        // 2. Boolean tuple/array equality packing: assert_eq!((a, b), (true, true))
        if macro_name.starts_with("assert_") || macro_name.starts_with("debug_assert_") {
            let args = extract_rust_macro_args(&token_tree);
            let has_boolean_sequence = args.iter().any(is_rust_boolean_tuple_or_array);
            if has_boolean_sequence {
                diagnostics.push(NoAssertionPacking.diagnostic_at_node(
                    path,
                    node,
                    &[
                        ("construct", "Boolean tuple/collection equality"),
                        ("macro_name", &macro_name),
                    ],
                ));
                return;
            }
        }
    }

    for child in node.children() {
        check_rust_node(&child, diagnostics, path);
    }
}

/// Recursively inspects a Python test function body for packed assertions.
fn check_python_node(node: &AstNode<'_>, diagnostics: &mut Vec<Diagnostic>, path: &Path) {
    let kind = node.kind();
    if matches!(kind.as_ref(), "function_definition" | "class_definition") {
        return;
    }

    if kind == "assert_statement" {
        // 1. Compound boolean condition: assert a and b
        let has_and = node
            .children()
            .any(|c| c.kind() == "boolean_operator" && c.children().any(|op| op.kind() == "and"));

        if has_and {
            diagnostics.push(NoAssertionPacking.diagnostic_at_node(
                path,
                node,
                &[("construct", "Compound boolean condition (`and`)")],
            ));
            return;
        }

        // 2. Boolean tuple/list equality: assert (a, b) == (True, True)
        if let Some(comp) = node.children().find(|c| c.kind() == "comparison_operator") {
            let has_boolean_sequence = comp.children().any(|c| is_python_boolean_sequence(&c));
            if has_boolean_sequence {
                diagnostics.push(NoAssertionPacking.diagnostic_at_node(
                    path,
                    node,
                    &[("construct", "Boolean tuple/collection equality")],
                ));
                return;
            }
        }
    }

    for child in node.children() {
        check_python_node(&child, diagnostics, path);
    }
}

/// Collects all top-level functions matching `target_kind`.
fn collect_top_level_functions<'a>(
    node: &AstNode<'a>,
    target_kind: &str,
    out: &mut Vec<AstNode<'a>>,
) {
    if node.kind() == target_kind {
        out.push(node.clone());
        return;
    }
    for child in node.children() {
        collect_top_level_functions(&child, target_kind, out);
    }
}

impl CodeRule for NoAssertionPacking {
    fn target(&self) -> RuleTarget {
        RuleTarget::TestsOnly
    }

    fn check_file(
        &self,
        path: &Path,
        grep: &AstGrep<SourceDoc>,
        _config: &Config,
    ) -> Vec<Diagnostic> {
        let lang = *grep.lang();
        let func_kind = match lang {
            SupportLang::Rust => "function_item",
            _ => "function_definition",
        };

        let mut functions = Vec::new();
        collect_top_level_functions(&grep.root(), func_kind, &mut functions);

        let mut diagnostics = Vec::new();

        for func_node in functions {
            let Some(name_node) = func_node.field("name") else {
                continue;
            };
            let func_name = name_node.text();
            let is_test_fn = func_name == "test"
                || func_name.starts_with("test_")
                || (lang == SupportLang::Rust
                    && crate::code_lint::ast_rust::has_test_attribute(&func_node));

            if !is_test_fn {
                continue;
            }

            let Some(body_node) = func_node.field("body") else {
                continue;
            };

            match lang {
                SupportLang::Rust => check_rust_node(&body_node, &mut diagnostics, path),
                _ => check_python_node(&body_node, &mut diagnostics, path),
            }
        }

        diagnostics
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::Config;
    use indoc::indoc;

    fn run_test_rule(source: &str, file_name: &str) -> String {
        let config = Config::default();
        let lang =
            if Path::new(file_name).extension().is_some_and(|ext| ext.eq_ignore_ascii_case("rs")) {
                SupportLang::Rust
            } else {
                SupportLang::Python
            };
        let grep = AstGrep::new(source, lang);
        let diags = NoAssertionPacking.check_file(Path::new(file_name), &grep, &config);

        let mut lines = Vec::new();
        for diagnostic in diags {
            lines.push(format!(
                "[{}] Line {}, Col {}: {}",
                diagnostic.rule_name,
                diagnostic.location.line,
                diagnostic.location.column,
                diagnostic.message.summary
            ));
        }
        lines.join("\n")
    }

    #[test]
    fn test_rust_assertion_packing_flagged() {
        let source = indoc! {r"
            #[test]
            fn test_packed_assertions() {
                assert!(ready && connected);
                assert_eq!((valid, active), (true, true));
                assert_eq!((status, ready), (false, true));
                assert_eq!([first, second], [true, true]);
            }

            #[test]
            fn test_valid_assertions() {
                assert!(ready);
                assert!(connected);
                assert_eq!(count, 10);
                assert_eq!(coords, (10, 20));
                assert_eq!(flag, true);
                assert!(check_connection(ready && connected));
            }
        "};

        let output = run_test_rule(source, "tests/test_packing.rs");
        insta::assert_snapshot!(output, @"
        [no-assertion-packing] Line 3, Col 5: Compound boolean condition (`&&`) in `assert!` assertion.
        [no-assertion-packing] Line 4, Col 5: Boolean tuple/collection equality in `assert_eq!` assertion.
        [no-assertion-packing] Line 5, Col 5: Boolean tuple/collection equality in `assert_eq!` assertion.
        [no-assertion-packing] Line 6, Col 5: Boolean tuple/collection equality in `assert_eq!` assertion.
        ");
    }

    #[test]
    fn test_python_assertion_packing_flagged() {
        let source = indoc! {r"
            def test_packed():
                assert ready and connected
                assert (valid, active) == (True, True)
                assert (status, ready) == (False, True)
                assert [first, second] == [True, True]

            def test_valid():
                assert ready
                assert connected
                assert count == 10
                assert coords == (10, 20)
                assert flag == True
                assert check_connection(ready and connected)
        "};

        let output = run_test_rule(source, "tests/test_packing.py");
        insta::assert_snapshot!(output, @"
        [no-assertion-packing] Line 2, Col 5: Compound boolean condition (`and`) in `assert` statement.
        [no-assertion-packing] Line 3, Col 5: Boolean tuple/collection equality in `assert` statement.
        [no-assertion-packing] Line 4, Col 5: Boolean tuple/collection equality in `assert` statement.
        [no-assertion-packing] Line 5, Col 5: Boolean tuple/collection equality in `assert` statement.
        ");
    }
}
