//! Binding and enclosing-function extraction for Rust (`ra_ap_syntax`).

use super::SELF_KEYWORD;
use crate::code_lint::ast::{
    AstNode, Binding, BindingKind, EnclosingFunction, ParsedFile, push_bindings,
    span_from_rowan_range,
};
use ra_ap_syntax::ast::{self, HasName as _};
use ra_ap_syntax::{AstNode as _, SyntaxKind, SyntaxNode};

/// Returns `node` as an [`EnclosingFunction`] if it is a named function or method item; top level
/// if it is an item of the source file.
pub(super) fn named_function(node: &SyntaxNode) -> Option<EnclosingFunction> {
    let name = ast::Fn::cast(node.clone())?.name()?;
    Some(EnclosingFunction {
        name: name.text().to_string(),
        name_span: span_from_rowan_range(name.syntax().text_range()),
        is_top_level: node
            .parent()
            .is_some_and(|parent| ast::SourceFile::can_cast(parent.kind())),
        enclosing_class: None,
    })
}

/// Collects all binding definitions (variables, functions, structs, named struct fields, etc.)
/// within `file`, with what introduced each.
#[must_use]
pub(in crate::code_lint::ast) fn collect_bindings(file: &ParsedFile) -> Vec<Binding<'_>> {
    let Some(parsed) = file.rs_parsed() else {
        return Vec::new();
    };
    let mut bindings = Vec::new();
    traverse_rust(parsed.tree().syntax(), file, &mut bindings);
    bindings
}

fn traverse_rust<'a>(node: &SyntaxNode, file: &'a ParsedFile, bindings: &mut Vec<Binding<'a>>) {
    if let Some(use_item) = ast::Use::cast(node.clone()) {
        if let Some(use_tree) = use_item.use_tree() {
            push_bindings(bindings, BindingKind::Import, |out| {
                extract_from_use_tree(&use_tree, None, file, out);
            });
        }
        return;
    }
    if node.kind() == SyntaxKind::RECORD_FIELD {
        if let Some(name) = named_item_identifier(node) {
            bindings.push(Binding {
                node: AstNode::from_span(file, span_from_rowan_range(name.syntax().text_range())),
                kind: BindingKind::Value,
            });
        }
        return;
    }
    if let Some(pattern) = node_pattern(node) {
        let pattern_range = pattern.syntax().text_range();
        push_bindings(bindings, BindingKind::Value, |out| {
            extract_from_pattern(&pattern, file, out);
        });
        for child in node.children() {
            if child.text_range() != pattern_range {
                traverse_rust(&child, file, bindings);
            }
        }
        return;
    }
    if let Some(name) = named_item_identifier(node) {
        let name_range = name.syntax().text_range();
        bindings.push(Binding {
            node: AstNode::from_span(file, span_from_rowan_range(name_range)),
            kind: named_item_binding_kind(node),
        });
        for child in node.children() {
            if child.text_range() != name_range {
                traverse_rust(&child, file, bindings);
            }
        }
        return;
    }
    for child in node.children() {
        traverse_rust(&child, file, bindings);
    }
}

/// Recursively extracts bindings from a `use` tree in source order.
fn extract_from_use_tree<'a>(
    use_tree: &ast::UseTree,
    prefix_last_segment: Option<&ast::NameRef>,
    file: &'a ParsedFile,
    bindings: &mut Vec<AstNode<'a>>,
) {
    if let Some(rename) = use_tree.rename() {
        if let Some(name) = rename.name()
            && name.text() != "_"
        {
            bindings.push(AstNode::from_span(
                file,
                span_from_rowan_range(name.syntax().text_range()),
            ));
        }
        return;
    }
    if let Some(sub_trees) = use_tree.use_tree_list() {
        let last_segment = use_tree
            .path()
            .and_then(|path| path.segment())
            .and_then(|segment| segment.name_ref());
        for child_tree in sub_trees.use_trees() {
            extract_from_use_tree(&child_tree, last_segment.as_ref(), file, bindings);
        }
        return;
    }
    if use_tree.star_token().is_some() {
        return;
    }
    if let Some(path) = use_tree.path()
        && let Some(segment) = path.segment()
        && let Some(name_ref) = segment.name_ref()
    {
        if name_ref.text() == SELF_KEYWORD && path.qualifier().is_none() {
            if let Some(parent_segment) = prefix_last_segment {
                bindings.push(AstNode::from_span(
                    file,
                    span_from_rowan_range(parent_segment.syntax().text_range()),
                ));
            }
        } else if name_ref.text() != "_" {
            bindings.push(AstNode::from_span(
                file,
                span_from_rowan_range(name_ref.syntax().text_range()),
            ));
        }
    }
}

/// Extracts the pattern child of a pattern-binding syntax node (`let`, `for`, parameter, match arm).
fn node_pattern(node: &SyntaxNode) -> Option<ast::Pat> {
    match node.kind() {
        SyntaxKind::LET_STMT => ast::LetStmt::cast(node.clone())?.pat(),
        SyntaxKind::LET_EXPR => ast::LetExpr::cast(node.clone())?.pat(),
        SyntaxKind::FOR_EXPR => ast::ForExpr::cast(node.clone())?.pat(),
        SyntaxKind::PARAM => ast::Param::cast(node.clone())?.pat(),
        SyntaxKind::MATCH_ARM => ast::MatchArm::cast(node.clone())?.pat(),
        _ => None,
    }
}

/// Extracts binding identifiers in source order from a pattern subtree.
fn extract_from_pattern<'a>(
    pattern: &ast::Pat,
    file: &'a ParsedFile,
    bindings: &mut Vec<AstNode<'a>>,
) {
    for descendant in pattern.syntax().descendants() {
        if let Some(ident_pattern) = ast::IdentPat::cast(descendant)
            && let Some(name) = ident_pattern.name()
        {
            let text = name.text();
            if text
                .chars()
                .next()
                .is_some_and(|first_char| first_char.is_ascii_uppercase())
            {
                continue;
            }
            if text != "_" {
                bindings.push(AstNode::from_span(
                    file,
                    span_from_rowan_range(name.syntax().text_range()),
                ));
            }
        }
    }
}

/// Extracts the declared [`ast::Name`] of a named Rust item or record field declaration.
fn named_item_identifier(node: &SyntaxNode) -> Option<ast::Name> {
    match node.kind() {
        SyntaxKind::CONST => ast::Const::cast(node.clone())?.name(),
        SyntaxKind::STATIC => ast::Static::cast(node.clone())?.name(),
        SyntaxKind::FN => ast::Fn::cast(node.clone())?.name(),
        SyntaxKind::STRUCT => ast::Struct::cast(node.clone())?.name(),
        SyntaxKind::ENUM => ast::Enum::cast(node.clone())?.name(),
        SyntaxKind::TRAIT => ast::Trait::cast(node.clone())?.name(),
        SyntaxKind::TYPE_ALIAS => ast::TypeAlias::cast(node.clone())?.name(),
        SyntaxKind::RECORD_FIELD => ast::RecordField::cast(node.clone())?.name(),
        _ => None,
    }
}

/// Returns the kind of the binding declared by the named item `item`: a contract member if it is
/// a function, type alias, or constant defined in an `impl Trait for Type`, a structural
/// definition if it is a struct, enum, trait, type alias, or function, and a value otherwise.
fn named_item_binding_kind(item: &SyntaxNode) -> BindingKind {
    let is_trait_impl_member = matches!(
        item.kind(),
        SyntaxKind::FN | SyntaxKind::TYPE_ALIAS | SyntaxKind::CONST
    ) && item
        .parent()
        .filter(|assoc_items| ast::AssocItemList::can_cast(assoc_items.kind()))
        .and_then(|assoc_items| assoc_items.parent())
        .and_then(ast::Impl::cast)
        .is_some_and(|impl_item| impl_item.trait_().is_some());
    if is_trait_impl_member {
        return BindingKind::ContractMember;
    }
    match item.kind() {
        SyntaxKind::STRUCT
        | SyntaxKind::ENUM
        | SyntaxKind::TRAIT
        | SyntaxKind::TYPE_ALIAS
        | SyntaxKind::FN => BindingKind::StructuralDefinition,
        _ => BindingKind::Value,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagnostic::Language;

    #[test]
    fn test_collect_bindings_rust() {
        let source = indoc::indoc! {r"
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
        "};
        let file = ParsedFile::new(source, Language::Rust);
        let bindings = collect_bindings(&file);
        let names: Vec<String> = bindings
            .iter()
            .map(|binding| binding.node.text().to_string())
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
        let file = ParsedFile::new(source, Language::Rust);
        let bindings = collect_bindings(&file);
        let names: Vec<String> = bindings
            .iter()
            .map(|binding| binding.node.text().to_string())
            .collect();
        assert_eq!(names, vec!["main", "x"]);
    }
}
