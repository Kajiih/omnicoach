//! AST helper predicates for structural traversal in Rust.

use ast_grep_language::SupportLang;

/// Recursively extracts binding identifiers from a pattern node.
fn extract_from_pattern<'a>(
    node: &ast_grep_core::Node<'a, ast_grep_core::source::StrDoc<SupportLang>>,
    bindings: &mut Vec<ast_grep_core::Node<'a, ast_grep_core::source::StrDoc<SupportLang>>>,
) {
    let kind = node.kind();
    match kind.as_ref() {
        "identifier" => {
            // Exclude uppercase names (variants/constants like None, Ok, MAX)
            // which are type constructor references rather than variable bindings.
            if let Some(first_char) = node.text().chars().next() {
                if first_char.is_ascii_uppercase() {
                    return;
                }
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
                if let Some(ref struct_type) = type_node {
                    if struct_type.range() == child.range() {
                        continue;
                    }
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
                if let Some(ref condition) = cond {
                    if condition.range() == child.range() {
                        continue;
                    }
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
fn extract_last_segment<'a>(
    node: &ast_grep_core::Node<'a, ast_grep_core::source::StrDoc<SupportLang>>,
) -> Option<ast_grep_core::Node<'a, ast_grep_core::source::StrDoc<SupportLang>>> {
    match node.kind().as_ref() {
        "identifier" => Some(node.clone()),
        "scoped_identifier" => node.field("name"),
        _ => None,
    }
}

/// Recursively extracts bindings from a use declaration.
fn extract_from_use<'a>(
    node: &ast_grep_core::Node<'a, ast_grep_core::source::StrDoc<SupportLang>>,
    bindings: &mut Vec<ast_grep_core::Node<'a, ast_grep_core::source::StrDoc<SupportLang>>>,
    prefix_last_segment: Option<&ast_grep_core::Node<'a, ast_grep_core::source::StrDoc<SupportLang>>>,
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
            if let Some(last_seg) = extract_last_segment(node) {
                if last_seg.text() != "_" {
                    bindings.push(last_seg);
                }
            }
        }
        "use_as_clause" => {
            if let Some(alias) = node.field("alias") {
                if alias.text() != "_" {
                    bindings.push(alias);
                }
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

fn traverse_rust<'a>(
    node: &ast_grep_core::Node<'a, ast_grep_core::source::StrDoc<SupportLang>>,
    bindings: &mut Vec<ast_grep_core::Node<'a, ast_grep_core::source::StrDoc<SupportLang>>>,
) {
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
        "const_item"
        | "static_item"
        | "function_item"
        | "struct_item"
        | "enum_item"
        | "trait_item"
        | "type_item"
        | "associated_type" => {
            if let Some(name_node) = node.field("name") {
                bindings.push(name_node);
            }
            for child in node.children() {
                if let Some(name_node) = node.field("name") {
                    if child.range() == name_node.range() {
                        continue;
                    }
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
pub fn collect_bindings<'a>(
    root: &ast_grep_core::Node<'a, ast_grep_core::source::StrDoc<SupportLang>>,
) -> Vec<ast_grep_core::Node<'a, ast_grep_core::source::StrDoc<SupportLang>>> {
    let mut bindings = Vec::new();
    traverse_rust(root, &mut bindings);
    bindings
}

#[cfg(test)]
mod tests {
    use super::*;
    use ast_grep_core::AstGrep;

    #[test]
    fn test_collect_bindings_rust() {
        let source = r#"
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
        "#;
        let grep = AstGrep::new(source, SupportLang::Rust);
        let bindings = collect_bindings(&grep.root());
        let names: Vec<String> = bindings.iter().map(|node| node.text().to_string()).collect();
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
        let names: Vec<String> = bindings.iter().map(|node| node.text().to_string()).collect();
        assert_eq!(names, vec!["main", "x"]);
    }
}
