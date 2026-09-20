//! AST helper predicates for structural traversal in Rust.

use crate::code_lint::AstNode;

/// Recursively extracts binding identifiers from a pattern node.
fn extract_from_pattern<'a>(node: &AstNode<'a>, bindings: &mut Vec<AstNode<'a>>) {
    let kind = node.kind();
    match kind.as_ref() {
        "identifier" => {
            // Exclude uppercase names (variants/constants like None, Ok, MAX)
            // which are type constructor references rather than variable bindings.
            if let Some(first_char) = node.text().chars().next()
                && first_char.is_ascii_uppercase()
            {
                return;
            }
            if node.text() != "_" {
                bindings.push(node.clone());
            }
        }
        "shorthand_field_identifier" => {
            bindings.push(node.clone());
        }
        "struct_pattern" | "tuple_struct_pattern" => {
            let type_node = node.field("type");
            for child in node.children() {
                if let Some(ref struct_type) = type_node
                    && struct_type.range() == child.range()
                {
                    continue;
                }
                if child.kind() == "type_identifier" {
                    continue;
                }
                extract_from_pattern(&child, bindings);
            }
        }
        "field_pattern" => {
            for child in node.children() {
                if child.kind() != "field_identifier" && child.kind() != ":" {
                    extract_from_pattern(&child, bindings);
                }
            }
        }
        "match_pattern" => {
            let cond = node.field("condition");
            for child in node.children() {
                if let Some(ref condition) = cond
                    && condition.range() == child.range()
                {
                    continue;
                }
                if child.kind() == "if" {
                    continue;
                }
                extract_from_pattern(&child, bindings);
            }
        }
        _ => {
            for child in node.children() {
                extract_from_pattern(&child, bindings);
            }
        }
    }
}

/// Helper to extract the last segment identifier from a path node.
fn extract_last_segment<'a>(node: &AstNode<'a>) -> Option<AstNode<'a>> {
    match node.kind().as_ref() {
        "identifier" => Some(node.clone()),
        "scoped_identifier" => node.field("name"),
        _ => None,
    }
}

/// Recursively extracts bindings from a use declaration.
fn extract_from_use<'a>(
    node: &AstNode<'a>,
    bindings: &mut Vec<AstNode<'a>>,
    prefix_last_segment: Option<&AstNode<'a>>,
) {
    match node.kind().as_ref() {
        "use_declaration" => {
            for child in node.children() {
                if child.kind() != "use" && child.kind() != ";" {
                    extract_from_use(&child, bindings, None);
                }
            }
        }
        "identifier" => {
            if node.text() != "_" {
                bindings.push(node.clone());
            }
        }
        "self" => {
            if let Some(parent) = prefix_last_segment {
                bindings.push(parent.clone());
            }
        }
        "scoped_identifier" => {
            if let Some(last_seg) = extract_last_segment(node)
                && last_seg.text() != "_"
            {
                bindings.push(last_seg);
            }
        }
        "use_as_clause" => {
            if let Some(alias) = node.field("alias")
                && alias.text() != "_"
            {
                bindings.push(alias);
            }
        }
        "scoped_use_list" => {
            if let (Some(path), Some(list)) = (node.field("path"), node.field("list")) {
                let last_seg = extract_last_segment(&path);
                extract_from_use(&list, bindings, last_seg.as_ref());
            }
        }
        "use_list" => {
            for child in node.children() {
                let kind = child.kind();
                if kind != "{" && kind != "}" && kind != "," {
                    extract_from_use(&child, bindings, prefix_last_segment);
                }
            }
        }
        _ => {}
    }
}

fn traverse_rust<'a>(node: &AstNode<'a>, bindings: &mut Vec<AstNode<'a>>) {
    let kind = node.kind();
    match kind.as_ref() {
        "let_declaration" | "let_condition" => {
            if let Some(pattern) = node.field("pattern") {
                extract_from_pattern(&pattern, bindings);
            } else {
                let mut found_let = false;
                for child in node.children() {
                    if child.kind() == "let" {
                        found_let = true;
                        continue;
                    }
                    if found_let {
                        extract_from_pattern(&child, bindings);
                        break;
                    }
                }
            }
            for child in node.children() {
                traverse_rust(&child, bindings);
            }
        }
        "for_expression" => {
            if let Some(pattern) = node.field("pattern") {
                extract_from_pattern(&pattern, bindings);
            } else {
                let mut found_for = false;
                for child in node.children() {
                    if child.kind() == "for" {
                        found_for = true;
                        continue;
                    }
                    if found_for {
                        extract_from_pattern(&child, bindings);
                        break;
                    }
                }
            }
            for child in node.children() {
                traverse_rust(&child, bindings);
            }
        }
        "parameter" => {
            if let Some(pattern) = node.field("pattern") {
                extract_from_pattern(&pattern, bindings);
            }
            for child in node.children() {
                traverse_rust(&child, bindings);
            }
        }
        "closure_parameters" => {
            for child in node.children() {
                if child.kind() != "|" {
                    extract_from_pattern(&child, bindings);
                }
            }
        }
        "match_arm" => {
            for child in node.children() {
                if child.kind() == "match_pattern" {
                    extract_from_pattern(&child, bindings);
                } else if child.kind() != "=>" {
                    traverse_rust(&child, bindings);
                }
            }
        }
        "const_item" | "static_item" | "function_item" | "struct_item" | "enum_item"
        | "trait_item" | "type_item" | "associated_type" => {
            if let Some(name_node) = node.field("name") {
                bindings.push(name_node);
            }
            for child in node.children() {
                if let Some(name_node) = node.field("name")
                    && child.range() == name_node.range()
                {
                    continue;
                }
                traverse_rust(&child, bindings);
            }
        }
        "use_declaration" => {
            extract_from_use(node, bindings, None);
        }
        _ => {
            for child in node.children() {
                traverse_rust(&child, bindings);
            }
        }
    }
}

/// Collects all binding definitions (variables, functions, structs, etc.) within a node.
#[must_use]
pub fn collect_bindings<'a>(root: &AstNode<'a>) -> Vec<AstNode<'a>> {
    let mut bindings = Vec::new();
    traverse_rust(root, &mut bindings);
    bindings
}

/// Returns true if an `attribute_item` text represents a Rust test attribute
/// (`#[test]`, `#[tokio::test]`, `#[rstest]`, `#[test_case(...)]`).
#[must_use]
fn is_test_attribute(attr_text: &str) -> bool {
    let trimmed = attr_text
        .trim()
        .trim_start_matches("#[")
        .trim_end_matches(']');
    let attr_path = trimmed.split(['(', '=']).next().unwrap_or("").trim();
    let terminal = attr_path.rsplit("::").next().unwrap_or("").trim();
    matches!(terminal, "test" | "rstest" | "test_case")
}

/// Returns true if an `attribute_item` text represents a `#[cfg(test)]` attribute.
#[must_use]
fn is_conditional_test_attribute(attr_text: &str) -> bool {
    let trimmed = attr_text
        .trim()
        .trim_start_matches("#[")
        .trim_end_matches(']')
        .trim();
    trimmed
        .strip_prefix("cfg(")
        .and_then(|inner| inner.strip_suffix(')'))
        .is_some_and(|inner| {
            inner
                .split(|character: char| !character.is_ascii_alphanumeric() && character != '_')
                .any(|token| token == "test")
        })
}

/// Returns true if `node` is preceded by an `attribute_item` sibling matching `predicate`.
fn has_matching_attribute(node: &AstNode<'_>, predicate: fn(&str) -> bool) -> bool {
    std::iter::successors(node.prev(), AstNode::prev)
        .take_while(|sibling| {
            matches!(
                sibling.kind().as_ref(),
                "attribute_item" | "line_comment" | "block_comment"
            )
        })
        .filter(|sibling| sibling.kind() == "attribute_item")
        .any(|sibling| predicate(&sibling.text()))
}

/// Returns true if a Rust item is preceded by a test attribute (`#[test]`, `#[tokio::test]`, `#[rstest]`, etc.).
#[must_use]
fn has_test_attribute(node: &AstNode<'_>) -> bool {
    has_matching_attribute(node, is_test_attribute)
}

/// Returns true if a Rust item is preceded by a `#[cfg(test)]` attribute.
#[must_use]
fn has_conditional_test_attribute(node: &AstNode<'_>) -> bool {
    has_matching_attribute(node, is_conditional_test_attribute)
}

/// Collects byte spans for all inline test items (`#[cfg(test)]` modules/items and `#[test]` functions)
/// within a Rust source file.
#[must_use]
pub fn collect_inline_test_ranges(root: &AstNode<'_>) -> Vec<std::ops::Range<usize>> {
    let mut ranges = Vec::new();
    collect_inline_test_ranges_rec(root, &mut ranges);
    ranges
}

fn collect_inline_test_ranges_rec(node: &AstNode<'_>, ranges: &mut Vec<std::ops::Range<usize>>) {
    if has_conditional_test_attribute(node) || has_test_attribute(node) {
        ranges.push(node.range());
        return;
    }
    for child in node.children() {
        collect_inline_test_ranges_rec(&child, ranges);
    }
}

/// Returns true if a Rust `function_item` node is a test function (`#[test]` / `#[rstest]` or named `test` / `test_*`).
#[must_use]
pub fn is_test_function(func_node: &AstNode<'_>) -> bool {
    let is_named_test = func_node.field("name").is_some_and(|name_node| {
        let func_name = name_node.text();
        func_name == "test" || func_name.starts_with("test_")
    });
    is_named_test || has_test_attribute(func_node)
}

/// Extracts the terminal macro identifier from a Rust `macro_invocation` node (e.g. `assert` from `std::assert!`).
#[must_use]
pub fn macro_terminal_name<'tree>(macro_node: &AstNode<'tree>) -> std::borrow::Cow<'tree, str> {
    let Some(macro_id) = macro_node.field("macro") else {
        return std::borrow::Cow::Borrowed("");
    };
    let text = macro_id.text();
    match text {
        std::borrow::Cow::Borrowed(borrowed) => {
            std::borrow::Cow::Borrowed(borrowed.rsplit("::").next().unwrap_or("").trim())
        }
        std::borrow::Cow::Owned(owned) => {
            std::borrow::Cow::Owned(owned.rsplit("::").next().unwrap_or("").trim().to_string())
        }
    }
}

/// Returns true if a Rust `macro_invocation` node invokes an assertion macro
/// (`assert!`, `assert_*!`, `debug_assert!`, `debug_assert_*!`, `insta::assert_snapshot!`, etc.).
#[must_use]
pub fn is_assertion_macro(macro_node: &AstNode<'_>) -> bool {
    let terminal = macro_terminal_name(macro_node);
    terminal == "assert"
        || terminal.starts_with("assert_")
        || terminal == "debug_assert"
        || terminal.starts_with("debug_assert_")
}

#[cfg(test)]
mod tests {
    use super::*;
    use ast_grep_core::AstGrep;
    use ast_grep_language::SupportLang;

    #[test]
    fn test_collect_bindings_rust() {
        let source = r"
            use std;
            use std::collections::HashMap;
            use std::io::{self, Read};
            use std::sync::Arc as MyArc;
            use std::{io::{self as my_io, Write}, fs};
            use std::collections::*; // wildcard
            fn main() {
                let a = b;
                let mut c = 1;
                let (d, e) = (1, 2);
                let Point { x: f, y: _ } = p;
                let Point { g, h } = p;
                for i in 0..10 {}
                let f = |x: i32| x + 1;
                if let Some(y) = val {}
                while let Some(z) = val {}
                match val {
                    Some(w) if w > 0 => {}
                    None => {}
                }
            }
            fn my_func(j: i32) {}
            struct MyStruct;
            enum MyEnum { Variant }
            trait MyTrait {}
            type MyAlias = i32;
            trait Other {
                type MyAssoc;
            }
            const MY_CONST: i32 = 1;
            static MY_STATIC: i32 = 2;
        ";
        let grep = AstGrep::new(source, SupportLang::Rust);
        let bindings = collect_bindings(&grep.root());
        let names: Vec<String> = bindings
            .iter()
            .map(|node| node.text().to_string())
            .collect();
        assert_eq!(
            names,
            vec![
                "std",
                "HashMap",
                "io",
                "Read",
                "MyArc",
                "my_io",
                "Write",
                "fs",
                "main",
                "a",
                "c",
                "d",
                "e",
                "f",
                "g",
                "h",
                "i",
                "f",
                "x",
                "y",
                "z",
                "w",
                "my_func",
                "j",
                "MyStruct",
                "MyEnum",
                "MyTrait",
                "MyAlias",
                "Other",
                "MyAssoc",
                "MY_CONST",
                "MY_STATIC",
            ]
        );
    }

    #[test]
    fn test_collect_bindings_rust_negatives() {
        let source = "fn main() { let x: MyStruct = MyStruct; }";
        let grep = AstGrep::new(source, SupportLang::Rust);
        let bindings = collect_bindings(&grep.root());
        let names: Vec<String> = bindings
            .iter()
            .map(|node| node.text().to_string())
            .collect();
        assert_eq!(names, vec!["main", "x"]);
    }
}
