//! AST helper predicates and structural extractors for Rust (`ra_ap_syntax`).

use crate::code_lint::ast::{
    AstNode, Binding, BindingKind, EnclosingFunction, LiteralOccurrence, LiteralRole, LiteralValue,
    ParsedFile, PositionalRead, ScopePositionalReads, is_rust_comment_kind, parse_float_literal,
    parse_integer_literal, push_bindings, span_from_rowan_range,
};
use crate::diagnostic::SourceSpan;
use ra_ap_syntax::ast::{
    self, HasAttrs as _, HasGenericArgs as _, HasModuleItem as _, HasName as _, HasVisibility as _,
};
use ra_ap_syntax::{
    AstNode as _, AstToken as _, SyntaxElement, SyntaxKind, SyntaxNode, SyntaxToken,
};

/// Keyword identifier `"self"` in Rust `use` trees and expressions.
const SELF_KEYWORD: &str = "self";
/// Identifier `"test"` used in `#[test]`, `#[cfg(test)]`, and `test_*` functions.
const TEST_IDENTIFIER: &str = "test";
/// Rust 32-bit float literal suffix.
const FLOAT_SUFFIX_32: &str = "f32";
/// Rust 64-bit float literal suffix.
const FLOAT_SUFFIX_64: &str = "f64";
/// Unqualified name of `std::option::Option`.
const OPTION_TYPE_NAME: &str = "Option";
/// Unqualified name of `std::task::Poll`.
const POLL_TYPE_NAME: &str = "Poll";
/// Unqualified name of `std::boxed::Box`.
const BOX_TYPE_NAME: &str = "Box";
/// Unqualified name of `std::rc::Rc`.
const RC_TYPE_NAME: &str = "Rc";
/// Unqualified name of `std::sync::Arc`.
const ARC_TYPE_NAME: &str = "Arc";
/// Unqualified name of `std::borrow::Cow`.
const COW_TYPE_NAME: &str = "Cow";

/// Resolves the deepest [`SyntaxNode`] in `node`'s file covering `node.span()`.
fn syntax_node(node: &AstNode<'_>) -> Option<SyntaxNode> {
    let parsed = node.file.rs_parsed()?;
    let span = node.span();
    let start_offset = u32::try_from(span.start).ok()?;
    let end_offset = u32::try_from(span.end).ok()?;
    let range = ra_ap_syntax::TextRange::new(start_offset.into(), end_offset.into());
    match parsed.tree().syntax().covering_element(range) {
        SyntaxElement::Node(found) => Some(found),
        SyntaxElement::Token(token) => token.parent(),
    }
}

/// Casts the syntax node at `node.span()` (or an exact-range wrapper ancestor) to `T`.
fn cast_at_span<T: ra_ap_syntax::AstNode>(node: &AstNode<'_>) -> Option<T> {
    let inner = syntax_node(node)?;
    let target_range = inner.text_range();
    inner
        .ancestors()
        .take_while(|ancestor| ancestor.text_range() == target_range)
        .find_map(T::cast)
}

/// Returns `node` as an [`EnclosingFunction`] if it is a named function or method item; top level
/// if it is an item of the source file.
pub(super) fn named_function(node: &SyntaxNode) -> Option<EnclosingFunction> {
    let name = ast::Fn::cast(node.clone())?.name()?;
    Some(EnclosingFunction {
        name: name.text().to_string(),
        is_top_level: node
            .parent()
            .is_some_and(|parent| ast::SourceFile::can_cast(parent.kind())),
    })
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

/// Collects all binding definitions (variables, functions, structs, named struct fields, etc.)
/// within `file`, with what introduced each.
#[must_use]
pub(super) fn collect_bindings(file: &ParsedFile) -> Vec<Binding<'_>> {
    let Some(parsed) = file.rs_parsed() else {
        return Vec::new();
    };
    let mut bindings = Vec::new();
    traverse_rust(parsed.tree().syntax(), file, &mut bindings);
    bindings
}

/// Extracts the terminal identifier of an attribute's path (e.g. `"test"` for `#[tokio::test]`).
fn attribute_terminal_name(attribute: &ast::Attr) -> Option<ast::NameRef> {
    let path = attribute
        .path()
        .or_else(|| attribute.syntax().descendants().find_map(ast::Path::cast))?;
    path.segment()?.name_ref()
}

/// Returns true if `attribute` is a Rust test attribute (`#[test]`, `#[tokio::test]`, `#[rstest]`, `#[test_case(...)]`).
#[must_use]
fn is_test_attribute(attribute: &ast::Attr) -> bool {
    attribute_terminal_name(attribute)
        .is_some_and(|terminal| matches!(terminal.text(), TEST_IDENTIFIER | "rstest" | "test_case"))
}

/// Returns true if `element` is a delimiter or comma token inside a `TokenTree`.
fn is_token_tree_delimiter_or_trivia(element: &SyntaxElement) -> bool {
    match element {
        SyntaxElement::Node(_) => false,
        SyntaxElement::Token(token) => {
            token.kind().is_trivia()
                || matches!(
                    token.kind(),
                    SyntaxKind::L_PAREN
                        | SyntaxKind::R_PAREN
                        | SyntaxKind::L_BRACK
                        | SyntaxKind::R_BRACK
                        | SyntaxKind::L_CURLY
                        | SyntaxKind::R_CURLY
                        | SyntaxKind::COMMA
                )
        }
    }
}

/// Extracts the non-delimiter, non-trivia direct child elements of `token_tree`.
fn meaningful_token_tree_elements(token_tree: &ast::TokenTree) -> Vec<SyntaxElement> {
    token_tree
        .syntax()
        .children_with_tokens()
        .filter(|element| !is_token_tree_delimiter_or_trivia(element))
        .collect()
}

/// Returns true if `attribute` represents a `#[cfg(test)]` attribute.
#[must_use]
fn is_conditional_test_attribute(attribute: &ast::Attr) -> bool {
    let Some(ast::Meta::CfgMeta(conditional_meta)) = attribute.meta() else {
        return false;
    };
    let Some(ast::CfgPredicate::CfgAtom(atom)) = conditional_meta.cfg_predicate() else {
        return false;
    };
    atom.eq_token().is_none()
        && atom
            .ident_token()
            .is_some_and(|ident| ident.text() == TEST_IDENTIFIER)
}

/// Returns true if `attribute` represents a `#[doc = "..."]` attribute.
#[must_use]
fn is_doc_attribute(attribute: &ast::Attr) -> bool {
    attribute_terminal_name(attribute).is_some_and(|terminal| terminal.text() == "doc")
}

fn collect_inline_test_ranges_rec(node: &SyntaxNode, ranges: &mut Vec<std::ops::Range<usize>>) {
    let mut first_attribute_start: Option<usize> = None;
    let mut has_test_attr = false;
    for attribute in node.children().filter_map(ast::Attr::cast) {
        if attribute.kind() != ast::AttrKind::Outer {
            continue;
        }
        first_attribute_start.get_or_insert_with(|| attribute.syntax().text_range().start().into());
        if is_conditional_test_attribute(&attribute) || is_test_attribute(&attribute) {
            has_test_attr = true;
        }
    }
    if has_test_attr {
        let start = first_attribute_start.unwrap_or_else(|| node.text_range().start().into());
        let end: usize = node.text_range().end().into();
        ranges.push(start..end);
        return;
    }
    for child in node.children() {
        if child.kind() != SyntaxKind::TOKEN_TREE {
            collect_inline_test_ranges_rec(&child, ranges);
        }
    }
}

/// Collects byte spans for all inline test items (`#[cfg(test)]` modules/items and `#[test]` functions)
/// within a Rust source file.
#[must_use]
pub fn collect_inline_test_ranges(file: &ParsedFile) -> &[std::ops::Range<usize>] {
    file.rust_inline_test_ranges.get_or_init(|| {
        let Some(parsed) = file.rs_parsed() else {
            return Vec::new();
        };
        let mut ranges = Vec::new();
        collect_inline_test_ranges_rec(parsed.tree().syntax(), &mut ranges);
        ranges
    })
}

/// Returns true if `function` is a test function (`#[test]` / `#[rstest]` or named `test` / `test_*`).
#[must_use]
fn is_test_function(function: &ast::Fn) -> bool {
    let is_named_test = function.name().is_some_and(|name_node| {
        let function_name = name_node.text();
        function_name == TEST_IDENTIFIER || function_name.starts_with("test_")
    });
    let has_test_attr = function
        .attrs()
        .any(|attribute| attribute.kind() == ast::AttrKind::Outer && is_test_attribute(&attribute));
    is_named_test || has_test_attr
}

fn collect_outer_test_functions_rec(node: &SyntaxNode, out: &mut Vec<ast::Fn>) {
    if let Some(function) = ast::Fn::cast(node.clone()) {
        if is_test_function(&function) {
            out.push(function);
        }
        return;
    }
    for child in node.children() {
        collect_outer_test_functions_rec(&child, out);
    }
}

/// Extracts the terminal macro identifier from a [`ast::MacroCall`] node.
fn macro_call_terminal_name(macro_call: &ast::MacroCall) -> String {
    macro_call
        .path()
        .and_then(|path| path.segment())
        .and_then(|segment| segment.name_ref())
        .map_or_else(String::new, |name_ref| name_ref.text().trim().to_string())
}

/// Extracts the terminal macro identifier from a Rust `macro_invocation` node (e.g. `assert` from `std::assert!`).
#[must_use]
pub fn macro_terminal_name<'tree>(macro_node: &AstNode<'tree>) -> std::borrow::Cow<'tree, str> {
    cast_at_span::<ast::MacroCall>(macro_node)
        .map_or(std::borrow::Cow::Borrowed(""), |macro_call| {
            std::borrow::Cow::Owned(macro_call_terminal_name(&macro_call))
        })
}

/// Returns true if `macro_call` invokes an assertion macro (`assert!`, `assert_*!`, `debug_assert!`, `debug_assert_*!`).
#[must_use]
fn is_assertion_macro(macro_call: &ast::MacroCall) -> bool {
    let terminal = macro_call_terminal_name(macro_call);
    terminal == "assert"
        || terminal.starts_with("assert_")
        || terminal == "debug_assert"
        || terminal.starts_with("debug_assert_")
}

/// Recursively counts top-level assertion macro invocations in a Rust test function body.
fn count_rust_assertions(node: &SyntaxNode) -> usize {
    if ast::Fn::can_cast(node.kind()) {
        return 0;
    }
    if let Some(macro_call) = ast::MacroCall::cast(node.clone())
        && is_assertion_macro(&macro_call)
    {
        return 1;
    }
    node.children()
        .map(|child| count_rust_assertions(&child))
        .sum()
}

/// Collects all outermost Rust test functions together with their `(name_node, func_name, assertion_count)`.
#[must_use]
pub(super) fn collect_test_function_assertion_counts(
    file: &ParsedFile,
) -> Vec<(AstNode<'_>, String, usize)> {
    let Some(parsed) = file.rs_parsed() else {
        return Vec::new();
    };
    let mut test_functions = Vec::new();
    collect_outer_test_functions_rec(parsed.tree().syntax(), &mut test_functions);
    test_functions
        .into_iter()
        .filter_map(|function| {
            let name_node = function.name()?;
            let body_node = function.body()?;
            let function_name = name_node.text().to_string();
            let count = count_rust_assertions(body_node.syntax());
            Some((
                AstNode::from_span(file, span_from_rowan_range(name_node.syntax().text_range())),
                function_name,
                count,
            ))
        })
        .collect()
}

/// Collects all `macro_invocation` nodes in `file`.
#[must_use]
pub fn collect_macro_invocations(file: &ParsedFile) -> Vec<AstNode<'_>> {
    let Some(parsed) = file.rs_parsed() else {
        return Vec::new();
    };
    parsed
        .tree()
        .syntax()
        .descendants()
        .filter_map(ast::MacroCall::cast)
        .map(|macro_call| {
            AstNode::from_span(
                file,
                span_from_rowan_range(macro_call.syntax().text_range()),
            )
        })
        .collect()
}

/// Returns true if `token_tree` has a direct `&&` operator among its children.
fn token_tree_has_direct_logical_and(token_tree: &ast::TokenTree) -> bool {
    let elements: Vec<_> = token_tree.syntax().children_with_tokens().collect();
    for (index, element) in elements.iter().enumerate() {
        if let SyntaxElement::Token(token) = element {
            if token.kind() == SyntaxKind::AMP2 {
                return true;
            }
            if token.kind() == SyntaxKind::AMP
                && let Some(SyntaxElement::Token(next)) = elements.get(index + 1)
                && next.kind() == SyntaxKind::AMP
            {
                return true;
            }
        }
    }
    false
}

/// Returns true if a Rust `macro_invocation`'s `token_tree` contains a top-level `&&` logical operator.
#[must_use]
pub fn has_top_level_logical_and(macro_node: &AstNode<'_>) -> bool {
    let Some(macro_call) = cast_at_span::<ast::MacroCall>(macro_node) else {
        return false;
    };
    let Some(token_tree) = macro_call.token_tree() else {
        return false;
    };
    if token_tree_has_direct_logical_and(&token_tree) {
        return true;
    }
    let meaningful = meaningful_token_tree_elements(&token_tree);
    matches!(
        meaningful.as_slice(),
        [SyntaxElement::Node(only_child)]
            if ast::TokenTree::cast(only_child.clone())
                .is_some_and(|inner| token_tree_has_direct_logical_and(&inner))
    )
}

/// Extracts the argument nodes inside a Rust `macro_invocation`'s `token_tree`.
#[must_use]
pub fn extract_macro_arguments<'a>(macro_node: &AstNode<'a>) -> Vec<AstNode<'a>> {
    let Some(macro_call) = cast_at_span::<ast::MacroCall>(macro_node) else {
        return Vec::new();
    };
    let Some(token_tree) = macro_call.token_tree() else {
        return Vec::new();
    };
    meaningful_token_tree_elements(&token_tree)
        .into_iter()
        .map(|element| {
            AstNode::from_span(macro_node.file, span_from_rowan_range(element.text_range()))
        })
        .collect()
}

/// Returns true if `expression` is a boolean literal (`true` or `false`).
fn is_boolean_literal_expr(expression: &ast::Expr) -> bool {
    matches!(
        expression,
        ast::Expr::Literal(literal)
            if matches!(literal.kind(), ast::LiteralKind::Bool(_))
    )
}

/// Returns true if `node` is a Rust tuple, array, or parenthesized macro `token_tree` consisting
/// solely of `>= 2` boolean literals (`true` / `false`).
#[must_use]
pub fn is_boolean_literal_collection(node: &AstNode<'_>) -> bool {
    if let Some(token_tree) = cast_at_span::<ast::TokenTree>(node) {
        let items = meaningful_token_tree_elements(&token_tree);
        return items.len() >= 2
            && items.iter().all(|item| {
                matches!(
                    item,
                    SyntaxElement::Token(token)
                        if matches!(token.kind(), SyntaxKind::TRUE_KW | SyntaxKind::FALSE_KW)
                )
            });
    }
    if let Some(array_expr) = cast_at_span::<ast::ArrayExpr>(node) {
        let elements: Vec<_> = array_expr.exprs().collect();
        return elements.len() >= 2 && elements.iter().all(is_boolean_literal_expr);
    }
    if let Some(tuple_expr) = cast_at_span::<ast::TupleExpr>(node) {
        let elements: Vec<_> = tuple_expr.fields().collect();
        return elements.len() >= 2 && elements.iter().all(is_boolean_literal_expr);
    }
    false
}

/// Returns the previous non-trivia token before `token`.
fn previous_non_trivia_token(token: &SyntaxToken) -> Option<SyntaxToken> {
    std::iter::successors(token.prev_token(), SyntaxToken::prev_token)
        .find(|candidate| !candidate.kind().is_trivia())
}

/// Returns true if `kind` can be a segment of a Rust path (`ident`, `crate`, `self`, `super`).
const fn is_path_segment_token_kind(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::IDENT | SyntaxKind::CRATE_KW | SyntaxKind::SELF_KW | SyntaxKind::SUPER_KW
    )
}

/// Resolves `(full_path, terminal_name)` if `token_tree` is immediately preceded by `!` and a macro path
/// (such as `indoc! { ... }` or `indoc::indoc! { ... }` inside an outer `token_tree`).
fn resolve_preceding_macro_path(token_tree: &SyntaxNode) -> Option<(String, String)> {
    let first_token = token_tree.first_token()?;
    let bang = previous_non_trivia_token(&first_token)?;
    if bang.kind() != SyntaxKind::BANG {
        return None;
    }
    let macro_ident = previous_non_trivia_token(&bang)?;
    if !is_path_segment_token_kind(macro_ident.kind()) {
        return None;
    }
    let terminal = macro_ident.text().trim().to_string();
    let mut full_path = terminal.clone();
    let mut current = macro_ident;
    while let Some(preceding) = previous_non_trivia_token(&current) {
        let before_colons = if preceding.kind() == SyntaxKind::COLON2 {
            previous_non_trivia_token(&preceding)
        } else if preceding.kind() == SyntaxKind::COLON
            && let Some(first_colon) = previous_non_trivia_token(&preceding)
            && first_colon.kind() == SyntaxKind::COLON
        {
            previous_non_trivia_token(&first_colon)
        } else {
            break;
        };
        if let Some(prev_ident) = before_colons
            && is_path_segment_token_kind(prev_ident.kind())
        {
            full_path = format!("{prefix}::{full_path}", prefix = prev_ident.text().trim());
            current = prev_ident;
        } else {
            break;
        }
    }
    Some((full_path, terminal))
}

/// Returns true if `token` is preceded by `@` (an `insta` inline snapshot literal `@"..."`).
#[must_use]
fn is_insta_inline_snapshot(token: &SyntaxToken) -> bool {
    previous_non_trivia_token(token).is_some_and(|previous| previous.kind() == SyntaxKind::AT)
}

/// Returns true if `token` is enclosed inside a `#[doc = "..."]` attribute.
#[must_use]
fn is_enclosed_in_doc_attribute(token: &SyntaxToken) -> bool {
    token
        .parent_ancestors()
        .filter_map(ast::Attr::cast)
        .any(|attribute| is_doc_attribute(&attribute))
}

/// Returns true if `token` is enclosed in a Rust `macro_invocation` (or nested macro `token_tree`)
/// within the current scope whose `(full_path, terminal_name)` satisfies `predicate`.
#[must_use]
fn is_enclosed_in_macro(token: &SyntaxToken, predicate: &impl Fn(&str, &str) -> bool) -> bool {
    for ancestor in token.parent_ancestors() {
        if matches!(ancestor.kind(), SyntaxKind::FN | SyntaxKind::CLOSURE_EXPR) {
            break;
        }
        if let Some(macro_call) = ast::MacroCall::cast(ancestor.clone()) {
            let terminal = macro_call_terminal_name(&macro_call);
            let full_path = macro_call.path().map_or_else(String::new, |path| {
                compact_path_text(&path.syntax().text().to_string())
            });
            if predicate(&full_path, &terminal) {
                return true;
            }
        } else if ancestor.kind() == SyntaxKind::TOKEN_TREE
            && let Some((full_path, terminal)) = resolve_preceding_macro_path(&ancestor)
            && predicate(&full_path, &terminal)
        {
            return true;
        }
    }
    false
}

/// Returns true if `token` is a Rust string literal token that spans multiple lines and contains
/// runtime newlines (raw strings spanning lines, or standard strings with at least one intermediate
/// line not ending in a `\` line continuation).
#[must_use]
fn is_multiline_string_token(token: &SyntaxToken) -> bool {
    if !matches!(
        token.kind(),
        SyntaxKind::STRING | SyntaxKind::BYTE_STRING | SyntaxKind::C_STRING
    ) {
        return false;
    }
    let text = token.text();
    if !text.contains('\n') {
        return false;
    }
    if text.starts_with('r') || text.starts_with("br") || text.starts_with("cr") {
        return true;
    }
    let mut lines = text.lines();
    lines.next_back();
    lines.any(|line| !line.trim_end().ends_with('\\'))
}

/// Collects all Rust multiline string literal nodes in `file` that are not doc attributes,
/// `insta` inline snapshots, or enclosed in a macro matching `is_allowed_wrapper`.
#[must_use]
pub(super) fn find_unwrapped_multiline_strings(
    file: &ParsedFile,
    is_allowed_wrapper: impl Fn(&str, &str) -> bool,
) -> Vec<AstNode<'_>> {
    let Some(parsed) = file.rs_parsed() else {
        return Vec::new();
    };
    parsed
        .tree()
        .syntax()
        .descendants_with_tokens()
        .filter_map(SyntaxElement::into_token)
        .filter(|token| {
            is_multiline_string_token(token)
                && !is_insta_inline_snapshot(token)
                && !is_enclosed_in_doc_attribute(token)
                && !is_enclosed_in_macro(token, &is_allowed_wrapper)
        })
        .map(|token| AstNode::from_span(file, span_from_rowan_range(token.text_range())))
        .collect()
}

/// Visibility and name of a top-level external `mod <name>;` declaration in a Rust file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExternalModDeclaration {
    /// Module identifier (e.g. `"rust"` in `pub mod rust;`).
    pub name: String,
    /// Full trimmed source text of the declaration (e.g. `"pub mod rust;"`).
    pub declaration_text: String,
    /// True when the declaration has no `visibility_modifier` (`mod child;`).
    pub is_private: bool,
}

/// A referenced path in production Rust code (from a `use` tree or inline qualified path).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RustPathReference {
    /// 1-indexed starting line number of the reference or enclosing `use` declaration.
    pub line: usize,
    /// Raw syntactic path segments joined by `::` (e.g. `"crate::code_lint::ast::ParsedFile"`,
    /// `"super::AstNode"`, `"self::rust::collect_bindings"`, `"ra_ap_syntax::SyntaxNode"`).
    pub raw_path: String,
    /// Trimmed source text of the enclosing `use` declaration or inline path node for diagnostics.
    pub statement_text: String,
}

/// A visible `use` declaration (`pub use ...`, `pub(crate) use ...`) in production Rust code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VisibleUseDeclaration {
    /// 1-indexed starting line number.
    pub line: usize,
    /// Full trimmed source text (e.g. `"pub use self::child::Item;"`).
    pub declaration_text: String,
    /// Expanded target paths imported by this `use` tree (e.g. `["self::child::Item"]`).
    pub target_paths: Vec<String>,
}

/// Structural and dependency summary of the production code in a Rust source file,
/// extracted in a single CST pass while skipping `#[cfg(test)]` and `#[test]` subtrees.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RustFileSummary {
    /// Arguments of all `architecture_component!(...)` macro invocations in production code.
    pub architecture_components: Vec<String>,
    /// Top-level external `mod <name>;` declarations (`mod foo;`, `pub mod foo;`).
    pub external_mods: Vec<ExternalModDeclaration>,
    /// `(line, item_text)` of top-level production items that are neither external `mod` declarations
    /// nor `macro_rules!` definitions.
    pub non_namespace_items: Vec<(usize, String)>,
    /// `(line, item_text)` of top-level production `macro_rules!` definitions (`macro_definition`).
    pub macro_definitions: Vec<(usize, String)>,
    /// All `#[macro_export]` attribute texts in production code.
    pub macro_exports: Vec<String>,
    /// Visible `use` declarations (`pub use`, `pub(crate) use`, etc.) excluding `pub(crate) use <local_macro>;`.
    pub visible_uses: Vec<VisibleUseDeclaration>,
    /// All module/item paths referenced in production code (`use` trees, inline qualified paths,
    /// and `::`-qualified paths inside macro `token_tree` arguments).
    pub referenced_paths: Vec<RustPathReference>,
}

fn compact_path_text(text: &str) -> String {
    text.chars()
        .filter(|character| !character.is_whitespace())
        .collect()
}

fn join_use_prefix(prefix: &str, segment: &str) -> String {
    let clean = compact_path_text(segment);
    if prefix.is_empty() {
        clean
    } else if clean == SELF_KEYWORD {
        prefix.to_string()
    } else if let Some(rest) = clean.strip_prefix("self::") {
        format!("{prefix}::{rest}")
    } else {
        format!("{prefix}::{clean}")
    }
}

fn expand_use_tree(use_tree: &ast::UseTree, prefix: &str, out: &mut Vec<String>) {
    if let Some(sub_trees) = use_tree.use_tree_list() {
        let next_prefix = use_tree.path().map_or_else(
            || prefix.to_string(),
            |path| join_use_prefix(prefix, &path.syntax().text().to_string()),
        );
        for child_tree in sub_trees.use_trees() {
            expand_use_tree(&child_tree, &next_prefix, out);
        }
        return;
    }
    if use_tree.star_token().is_some() {
        if let Some(path) = use_tree.path() {
            out.push(join_use_prefix(prefix, &path.syntax().text().to_string()));
        } else if !prefix.is_empty() {
            out.push(prefix.to_string());
        }
        return;
    }
    if let Some(path) = use_tree.path() {
        let path_text = path.syntax().text().to_string();
        if compact_path_text(&path_text) != "_" {
            out.push(join_use_prefix(prefix, &path_text));
        }
    }
}

fn is_simple_path_segment(segment: &ast::PathSegment) -> bool {
    segment.name_ref().is_some()
        && segment.type_anchor().is_none()
        && segment.generic_arg_list().is_none()
        && segment.parenthesized_arg_list().is_none()
        && segment.ret_type().is_none()
}

fn has_pure_qualifiers(path: &ast::Path) -> bool {
    let Some(qualifier) = path.qualifier() else {
        return false;
    };
    let mut current = Some(qualifier);
    while let Some(qual_path) = current {
        let Some(segment) = qual_path.segment() else {
            return false;
        };
        if !is_simple_path_segment(&segment) {
            return false;
        }
        current = qual_path.qualifier();
    }
    path.segment()
        .is_some_and(|segment| segment.name_ref().is_some() && segment.type_anchor().is_none())
}

/// Checks if `elements[index..]` begins with `::` (either `COLON2` or two adjacent `COLON` tokens),
/// returning the index immediately after `::`.
fn consume_double_colon(elements: &[SyntaxElement], index: usize) -> Option<usize> {
    match elements.get(index)? {
        SyntaxElement::Token(token) if token.kind() == SyntaxKind::COLON2 => Some(index + 1),
        SyntaxElement::Token(first) if first.kind() == SyntaxKind::COLON => {
            if let Some(SyntaxElement::Token(second)) = elements.get(index + 1)
                && second.kind() == SyntaxKind::COLON
            {
                Some(index + 2)
            } else {
                None
            }
        }
        _ => None,
    }
}

fn token_segment_at(elements: &[SyntaxElement], index: usize) -> Option<&SyntaxToken> {
    match elements.get(index)? {
        SyntaxElement::Token(token) if is_path_segment_token_kind(token.kind()) => Some(token),
        _ => None,
    }
}

fn collect_token_tree_paths(
    token_tree: &ast::TokenTree,
    file: &ParsedFile,
    statement_text: &str,
    out: &mut Vec<RustPathReference>,
) {
    let elements: Vec<SyntaxElement> = token_tree
        .syntax()
        .children_with_tokens()
        .filter(
            |element| !matches!(element, SyntaxElement::Token(token) if token.kind().is_trivia()),
        )
        .collect();
    let mut index = 0;
    while index < elements.len() {
        if let SyntaxElement::Node(child_node) = &elements[index]
            && let Some(nested_tree) = ast::TokenTree::cast(child_node.clone())
        {
            collect_token_tree_paths(&nested_tree, file, statement_text, out);
            index += 1;
            continue;
        }
        if let Some(first_token) = token_segment_at(&elements, index)
            && let Some(after_colons) = consume_double_colon(&elements, index + 1)
            && token_segment_at(&elements, after_colons).is_some()
        {
            let line = file
                .line_index
                .line(first_token.text_range().start().into());
            let mut segments = vec![first_token.text().trim().to_string()];
            let mut cursor = index + 1;
            while let Some(next_segment_idx) = consume_double_colon(&elements, cursor)
                && let Some(segment_token) = token_segment_at(&elements, next_segment_idx)
            {
                segments.push(segment_token.text().trim().to_string());
                cursor = next_segment_idx + 1;
            }
            out.push(RustPathReference {
                line,
                raw_path: segments.join("::"),
                statement_text: statement_text.to_string(),
            });
            index = cursor;
            continue;
        }
        index += 1;
    }
}

/// Returns `(start_line, trimmed_declaration_text)` of `node`, starting after any leading
/// outer attributes, doc comments, and whitespace.
fn item_line_and_text(file: &ParsedFile, node: &SyntaxNode) -> (usize, String) {
    let start_offset: usize = node
        .children_with_tokens()
        .find(|element| {
            let kind = element.kind();
            kind != SyntaxKind::ATTR
                && kind != SyntaxKind::WHITESPACE
                && !is_rust_comment_kind(kind)
        })
        .map_or_else(
            || node.text_range().start().into(),
            |element| element.text_range().start().into(),
        );
    let end_offset: usize = node.text_range().end().into();
    let line = file.line_index.line(start_offset);
    let text = file.source[start_offset..end_offset].trim().to_string();
    (line, text)
}

fn summarize_use_item(
    use_item: &ast::Use,
    file: &ParsedFile,
    defined_macros: &std::collections::HashSet<String>,
    summary: &mut RustFileSummary,
) {
    for attribute in use_item.attrs() {
        summarize_rust_node(attribute.syntax(), file, defined_macros, summary);
    }
    let (line, declaration_text) = item_line_and_text(file, use_item.syntax());
    let mut target_paths = Vec::new();
    if let Some(use_tree) = use_item.use_tree() {
        expand_use_tree(&use_tree, "", &mut target_paths);
    }
    for raw_path in &target_paths {
        summary.referenced_paths.push(RustPathReference {
            line,
            raw_path: raw_path.clone(),
            statement_text: declaration_text.clone(),
        });
    }
    if let Some(visibility) = use_item.visibility() {
        let is_own_macro_path = visibility.syntax().text().to_string().trim() == "pub(crate)"
            && matches!(
                target_paths.as_slice(),
                [target] if defined_macros.contains(target.as_str())
            );
        if !is_own_macro_path {
            summary.visible_uses.push(VisibleUseDeclaration {
                line,
                declaration_text,
                target_paths,
            });
        }
    }
}

fn summarize_qualified_path(
    path: &ast::Path,
    file: &ParsedFile,
    defined_macros: &std::collections::HashSet<String>,
    summary: &mut RustFileSummary,
) {
    let parent_is_pure = path
        .syntax()
        .parent()
        .and_then(ast::Path::cast)
        .is_some_and(|parent_path| has_pure_qualifiers(&parent_path));
    if !parent_is_pure
        && let Some(segment) = path.segment()
        && let Some(name_ref) = segment.name_ref()
    {
        let start_offset: usize = path.syntax().text_range().start().into();
        let end_offset: usize = name_ref.syntax().text_range().end().into();
        let slice_text = file.source[start_offset..end_offset].trim();
        summary.referenced_paths.push(RustPathReference {
            line: file.line_index.line(start_offset),
            raw_path: compact_path_text(slice_text),
            statement_text: slice_text.to_string(),
        });
        if let Some(generic_args) = segment.generic_arg_list() {
            summarize_rust_node(generic_args.syntax(), file, defined_macros, summary);
        }
        if let Some(paren_args) = segment.parenthesized_arg_list() {
            summarize_rust_node(paren_args.syntax(), file, defined_macros, summary);
        }
        if let Some(ret_type) = segment.ret_type() {
            summarize_rust_node(ret_type.syntax(), file, defined_macros, summary);
        }
    }
}

fn summarize_rust_node(
    node: &SyntaxNode,
    file: &ParsedFile,
    defined_macros: &std::collections::HashSet<String>,
    summary: &mut RustFileSummary,
) {
    let start_offset: usize = node.text_range().start().into();
    if file.is_in_rust_inline_test(start_offset) {
        return;
    }

    if let Some(attribute) = ast::Attr::cast(node.clone()) {
        if attribute.kind() == ast::AttrKind::Outer
            && attribute_terminal_name(&attribute)
                .is_some_and(|terminal| terminal.text() == "macro_export")
        {
            summary.macro_exports.push("#[macro_export]".to_string());
        }
        for child in node.children() {
            if child.kind() != SyntaxKind::TOKEN_TREE {
                summarize_rust_node(&child, file, defined_macros, summary);
            }
        }
        return;
    }

    if let Some(use_item) = ast::Use::cast(node.clone()) {
        summarize_use_item(&use_item, file, defined_macros, summary);
        return;
    }

    if let Some(macro_rules) = ast::MacroRules::cast(node.clone()) {
        for attribute in macro_rules.attrs() {
            summarize_rust_node(attribute.syntax(), file, defined_macros, summary);
        }
        return;
    }

    if let Some(macro_call) = ast::MacroCall::cast(node.clone()) {
        let macro_ast_node = AstNode::from_span(
            file,
            span_from_rowan_range(macro_call.syntax().text_range()),
        );
        if macro_call_terminal_name(&macro_call) == "architecture_component" {
            let component_argument: String = extract_macro_arguments(&macro_ast_node)
                .into_iter()
                .map(|argument| argument.text().into_owned())
                .collect();
            summary.architecture_components.push(component_argument);
        }
        if let Some(token_tree) = macro_call.token_tree() {
            let (_, statement_text) = item_line_and_text(file, node);
            collect_token_tree_paths(
                &token_tree,
                file,
                &statement_text,
                &mut summary.referenced_paths,
            );
        }
    }

    if let Some(path) = ast::Path::cast(node.clone())
        && has_pure_qualifiers(&path)
    {
        summarize_qualified_path(&path, file, defined_macros, summary);
        return;
    }

    for child in node.children() {
        if child.kind() != SyntaxKind::TOKEN_TREE {
            summarize_rust_node(&child, file, defined_macros, summary);
        }
    }
}

/// Extracts a complete structural and dependency summary of the production code in `file`
/// in a single CST pass, skipping `#[cfg(test)]` and `#[test]` items.
#[must_use]
pub fn summarize_rust_file(file: &ParsedFile) -> RustFileSummary {
    let Some(parsed) = file.rs_parsed() else {
        return RustFileSummary::default();
    };
    let source_file = parsed.tree();
    let mut summary = RustFileSummary::default();
    let mut defined_macros = std::collections::HashSet::new();

    for item in source_file.items() {
        let start_offset: usize = item.syntax().text_range().start().into();
        if file.is_in_rust_inline_test(start_offset) {
            continue;
        }
        let (item_line, declaration_text) = item_line_and_text(file, item.syntax());
        match &item {
            ast::Item::Module(module) if module.item_list().is_none() => {
                if let Some(name_node) = module.name() {
                    let is_private = module.visibility().is_none();
                    summary.external_mods.push(ExternalModDeclaration {
                        name: name_node.text().trim().to_string(),
                        declaration_text,
                        is_private,
                    });
                }
            }
            ast::Item::MacroRules(macro_rules) => {
                if let Some(name_node) = macro_rules.name() {
                    defined_macros.insert(name_node.text().to_string());
                }
                summary
                    .macro_definitions
                    .push((item_line, declaration_text));
            }
            _ => {
                summary
                    .non_namespace_items
                    .push((item_line, declaration_text));
            }
        }
    }

    for child in source_file.syntax().children() {
        summarize_rust_node(&child, file, &defined_macros, &mut summary);
    }

    summary
}

/// Returns true if `receiver` names a stable value: a single-segment name/`self` or field chain
/// with no call inside, so that identical text means the same value.
fn is_stable_receiver(receiver: &ast::Expr) -> bool {
    let is_name_or_field = match receiver {
        ast::Expr::PathExpr(path_expr) => path_expr
            .path()
            .is_some_and(|path| path.qualifier().is_none()),
        ast::Expr::FieldExpr(_) => true,
        _ => false,
    };
    is_name_or_field
        && !receiver.syntax().descendants().any(|descendant| {
            matches!(
                descendant.kind(),
                SyntaxKind::CALL_EXPR | SyntaxKind::METHOD_CALL_EXPR
            )
        })
}

/// Returns true if `node` is assigned to (`t.0 = x`, `t.0 += x`, `(t.0, t.1) = (b, a)`) or
/// mutably borrowed (`&mut t.0`).
fn is_mutated(node: &SyntaxNode) -> bool {
    let mut place = node.clone();
    while let Some(parent) = place.parent() {
        match parent.kind() {
            SyntaxKind::TUPLE_EXPR | SyntaxKind::PAREN_EXPR => place = parent,
            SyntaxKind::BIN_EXPR => {
                let Some(bin_expr) = ast::BinExpr::cast(parent) else {
                    return false;
                };
                if !matches!(bin_expr.op_kind(), Some(ast::BinaryOp::Assignment { .. })) {
                    return false;
                }
                return bin_expr
                    .lhs()
                    .is_some_and(|left| left.syntax().text_range() == place.text_range());
            }
            SyntaxKind::REF_EXPR => {
                return ast::RefExpr::cast(parent)
                    .is_some_and(|ref_expr| ref_expr.mut_token().is_some());
            }
            _ => return false,
        }
    }
    false
}

/// Records a tuple-field access (`span.0`) as a positional read, or its receiver as an exempt
/// receiver when the field is mutated.
fn record_field_expression<'a>(
    field_expr: &ast::FieldExpr,
    file: &'a ParsedFile,
    scope: &mut ScopePositionalReads<'a>,
) {
    let (Some(receiver), Some(field_name)) = (field_expr.expr(), field_expr.name_ref()) else {
        return;
    };
    if field_name
        .syntax()
        .first_token()
        .is_none_or(|token| token.kind() != SyntaxKind::INT_NUMBER)
    {
        return;
    }
    let receiver_span = span_from_rowan_range(receiver.syntax().text_range());
    let receiver_text = file.source[receiver_span.start..receiver_span.end].to_string();
    if is_mutated(field_expr.syntax()) {
        scope.exempt_receivers.insert(receiver_text);
    } else if is_stable_receiver(&receiver)
        && let Ok(position) = field_name.text().parse()
    {
        scope.reads.push(PositionalRead {
            node: AstNode::from_span(
                file,
                span_from_rowan_range(field_expr.syntax().text_range()),
            ),
            receiver: receiver_text,
            position,
        });
    }
}

/// Walks `node`, recording reads and mutations into the enclosing function's `scope` (none
/// outside functions; closures belong to it) and pushing each finished function scope to `out`.
fn collect_positional_reads_rec<'a>(
    node: &SyntaxNode,
    file: &'a ParsedFile,
    mut scope: Option<&mut ScopePositionalReads<'a>>,
    out: &mut Vec<ScopePositionalReads<'a>>,
) {
    if ast::Fn::can_cast(node.kind()) {
        let mut function_scope = ScopePositionalReads::default();
        for child in node.children() {
            collect_positional_reads_rec(&child, file, Some(&mut function_scope), out);
        }
        out.push(function_scope);
        return;
    }
    if let Some(field_expr) = ast::FieldExpr::cast(node.clone())
        && let Some(active_scope) = scope.as_deref_mut()
    {
        record_field_expression(&field_expr, file, active_scope);
    }
    for child in node.children() {
        collect_positional_reads_rec(&child, file, scope.as_deref_mut(), out);
    }
}

/// Collects Rust tuple-field reads grouped by function (see [`super::collect_positional_reads`]).
#[must_use]
pub(super) fn collect_positional_reads(file: &ParsedFile) -> Vec<ScopePositionalReads<'_>> {
    let Some(parsed) = file.rs_parsed() else {
        return Vec::new();
    };
    let mut out = Vec::new();
    collect_positional_reads_rec(parsed.tree().syntax(), file, None, &mut out);
    out
}

/// Macros whose literal arguments the language or API requires as written: format strings,
/// assertion and logging messages, and compile-time inputs. Matched on the last path segment.
const LITERAL_EXEMPT_MACROS: &[&str] = &[
    "format",
    "print",
    "println",
    "eprint",
    "eprintln",
    "write",
    "writeln",
    "panic",
    "todo",
    "unimplemented",
    "unreachable",
    "assert",
    "assert_eq",
    "assert_ne",
    "debug_assert",
    "debug_assert_eq",
    "debug_assert_ne",
    "bail",
    "ensure",
    "anyhow",
    "error",
    "warn",
    "info",
    "debug",
    "trace",
    "indoc",
    "concat",
    "env",
    "option_env",
    "include_str",
    "include_bytes",
];

/// Returns the value of a Rust integer literal, resolving its type suffix (`30u64`, `0xffu8`);
/// a decimal literal with an `f32` / `f64` suffix (`1f32`) is a float.
fn integer_literal_value(text: &str) -> Option<LiteralValue> {
    let is_hex = text.starts_with("0x") || text.starts_with("0X");
    let digits_start = if is_hex
        || text.starts_with("0o")
        || text.starts_with("0O")
        || text.starts_with("0b")
        || text.starts_with("0B")
    {
        2
    } else {
        0
    };
    let suffix_start = text[digits_start..]
        .find(|character: char| {
            if is_hex {
                matches!(character, 'u' | 'i')
            } else {
                character.is_ascii_alphabetic()
            }
        })
        .map_or(text.len(), |offset| digits_start + offset);
    let (number, suffix) = text.split_at(suffix_start);
    if matches!(suffix, FLOAT_SUFFIX_32 | FLOAT_SUFFIX_64) {
        parse_float_literal(number)
    } else {
        parse_integer_literal(number)
    }
}

/// Decodes a literal [`SyntaxToken`] (`STRING`, `BYTE_STRING`, `INT_NUMBER`, `FLOAT_NUMBER`)
/// into its [`LiteralValue`], using `ra_ap_syntax`'s native escape decoder for strings and byte strings.
fn token_literal_value(token: &SyntaxToken) -> Option<LiteralValue> {
    if let Some(string_token) = ast::String::cast(token.clone()) {
        let decoded = string_token.value().ok()?;
        return Some(LiteralValue::Str(decoded.into_owned()));
    }
    if let Some(byte_string_token) = ast::ByteString::cast(token.clone()) {
        let decoded = byte_string_token.value().ok()?;
        return Some(LiteralValue::Bytes(
            String::from_utf8_lossy(&decoded).into_owned(),
        ));
    }
    match token.kind() {
        SyntaxKind::INT_NUMBER => integer_literal_value(token.text()),
        SyntaxKind::FLOAT_NUMBER => {
            let text = token.text();
            let number = text
                .strip_suffix(FLOAT_SUFFIX_32)
                .or_else(|| text.strip_suffix(FLOAT_SUFFIX_64))
                .unwrap_or(text);
            parse_float_literal(number)
        }
        _ => None,
    }
}

/// Evaluates an [`ast::Literal`], negative [`ast::PrefixExpr`], or [`ast::LiteralPat`] node
/// to its [`LiteralValue`].
fn node_literal_value(node: &SyntaxNode) -> Option<LiteralValue> {
    if let Some(literal) = ast::Literal::cast(node.clone()) {
        let token = literal.token();
        if !matches!(
            token.kind(),
            SyntaxKind::STRING
                | SyntaxKind::BYTE_STRING
                | SyntaxKind::INT_NUMBER
                | SyntaxKind::FLOAT_NUMBER
        ) {
            return None;
        }
        let value = token_literal_value(&token)?;
        let has_leading_minus = node
            .children_with_tokens()
            .filter_map(SyntaxElement::into_token)
            .any(|child_tok| child_tok.kind() == SyntaxKind::MINUS);
        return if has_leading_minus {
            value.negated()
        } else {
            Some(value)
        };
    }
    if let Some(prefix_expr) = ast::PrefixExpr::cast(node.clone()) {
        if prefix_expr.op_kind() != Some(ast::UnaryOp::Neg) {
            return None;
        }
        let ast::Expr::Literal(inner_literal) = prefix_expr.expr()? else {
            return None;
        };
        let token = inner_literal.token();
        if !matches!(
            token.kind(),
            SyntaxKind::INT_NUMBER | SyntaxKind::FLOAT_NUMBER
        ) {
            return None;
        }
        return token_literal_value(&token)?.negated();
    }
    if let Some(literal_pat) = ast::LiteralPat::cast(node.clone()) {
        let inner_literal = literal_pat.literal()?;
        let token = inner_literal.token();
        let value = token_literal_value(&token)?;
        return if literal_pat.minus_token().is_some() {
            value.negated()
        } else {
            Some(value)
        };
    }
    None
}

/// Returns true if `name` (a macro's last path segment) is in [`LITERAL_EXEMPT_MACROS`].
fn is_literal_exempt_macro(name: &str) -> bool {
    LITERAL_EXEMPT_MACROS.contains(&name)
}

/// Returns the initializer expression node of a scalar constant (`const`, non-`mut` `static`, or `enum` variant).
fn constant_initializer_expr(node: &SyntaxNode) -> Option<(bool, Option<ast::Expr>)> {
    if let Some(const_item) = ast::Const::cast(node.clone()) {
        return Some((true, const_item.body()));
    }
    if let Some(variant) = ast::Variant::cast(node.clone()) {
        return Some((
            true,
            variant
                .const_arg()
                .and_then(|const_arg| const_arg.expr())
                .or_else(|| variant.syntax().children().find_map(ast::Expr::cast)),
        ));
    }
    if let Some(static_item) = ast::Static::cast(node.clone()) {
        if static_item.mut_token().is_none() {
            return Some((true, static_item.body()));
        }
        return Some((false, None));
    }
    None
}

/// Walks `node`, pushing collectable literals to `out` (see [`super::collect_literal_occurrences`]).
fn collect_literal_occurrences_rec<'a>(
    node: &SyntaxNode,
    file: &'a ParsedFile,
    out: &mut Vec<LiteralOccurrence<'a>>,
) {
    let kind = node.kind();
    if ast::Attr::can_cast(kind) || ast::Abi::can_cast(kind) {
        return;
    }
    if let Some(macro_call) = ast::MacroCall::cast(node.clone())
        && is_literal_exempt_macro(&macro_call_terminal_name(&macro_call))
    {
        return;
    }
    if kind == SyntaxKind::TOKEN_TREE {
        if resolve_preceding_macro_path(node)
            .is_some_and(|(_, terminal)| is_literal_exempt_macro(&terminal))
        {
            return;
        }
        let mut follows_minus = false;
        for element in node.children_with_tokens() {
            match element {
                SyntaxElement::Node(child_node) => {
                    follows_minus = false;
                    collect_literal_occurrences_rec(&child_node, file, out);
                }
                SyntaxElement::Token(token) => {
                    if token.kind().is_trivia() {
                        continue;
                    }
                    let is_signed_number = follows_minus
                        && matches!(
                            token.kind(),
                            SyntaxKind::INT_NUMBER | SyntaxKind::FLOAT_NUMBER
                        );
                    follows_minus = token.kind() == SyntaxKind::MINUS;
                    if !is_signed_number && let Some(value) = token_literal_value(&token) {
                        out.push(LiteralOccurrence {
                            node: AstNode::from_span(
                                file,
                                span_from_rowan_range(token.text_range()),
                            ),
                            value,
                            role: LiteralRole::Inline,
                        });
                    }
                }
            }
        }
        return;
    }
    if let Some(field_expr) = ast::FieldExpr::cast(node.clone()) {
        if let Some(receiver) = field_expr.expr() {
            collect_literal_occurrences_rec(receiver.syntax(), file, out);
        }
        return;
    }
    if let Some((true, initializer)) = constant_initializer_expr(node) {
        if let Some(expr_node) = initializer
            && let Some(value) = node_literal_value(expr_node.syntax())
        {
            out.push(LiteralOccurrence {
                node: AstNode::from_span(
                    file,
                    span_from_rowan_range(expr_node.syntax().text_range()),
                ),
                value,
                role: LiteralRole::ConstantDefinition,
            });
        }
        return;
    }
    if let Some(value) = node_literal_value(node) {
        out.push(LiteralOccurrence {
            node: AstNode::from_span(file, span_from_rowan_range(node.text_range())),
            value,
            role: LiteralRole::Inline,
        });
        return;
    }
    for child in node.children() {
        collect_literal_occurrences_rec(&child, file, out);
    }
}

/// Collects Rust literal occurrences (see [`super::collect_literal_occurrences`]).
#[must_use]
pub(super) fn collect_literal_occurrences(file: &ParsedFile) -> Vec<LiteralOccurrence<'_>> {
    let Some(parsed) = file.rs_parsed() else {
        return Vec::new();
    };
    let mut out = Vec::new();
    collect_literal_occurrences_rec(parsed.tree().syntax(), file, &mut out);
    out
}

/// Normalizes a raw type path string by stripping whitespace and any leading `::`,
/// returning `(normalized_path, terminal_identifier)`.
fn normalize_type_path_text(raw_text: &str) -> Option<(String, String)> {
    let compact = compact_path_text(raw_text);
    if compact.is_empty() || compact.contains('<') || compact.contains('[') || compact.contains('(')
    {
        return None;
    }
    let normalized = compact
        .strip_prefix("::")
        .unwrap_or(compact.as_str())
        .to_owned();
    let terminal = normalized
        .rsplit("::")
        .next()
        .unwrap_or(normalized.as_str())
        .to_owned();
    Some((normalized, terminal))
}

/// Extracts `(base_span, (normalized_base_path, base_terminal), type_argument_nodes)` from an [`ast::PathType`]
/// whose terminal segment has a `<...>` generic argument list, excluding lifetime and associated-type arguments.
fn extract_path_type_generics(
    path_type: &ast::PathType,
    source: &str,
) -> Option<(SourceSpan, (String, String), Vec<ast::Type>)> {
    let path = path_type.path()?;
    let segment = path.segment()?;
    let name_ref = segment.name_ref()?;
    let generic_args = segment.generic_arg_list()?;
    let base_start: usize = path.syntax().text_range().start().into();
    let base_end: usize = name_ref.syntax().text_range().end().into();
    let base_span = SourceSpan::new(base_start, base_end);
    let base_resolved = normalize_type_path_text(&source[base_start..base_end])?;
    let type_arguments = generic_args
        .generic_args()
        .filter_map(|generic_arg| match generic_arg {
            ast::GenericArg::TypeArg(type_arg) => type_arg.ty(),
            _ => None,
        })
        .collect();
    Some((base_span, base_resolved, type_arguments))
}

/// Unwraps outer `Result<T, ...>` and `Poll<T>` return type envelopes from `type_node`.
fn unwrap_rust_return_envelope(type_node: ast::Type, source: &str) -> ast::Type {
    let mut current = type_node;
    while let ast::Type::PathType(ref path_type) = current {
        let Some((_, (base_path, base_terminal), type_args)) =
            extract_path_type_generics(path_type, source)
        else {
            break;
        };
        let is_result_or_poll = base_terminal == "Result"
            || (base_terminal == POLL_TYPE_NAME
                && matches!(
                    base_path.as_str(),
                    POLL_TYPE_NAME | "task::Poll" | "std::task::Poll" | "core::task::Poll"
                ));
        if is_result_or_poll && let Some(first_arg) = type_args.into_iter().next() {
            current = first_arg;
        } else {
            break;
        }
    }
    current
}

/// Unwraps transparent borrow and smart-pointer wrappers (`&T`, `&mut T`, `Box<T>`, `Rc<T>`,
/// `Arc<T>`, `Cow<'_, T>`) around a Rust type node.
fn unwrap_rust_pointer_wrappers(type_node: ast::Type, source: &str) -> ast::Type {
    let mut current = type_node;
    loop {
        match &current {
            ast::Type::RefType(ref_type) => {
                if let Some(inner) = ref_type.ty() {
                    current = inner;
                    continue;
                }
                break;
            }
            ast::Type::PathType(path_type) => {
                if let Some((_, (base_path, base_terminal), type_args)) =
                    extract_path_type_generics(path_type, source)
                {
                    let is_pointer_wrapper = match base_terminal.as_str() {
                        BOX_TYPE_NAME => matches!(
                            base_path.as_str(),
                            BOX_TYPE_NAME | "boxed::Box" | "std::boxed::Box" | "alloc::boxed::Box"
                        ),
                        RC_TYPE_NAME => matches!(
                            base_path.as_str(),
                            RC_TYPE_NAME | "rc::Rc" | "std::rc::Rc" | "alloc::rc::Rc"
                        ),
                        ARC_TYPE_NAME => matches!(
                            base_path.as_str(),
                            ARC_TYPE_NAME | "sync::Arc" | "std::sync::Arc" | "alloc::sync::Arc"
                        ),
                        COW_TYPE_NAME => matches!(
                            base_path.as_str(),
                            COW_TYPE_NAME
                                | "borrow::Cow"
                                | "std::borrow::Cow"
                                | "alloc::borrow::Cow"
                        ),
                        _ => false,
                    };
                    if is_pointer_wrapper
                        && type_args.len() == 1
                        && let Some(first_arg) = type_args.into_iter().next()
                    {
                        current = first_arg;
                        continue;
                    }
                }
                break;
            }
            _ => break,
        }
    }
    current
}

/// A Rust function or method item.
#[derive(Clone)]
pub struct RustFunction<'a> {
    /// Function identifier name.
    pub name: String,
    /// Function item AST node.
    pub node: AstNode<'a>,
    /// Declared return type annotation node, if present (`-> T`).
    pub return_type: Option<AstNode<'a>>,
    /// True if the function is inside a `trait` declaration or an `impl Trait for Type` block.
    pub is_trait_or_trait_impl: bool,
}

/// Returns true if `function` is declared inside a `trait` block or an `impl Trait for Type` block.
fn is_trait_or_trait_impl_function(function: &ast::Fn) -> bool {
    let Some(assoc_items) = function.syntax().parent() else {
        return false;
    };
    if !ast::AssocItemList::can_cast(assoc_items.kind()) {
        return false;
    }
    let Some(enclosing_item) = assoc_items.parent() else {
        return false;
    };
    ast::Trait::can_cast(enclosing_item.kind())
        || ast::Impl::cast(enclosing_item).is_some_and(|impl_item| impl_item.trait_().is_some())
}

/// Collects all function and method definitions in `file`, in source order.
#[must_use]
pub fn collect_functions(file: &ParsedFile) -> Vec<RustFunction<'_>> {
    let Some(parsed) = file.rs_parsed() else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for function in parsed
        .tree()
        .syntax()
        .descendants()
        .filter_map(ast::Fn::cast)
    {
        let Some(name_node) = function.name() else {
            continue;
        };
        let return_type = function
            .ret_type()
            .and_then(|ret| ret.ty())
            .map(|type_node| {
                AstNode::from_span(file, span_from_rowan_range(type_node.syntax().text_range()))
            });
        out.push(RustFunction {
            name: name_node.text().trim().to_owned(),
            return_type,
            is_trait_or_trait_impl: is_trait_or_trait_impl_function(&function),
            node: AstNode::from_span(file, span_from_rowan_range(function.syntax().text_range())),
        });
    }
    out
}

/// Returns `T` if `type_node` is `Option<T>` (unqualified, `std::option::Option`, or
/// `core::option::Option`).
fn option_payload(type_node: &ast::Type, source: &str) -> Option<ast::Type> {
    let ast::Type::PathType(path_type) = type_node else {
        return None;
    };
    let (_, (option_path, option_terminal), option_args) =
        extract_path_type_generics(path_type, source)?;
    if option_terminal != OPTION_TYPE_NAME
        || !matches!(
            option_path.as_str(),
            OPTION_TYPE_NAME | "option::Option" | "std::option::Option" | "core::option::Option"
        )
        || option_args.len() != 1
    {
        return None;
    }
    option_args.into_iter().next()
}

/// The payload `T` of an optional return type, as classified by [`nullable_return_payload`].
pub enum NullableReturnPayload {
    /// A slice or unsized array type (`[T]`), with its source text.
    Slice(String),
    /// A generic path type (`std::vec::Vec<T>`).
    Generic {
        /// Source text of the base path (`std::vec::Vec`).
        base: String,
        /// The base path with whitespace and any leading `::` removed.
        path: String,
        /// The last segment of the base path (`Vec`).
        terminal: String,
    },
}

/// Classifies the payload `T` of `return_type` if it is `Option<T>`.
///
/// Outer `Result<T, ...>` and `Poll<T>` envelopes are unwrapped first, then transparent borrow
/// and smart-pointer wrappers (`&T`, `&mut T`, `Box<T>`, `Rc<T>`, `Arc<T>`, `Cow<'_, T>`) around
/// `T`. Returns `None` if there is no `Option` or `T` is neither a slice nor a generic path type.
#[must_use]
pub fn nullable_return_payload(return_type: &AstNode<'_>) -> Option<NullableReturnPayload> {
    let source = &return_type.file.source;
    let return_type = cast_at_span::<ast::Type>(return_type)?;
    let payload = option_payload(&unwrap_rust_return_envelope(return_type, source), source)?;
    let inner = unwrap_rust_pointer_wrappers(payload, source);
    match &inner {
        ast::Type::SliceType(_) => {
            let span = span_from_rowan_range(inner.syntax().text_range());
            Some(NullableReturnPayload::Slice(
                source[span.start..span.end].trim().to_owned(),
            ))
        }
        ast::Type::ArrayType(array_type) if array_type.const_arg().is_none() => {
            let span = span_from_rowan_range(inner.syntax().text_range());
            Some(NullableReturnPayload::Slice(
                source[span.start..span.end].trim().to_owned(),
            ))
        }
        ast::Type::PathType(path_type) => {
            let (base_span, (path, terminal), _) = extract_path_type_generics(path_type, source)?;
            Some(NullableReturnPayload::Generic {
                base: source[base_span.start..base_span.end].trim().to_owned(),
                path,
                terminal,
            })
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagnostic::Language;
    use LiteralRole::{ConstantDefinition, Inline};

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

    #[test]
    fn test_summarize_rust_file_extracts_production_structure_and_paths() {
        let source = indoc::indoc! {r#"
            architecture_component!(CodeLintAst);

            mod detail;
            pub mod public_child;

            macro_rules! local_mac {
                () => {};
            }
            pub(crate) use local_mac;

            pub use self::detail::Exported;
            use crate::core::{self, Config as Cfg, tags::*};

            #[cfg(test)]
            fn ignored_test_helper() {
                use crate::forbidden::in_test;
                let _ = super::ignored::call();
            }

            const RAW_FIXTURE: &str = r"
            #[cfg(test)]
            mod fake_tests {}
            ";

            pub fn run(input: super::ParentType) {
                let _ = crate::a::b::Foo::<crate::c::d::Bar>::baz();
                assert!(super::detail::check(input));
            }
        "#};
        let file = ParsedFile::rust(source);
        let summary = summarize_rust_file(&file);
        let raw_paths: Vec<&str> = summary
            .referenced_paths
            .iter()
            .map(|reference| reference.raw_path.as_str())
            .collect();

        assert_eq!(
            (summary.architecture_components, summary.visible_uses),
            (
                vec!["CodeLintAst".to_string()],
                vec![VisibleUseDeclaration {
                    line: 11,
                    declaration_text: "pub use self::detail::Exported;".to_string(),
                    target_paths: vec!["self::detail::Exported".to_string()],
                }],
            )
        );
        assert_eq!(
            summary.external_mods,
            vec![
                ExternalModDeclaration {
                    name: "detail".to_string(),
                    declaration_text: "mod detail;".to_string(),
                    is_private: true,
                },
                ExternalModDeclaration {
                    name: "public_child".to_string(),
                    declaration_text: "pub mod public_child;".to_string(),
                    is_private: false,
                },
            ]
        );
        assert_eq!(
            raw_paths,
            vec![
                "local_mac",
                "self::detail::Exported",
                "crate::core",
                "crate::core::Config",
                "crate::core::tags",
                "super::ParentType",
                "crate::a::b::Foo",
                "crate::c::d::Bar",
                "super::detail::check",
            ]
        );
    }

    #[rstest::rstest]
    #[case::const_defines("const MAX: u32 = 30;", &[("30", ConstantDefinition)])]
    #[case::static_defines("static MIN: i32 = -5;", &[("-5", ConstantDefinition)])]
    #[case::enum_discriminant_defines("enum E { V = 10 }", &[("10", ConstantDefinition)])]
    #[case::static_mut_is_inline("static mut LIMIT: u32 = 30;", &[("30", Inline)])]
    #[case::composite_constant_not_collected("const NAMES: &[&str] = &[\"n1\", \"n2\"];", &[])]
    #[case::negation_anchored_on_operator(
        "fn f() { g(-42, 7 - 42); match x { -42 => {} _ => {} } }",
        &[("-42", Inline), ("7", Inline), ("42", Inline), ("-42", Inline)]
    )]
    #[case::signed_number_in_token_tree_skipped("fn f() { vec![-42, 43]; }", &[("43", Inline)])]
    #[case::tuple_position_skipped("fn f() { g(pair.3, 30); }", &[("30", Inline)])]
    #[case::attribute_skipped("#[cfg(feature = \"ff\")]\nfn f() {}", &[])]
    #[case::inner_attribute_skipped("#![doc = \"dd\"]", &[])]
    #[case::extern_abi_skipped("extern \"C\" { fn g(); }", &[])]
    #[case::exempt_macro_by_path_skipped("fn f() { tracing::warn!(\"ww\"); }", &[])]
    #[case::exempt_macro_inside_collected_macro_skipped(
        "fn f() { vec![format!(\"{}\", 5), \"vv\"]; }",
        &[("\"vv\"", Inline)]
    )]
    #[case::matches_macro_collected("fn f() { matches!(k, \"ma\"); }", &[("\"ma\"", Inline)])]
    #[case::char_skipped("fn f() { g('c'); }", &[])]
    #[case::c_string_skipped("fn f() { g(c\"cs\"); }", &[])]
    fn test_collect_literal_occurrences_rust(
        #[case] source: &str,
        #[case] expected: &[(&str, LiteralRole)],
    ) {
        let file = ParsedFile::rust(source);
        let actual: Vec<(String, LiteralRole)> = collect_literal_occurrences(&file)
            .into_iter()
            .map(|occurrence| (occurrence.node.text().into_owned(), occurrence.role))
            .collect();
        let expected: Vec<(String, LiteralRole)> = expected
            .iter()
            .map(|(text, role)| ((*text).to_string(), *role))
            .collect();
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_collect_literal_occurrences_rust_skips_every_exempt_macro() {
        for name in LITERAL_EXEMPT_MACROS {
            let file = ParsedFile::rust(&format!("fn f() {{ {name}!(\"ee\"); }}"));
            assert!(
                collect_literal_occurrences(&file).is_empty(),
                "`{name}!` arguments were collected"
            );
        }
    }

    #[rstest::rstest]
    #[case::raw_string("r#\"ab\"#", LiteralValue::Str("ab".to_string()))]
    #[case::raw_backslash_decoded("r\"a\\tb\"", LiteralValue::Str("a\\tb".to_string()))]
    #[case::raw_quote_decoded("r#\"a\"b\"#", LiteralValue::Str("a\"b".to_string()))]
    #[case::empty_raw_string("r\"\"", LiteralValue::Str(String::new()))]
    #[case::byte_string("b\"ab\"", LiteralValue::Bytes("ab".to_string()))]
    #[case::raw_byte_string("br\"ab\"", LiteralValue::Bytes("ab".to_string()))]
    #[case::integer_suffix("1_000_u32", LiteralValue::Int(1000))]
    #[case::hex_suffix("0xffu8", LiteralValue::Int(255))]
    #[case::hex_digits_ending_in_f32("0x1f32", LiteralValue::Int(0x1f32))]
    #[case::octal_suffix("0o17u8", LiteralValue::Int(15))]
    #[case::binary("0b11", LiteralValue::Int(3))]
    #[case::float_suffix_on_integer("1f32", LiteralValue::Float(1.0_f64.to_bits()))]
    #[case::float_suffix_after_separator("1_f32", LiteralValue::Float(1.0_f64.to_bits()))]
    #[case::float_suffix("1.5f64", LiteralValue::Float(1.5_f64.to_bits()))]
    #[case::exponent_with_suffix("1e3f32", LiteralValue::Float(1000.0_f64.to_bits()))]
    #[case::negative_zero("-0.0", LiteralValue::Float(0.0_f64.to_bits()))]
    fn test_collect_literal_occurrences_rust_values(
        #[case] literal: &str,
        #[case] expected: LiteralValue,
    ) {
        let file = ParsedFile::rust(&format!("fn f() {{ g({literal}); }}"));
        let values: Vec<LiteralValue> = collect_literal_occurrences(&file)
            .into_iter()
            .map(|occurrence| occurrence.value)
            .collect();
        assert_eq!(values, vec![expected]);
    }

    #[test]
    fn test_collect_literal_occurrences_rust_skips_overflow() {
        let file = ParsedFile::rust("fn f() { g(0xffff_ffff_ffff_ffff_ffff_ffff_ffff_ffff_f); }");
        assert!(collect_literal_occurrences(&file).is_empty());
    }

    #[test]
    fn test_collect_functions_rust() {
        let source = indoc::indoc! {r"
            fn free_fn() -> Option<u32> { None }
            trait T {
                fn trait_fn(&self) {}
            }
            impl T for S {
                fn trait_fn(&self) {}
            }
            impl S {
                fn inherent_method(&self) -> Vec<u8> { Vec::new() }
            }
        "};
        let file = ParsedFile::rust(source);
        let functions = collect_functions(&file);
        let actual: Vec<_> = functions
            .iter()
            .map(|func| {
                (
                    func.name.as_str(),
                    func.return_type
                        .as_ref()
                        .map(|ret_type| ret_type.text().to_string()),
                    func.is_trait_or_trait_impl,
                )
            })
            .collect();
        assert_eq!(
            actual,
            &[
                ("free_fn", Some("Option<u32>".to_owned()), false),
                ("trait_fn", None, true),
                ("trait_fn", None, true),
                ("inherent_method", Some("Vec<u8>".to_owned()), false),
            ]
        );
    }

    #[rstest::rstest]
    #[case::bare_option("fn f() -> Option<Vec<u8>> { None }", "Vec<u8>", "Vec<u8>")]
    #[case::result_option(
        "fn f() -> Result<Option<String>, ()> { Ok(None) }",
        "String",
        "String"
    )]
    #[case::borrowed_slice("fn f() -> Option<&'a [u8]> { None }", "&'a [u8]", "[u8]")]
    #[case::boxed_slice("fn f() -> Option<Box<[u8]>> { None }", "Box<[u8]>", "[u8]")]
    fn test_return_type_unwrapping_rust(
        #[case] source: &str,
        #[case] expected_payload: &str,
        #[case] expected_unwrapped: &str,
    ) {
        let file = ParsedFile::rust(source);
        let functions = collect_functions(&file);
        let return_type = cast_at_span::<ast::Type>(functions[0].return_type.as_ref().unwrap())
            .expect("return type");
        let unwrapped_envelope = unwrap_rust_return_envelope(return_type, &file.source);
        let payload = option_payload(&unwrapped_envelope, &file.source).expect("option payload");
        assert_eq!(payload.syntax().text().to_string(), expected_payload);
        let inner = unwrap_rust_pointer_wrappers(payload, &file.source);
        assert_eq!(inner.syntax().text().to_string(), expected_unwrapped);
    }
}
